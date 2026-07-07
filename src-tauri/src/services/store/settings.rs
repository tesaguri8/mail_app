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
/// ゴミ箱（連絡先・組織の論理削除）の保持日数。0 で即時完全削除、上限は緩め。
pub const KEY_TRASH_RETENTION_DAYS: &str = "trash.retention_days";
pub const DEFAULT_TRASH_RETENTION_DAYS: i64 = 7;
/// メールのゴミ箱（trash フォルダ）の保持日数。0 = 無期限（自動削除しない）、1 以上で N 日後に完全削除。
/// 連絡先用（上記）とは別系統。既定は 30 日。
pub const KEY_MAIL_TRASH_RETENTION_DAYS: &str = "trash.mail_retention_days";
pub const DEFAULT_MAIL_TRASH_RETENTION_DAYS: i64 = 30;
/// 差出人ごとの外部画像許可（このアドレスは常に許可）の設定キー接頭辞。
/// キーは `remote_images.sender.<小文字メール>`（docs/MAIL_SECURITY.md §1）。
const KEY_REMOTE_IMAGES_SENDER_PREFIX: &str = "remote_images.sender.";

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

    /// ゴミ箱の保持日数を読む（未設定なら既定 7 日。負値は 0 に丸める）。
    pub fn trash_retention_days(&self) -> rusqlite::Result<i64> {
        Ok(self
            .get_setting(KEY_TRASH_RETENTION_DAYS)?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(DEFAULT_TRASH_RETENTION_DAYS)
            .max(0))
    }

    /// ゴミ箱の保持日数を保存する（負値は 0 に丸める）。
    pub fn set_trash_retention_days(&self, days: i64) -> rusqlite::Result<()> {
        self.set_setting(KEY_TRASH_RETENTION_DAYS, &days.max(0).to_string())
    }

    /// メールのゴミ箱の保持日数を読む（未設定なら既定 30 日。負値は 0=無期限に丸める）。
    pub fn mail_trash_retention_days(&self) -> rusqlite::Result<i64> {
        Ok(self
            .get_setting(KEY_MAIL_TRASH_RETENTION_DAYS)?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(DEFAULT_MAIL_TRASH_RETENTION_DAYS)
            .max(0))
    }

    /// メールのゴミ箱の保持日数を保存する（負値は 0=無期限 に丸める）。
    pub fn set_mail_trash_retention_days(&self, days: i64) -> rusqlite::Result<()> {
        self.set_setting(KEY_MAIL_TRASH_RETENTION_DAYS, &days.max(0).to_string())
    }

    /// 迷惑メール設定を保存する（呼び出し側で正規化済みを渡す）。
    pub fn set_spam_settings(&self, s: &SpamSettings) -> rusqlite::Result<()> {
        self.set_setting(KEY_SPAM_ENABLED, if s.enabled { "true" } else { "false" })?;
        self.set_setting(KEY_SPAM_THRESHOLD_LOW, &s.threshold_low.to_string())?;
        self.set_setting(KEY_SPAM_THRESHOLD_HIGH, &s.threshold_high.to_string())?;
        Ok(())
    }

    /// 差出人アドレスの外部画像を常に許可するか。
    /// 明示設定（KV）を最優先し、無ければ住所録の `allow_remote_images`（信頼済み連絡先）を見る。
    pub fn remote_images_allowed_for(&self, email: &str) -> rusqlite::Result<bool> {
        let addr = email.trim().to_lowercase();
        if addr.is_empty() {
            return Ok(false);
        }
        // 1) 明示設定（この差出人を常に許可/解除）。
        let key = format!("{KEY_REMOTE_IMAGES_SENDER_PREFIX}{addr}");
        if let Some(v) = self.get_setting(&key)? {
            return Ok(v == "1" || v == "true");
        }
        // 2) 住所録で信頼済み（allow_remote_images=1）の連絡先なら許可。
        let conn = self.conn.lock().unwrap();
        let allowed: bool = conn
            .query_row(
                "SELECT 1 FROM contacts \
                 WHERE lower(email) = ?1 AND allow_remote_images = 1 LIMIT 1",
                params![addr],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);
        Ok(allowed)
    }

    /// 差出人アドレスの外部画像許可（常に許可/解除）を保存する。
    pub fn set_remote_images_allowed_for(&self, email: &str, allow: bool) -> rusqlite::Result<()> {
        let addr = email.trim().to_lowercase();
        if addr.is_empty() {
            return Ok(());
        }
        let key = format!("{KEY_REMOTE_IMAGES_SENDER_PREFIX}{addr}");
        self.set_setting(&key, if allow { "1" } else { "0" })
    }
}

#[cfg(test)]
mod tests {
    use super::super::migrations;
    use super::*;
    use rusqlite::Connection;

    fn store() -> Store {
        Store::open_in_memory_for_test()
    }

    #[test]
    fn defaults_when_unset() {
        let s = store().spam_settings().unwrap();
        assert!(s.enabled);
        assert_eq!(s.threshold_low, spam::DEFAULT_THRESHOLD_LOW);
        assert_eq!(s.threshold_high, spam::DEFAULT_THRESHOLD_HIGH);
    }

    #[test]
    fn mail_trash_retention_default_and_roundtrip() {
        let store = store();
        // 未設定なら既定 30 日。
        assert_eq!(store.mail_trash_retention_days().unwrap(), 30);
        // 保存して読み戻し。
        store.set_mail_trash_retention_days(7).unwrap();
        assert_eq!(store.mail_trash_retention_days().unwrap(), 7);
        // 0 = 無期限、負値は 0 に丸める。
        store.set_mail_trash_retention_days(-5).unwrap();
        assert_eq!(store.mail_trash_retention_days().unwrap(), 0);
        // 連絡先用（既定 7）とは独立。
        assert_eq!(store.trash_retention_days().unwrap(), 7);
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
