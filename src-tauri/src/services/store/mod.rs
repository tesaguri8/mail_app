mod accounts;
mod emails;
mod migrations;
mod server_accounts;
mod settings;
mod signatures;
mod spam;
mod storage;
mod tags;

pub use accounts::{NewAccount, SmtpAccount};
pub use emails::{insert_email, AttachmentFetchInfo, InsertOutcome, NewAttachment, NewEmail};
pub use server_accounts::NewServerAccount;
pub use spam::SpamFeatures;

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// SQLite ストア。接続を Mutex で包み、Tauri の管理状態として共有する。
/// 暗号化（SQLCipher）は後続でフィーチャ差し替え（PRAGMA key を追加）。
pub struct Store {
    pub conn: Mutex<Connection>,
    pub path: PathBuf,
}

impl Store {
    /// DB を開き（無ければ作成）、未適用マイグレーションを順次適用する。
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        migrations::run(&conn)?;
        let store = Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
        };
        // 旧 TEXT 本文を一度だけ圧縮列へ移す（初回のみ実行、以降は no-op）。
        match store.compress_legacy_bodies() {
            Ok(n) if n > 0 => {
                log::info!("compressed {n} legacy HTML bodies");
                // 解放ページを実ファイルに反映（圧縮した初回だけ実行）。
                // WAL モードでは VACUUM だけだと主ファイルが縮まないので、
                // チェックポイントで WAL を反映・切り詰めてから VACUUM し、再度切り詰める。
                let conn = store.conn.lock().unwrap();
                if let Err(e) = conn.execute_batch(
                    "PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA wal_checkpoint(TRUNCATE);",
                ) {
                    log::warn!("compaction after compression failed: {e}");
                }
            }
            Ok(_) => {}
            Err(e) => log::warn!("legacy body compression skipped: {e}"),
        }
        Ok(store)
    }

    /// 現在のスキーマバージョン（PRAGMA user_version）。
    pub fn schema_version(&self) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
    }
}
