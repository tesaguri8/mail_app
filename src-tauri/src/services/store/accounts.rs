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
            "SELECT id, email, display_name, imap_host, smtp_host FROM accounts ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(AccountSummary {
                id: r.get::<_, i64>(0)? as i32,
                email: r.get(1)?,
                display_name: r.get(2)?,
                imap_host: r.get(3)?,
                smtp_host: r.get(4)?,
            })
        })?;
        rows.collect()
    }
}
