mod accounts;
mod migrations;

pub use accounts::NewAccount;

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
        Ok(Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
        })
    }

    /// 現在のスキーマバージョン（PRAGMA user_version）。
    pub fn schema_version(&self) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
    }
}
