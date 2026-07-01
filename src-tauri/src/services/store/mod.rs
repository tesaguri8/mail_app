mod accounts;
mod contacts;
mod emails;
mod migrations;
mod server_accounts;
mod signatures;
mod storage;
mod tags;

pub use accounts::NewAccount;
pub use emails::{insert_email, AttachmentFetchInfo, InsertOutcome, NewAttachment, NewEmail};
pub use server_accounts::NewServerAccount;

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// SQLite ストア。接続を Mutex で包み、Tauri の管理状態として共有する。
/// 暗号化（SQLCipher）は後続でフィーチャ差し替え（PRAGMA key を追加）。
pub struct Store {
    pub conn: Mutex<Connection>,
    /// 現在の mail.db パス。保存先の移動（relocate）で差し替わるため内部可変。
    path: Mutex<PathBuf>,
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
            path: Mutex::new(path.to_path_buf()),
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

    /// 現在の mail.db パス。
    pub fn path(&self) -> PathBuf {
        self.path.lock().unwrap().clone()
    }

    /// mail.db と attachments を置いているフォルダ。
    pub fn data_dir(&self) -> PathBuf {
        self.path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }

    /// データ（mail.db + WAL/SHM + attachments）を `new_dir` へ移動する。
    /// 再起動不要: VACUUM INTO で整合コピー → ポインタ更新 → ライブ接続を差し替え → 旧削除。
    /// `pointer_file` に新しい保存先を書き込む（データの外に置く記録。datadir 参照）。
    pub fn relocate(&self, new_dir: &Path, pointer_file: &Path) -> Result<(), String> {
        let old_db = self.path();
        let old_dir = old_db
            .parent()
            .ok_or("現在のデータフォルダが不明です")?
            .to_path_buf();
        if new_dir == old_dir {
            return Err("移動先が現在と同じ場所です".into());
        }
        // 移動先が現在のフォルダの内側だと自己コピーになるため拒否。
        if new_dir.starts_with(&old_dir) {
            return Err("移動先を現在のフォルダの内側にはできません".into());
        }
        let new_db = new_dir.join("mail.db");
        if new_db.exists() {
            return Err("移動先に既に mail.db があります".into());
        }
        std::fs::create_dir_all(new_dir).map_err(|e| format!("移動先を作成できません: {e}"))?;

        // 重い処理の間は接続ロックを保持し、他のクエリと直列化する。
        let mut guard = self.conn.lock().unwrap();

        // 1) WAL を主ファイルへ反映し、新しい場所へ整合スナップショットをコピー。
        guard
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| format!("チェックポイントに失敗: {e}"))?;
        let ver: i64 = guard
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let esc = new_db.to_string_lossy().replace('\'', "''");
        guard
            .execute_batch(&format!("VACUUM INTO '{esc}';"))
            .map_err(|e| format!("DBのコピーに失敗: {e}"))?;

        // 2) 添付キャッシュをコピー。
        let old_att = old_dir.join("attachments");
        if old_att.exists() {
            copy_dir_all(&old_att, &new_dir.join("attachments"))
                .map_err(|e| format!("添付のコピーに失敗: {e}"))?;
        }

        // 3) 新ファイルへ user_version を引き継ぐ（VACUUM INTO が落とす場合の保険）。
        {
            let c = Connection::open(&new_db).map_err(|e| e.to_string())?;
            c.execute_batch(&format!("PRAGMA user_version = {ver};"))
                .map_err(|e| e.to_string())?;
        }

        // 4) 旧を消す前にポインタを更新（途中でクラッシュしても新側が正となり安全）。
        std::fs::write(pointer_file, new_dir.to_string_lossy().as_bytes())
            .map_err(|e| format!("保存先の記録に失敗: {e}"))?;

        // 5) ライブ接続を新DBへ差し替え（旧接続が閉じてファイルロックが外れる）。
        let newc = Connection::open(&new_db).map_err(|e| e.to_string())?;
        newc.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| e.to_string())?;
        *guard = newc;
        drop(guard);
        *self.path.lock().unwrap() = new_db;

        // 6) 旧ファイルを削除（best-effort）。
        let _ = std::fs::remove_file(&old_db);
        let _ = std::fs::remove_file(old_dir.join("mail.db-wal"));
        let _ = std::fs::remove_file(old_dir.join("mail.db-shm"));
        let _ = std::fs::remove_dir_all(&old_att);
        Ok(())
    }
}

/// ディレクトリを再帰コピーする。
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}
