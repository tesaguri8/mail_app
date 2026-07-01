//! ローカル保存容量の管理（アカウント毎の添付キャッシュ上限とエビクション）。
//!
//! 上限を超えたら、保護対象（スター付き）以外の**古いメールの添付バイト**から順に
//! 追い出す（実ファイル削除＋file_path/checksum を NULL）。メタ情報は常に残すので、
//! UI ではボタンが「ダウンロード」に戻り、必要時に再取得できる。正本はサーバー。

use super::Store;
use crate::models::EvictionReport;
use rusqlite::params;

impl Store {
    /// アカウントのダウンロード済み添付の合計バイト。
    pub fn storage_used(&self, account_id: i64) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(SUM(a.size), 0)
             FROM attachments a JOIN emails e ON e.id = a.email_id
             WHERE e.account_id = ?1 AND a.file_path IS NOT NULL",
            params![account_id],
            |r| r.get(0),
        )
    }

    /// アカウントの容量上限（バイト）。未設定は既定 2GB。
    pub fn storage_limit(&self, account_id: i64) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(storage_limit, 2147483648) FROM accounts WHERE id = ?1",
            params![account_id],
            |r| r.get(0),
        )
    }

    /// 容量上限を設定する（バイト）。
    pub fn set_storage_limit(&self, account_id: i64, bytes: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET storage_limit = ?1 WHERE id = ?2",
            params![bytes, account_id],
        )?;
        Ok(())
    }

    /// 同期状態（uid_validity/last_uid）をリセットして次回フル再取得させる。
    pub fn reset_sync_state(&self, account_id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET uid_validity = NULL, last_uid = NULL WHERE id = ?1",
            params![account_id],
        )?;
        Ok(())
    }

    /// 上限を超えていれば、保護（スター付き）以外の古い添付から順に追い出す。
    /// 実ファイルを削除し file_path/checksum を NULL にする（メタは残す）。
    pub fn evict_over_limit(&self, account_id: i64) -> rusqlite::Result<EvictionReport> {
        let limit = self.storage_limit(account_id)?;
        let mut used = self.storage_used(account_id)?;
        if used <= limit {
            return Ok(EvictionReport {
                evicted: 0,
                freed_bytes: 0.0,
            });
        }

        // 古い順（メール日付昇順）に、スター以外のダウンロード済み添付を集める。
        let candidates: Vec<(i64, i64, String)> = {
            let conn = self.conn.lock().unwrap();
            // LRU: 最後に使ってから古い順（accessed_at 昇順、未設定は最古扱い）。
            let mut stmt = conn.prepare(
                "SELECT a.id, COALESCE(a.size, 0), a.file_path
                 FROM attachments a JOIN emails e ON e.id = a.email_id
                 WHERE e.account_id = ?1 AND a.file_path IS NOT NULL AND e.is_flagged = 0
                 ORDER BY a.accessed_at IS NOT NULL, a.accessed_at ASC, e.date ASC",
            )?;
            let rows = stmt.query_map(params![account_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let mut evicted = 0i32;
        let mut freed = 0i64;
        for (id, size, path) in candidates {
            if used <= limit {
                break;
            }
            // 実ファイル削除（存在しなくても DB は掃除する）。
            let _ = std::fs::remove_file(&path);
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "UPDATE attachments SET file_path = NULL, checksum = NULL WHERE id = ?1",
                params![id],
            )?;
            drop(conn);
            used -= size;
            freed += size;
            evicted += 1;
        }

        Ok(EvictionReport {
            evicted,
            freed_bytes: freed as f64,
        })
    }
}
