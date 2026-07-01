use super::Store;
use crate::models::AccountSummary;
use rusqlite::params;

/// アカウント挿入用（内部）。資格情報は含めない（keyring で別管理）。
pub struct NewAccount {
    pub email: String,
    pub display_name: Option<String>,
    pub username: Option<String>,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub server_account_id: Option<i64>,
}

impl Store {
    pub fn insert_account(&self, a: &NewAccount) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO accounts (email, display_name, username, imap_host, imap_port, smtp_host, smtp_port, server_account_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                a.email,
                a.display_name,
                a.username,
                a.imap_host,
                a.imap_port,
                a.smtp_host,
                a.smtp_port,
                a.server_account_id
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// 同期に必要な IMAP 接続情報（email, login_user, host, port）を取得。
    /// login_user = username があればそれ、無ければ email。
    pub fn get_account_imap(
        &self,
        id: i64,
    ) -> rusqlite::Result<Option<(String, String, String, u16)>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT email, COALESCE(NULLIF(username, ''), email), imap_host, imap_port
             FROM accounts WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)? as u16,
                ))
            },
        )
        .map(Some)
        .or_else(|e| {
            if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                Ok(None)
            } else {
                Err(e)
            }
        })
    }

    pub fn list_accounts(&self) -> rusqlite::Result<Vec<AccountSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, email, display_name, imap_host, smtp_host, COALESCE(sync_window,'6m'), signature_id,
                    (SELECT COUNT(*) FROM emails e WHERE e.account_id = accounts.id AND e.is_read = 0),
                    (SELECT COUNT(*) FROM emails e WHERE e.account_id = accounts.id)
             FROM accounts ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(AccountSummary {
                id: r.get::<_, i64>(0)? as i32,
                email: r.get(1)?,
                display_name: r.get(2)?,
                imap_host: r.get(3)?,
                smtp_host: r.get(4)?,
                sync_window: r.get(5)?,
                signature_id: r.get::<_, Option<i64>>(6)?.map(|v| v as i32),
                unread_count: r.get::<_, i64>(7)? as i32,
                total_count: r.get::<_, i64>(8)? as i32,
            })
        })?;
        rows.collect()
    }

    /// アカウントの編集（差出人名・既定署名）。
    pub fn update_account(
        &self,
        id: i64,
        display_name: Option<&str>,
        signature_id: Option<i64>,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET display_name = ?1, signature_id = ?2 WHERE id = ?3",
            params![display_name, signature_id, id],
        )?;
        Ok(())
    }

    /// 同期範囲を変更。次回同期で新範囲を初回取得し直せるよう UID 状態もリセットする。
    /// アカウントと、その受信メール（FTS含む）を削除する。
    pub fn delete_account(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM email_fts WHERE rowid IN (SELECT id FROM emails WHERE account_id=?1)",
            params![id],
        )?;
        // FK 制約のため、メール本体より先に添付を削除する。
        conn.execute(
            "DELETE FROM attachments WHERE email_id IN (SELECT id FROM emails WHERE account_id=?1)",
            params![id],
        )?;
        conn.execute(
            "DELETE FROM email_tags WHERE email_id IN (SELECT id FROM emails WHERE account_id=?1)",
            params![id],
        )?;
        conn.execute("DELETE FROM emails WHERE account_id=?1", params![id])?;
        conn.execute("DELETE FROM accounts WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn set_sync_window(&self, id: i64, window: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET sync_window=?1, uid_validity=NULL, last_uid=NULL WHERE id=?2",
            params![window, id],
        )?;
        Ok(())
    }
}
