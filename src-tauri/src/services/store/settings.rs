//! アプリ設定（非機密）のストア層（docs/SPAM.md §9）。
//! 汎用 key-value（app_settings）に保存し、Rust 側を設定の単一ソースにする。
//! 既定値はハードコードせず、各機能モジュールの定数を参照する（§9.2）。

use super::Store;
use crate::models::SpamSettings;
use crate::services::spam;
use rusqlite::{params, OptionalExtension};

/// 設定キー（単一ソース。UI 側もこの名前で読み書きする）。
pub const KEY_SPAM_ENABLED: &str = "spam.enabled";
pub const KEY_SPAM_THRESHOLD_LOW: &str = "spam.threshold_low";
pub const KEY_SPAM_THRESHOLD_HIGH: &str = "spam.threshold_high";

impl Store {
    /// 汎用設定の取得（未設定なら None）。
    pub fn get_setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()
    }

    /// 汎用設定の保存（upsert）。
    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }

    /// 迷惑メール設定を読む。未設定キーは既定値（spam モジュール定数）で補完する。
    pub fn spam_settings(&self) -> rusqlite::Result<SpamSettings> {
        // enabled は既定 true。"false"/"0" のみ無効扱い。
        let enabled = self
            .get_setting(KEY_SPAM_ENABLED)?
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        let threshold_low = self
            .get_setting(KEY_SPAM_THRESHOLD_LOW)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(spam::DEFAULT_THRESHOLD_LOW);
        let threshold_high = self
            .get_setting(KEY_SPAM_THRESHOLD_HIGH)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(spam::DEFAULT_THRESHOLD_HIGH);
        Ok(SpamSettings {
            enabled,
            threshold_low,
            threshold_high,
        })
    }

    /// 迷惑メール設定を保存する（呼び出し側で正規化済みを渡す）。
    pub fn set_spam_settings(&self, s: &SpamSettings) -> rusqlite::Result<()> {
        self.set_setting(KEY_SPAM_ENABLED, if s.enabled { "true" } else { "false" })?;
        self.set_setting(KEY_SPAM_THRESHOLD_LOW, &s.threshold_low.to_string())?;
        self.set_setting(KEY_SPAM_THRESHOLD_HIGH, &s.threshold_high.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::migrations;
    use super::*;
    use rusqlite::Connection;

    fn store() -> Store {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        Store {
            conn: std::sync::Mutex::new(conn),
            path: std::sync::Mutex::new(std::path::PathBuf::from(":memory:")),
        }
    }

    #[test]
    fn defaults_when_unset() {
        let s = store().spam_settings().unwrap();
        assert!(s.enabled);
        assert_eq!(s.threshold_low, spam::DEFAULT_THRESHOLD_LOW);
        assert_eq!(s.threshold_high, spam::DEFAULT_THRESHOLD_HIGH);
    }

    #[test]
    fn roundtrip_and_disable() {
        let store = store();
        store
            .set_spam_settings(&SpamSettings {
                enabled: false,
                threshold_low: 0.4,
                threshold_high: 0.8,
            })
            .unwrap();
        let s = store.spam_settings().unwrap();
        assert!(!s.enabled);
        assert_eq!(s.threshold_low, 0.4);
        assert_eq!(s.threshold_high, 0.8);
    }
}
