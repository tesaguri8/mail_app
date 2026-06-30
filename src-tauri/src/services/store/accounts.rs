use super::Store;
use crate::models::AccountSummary;
use rusqlite::params;

/// アカウント挿入用（内部）。資格情報は含めない（keyring で別管理）。
pub struct NewAccount {
    pub email: String,
    pub display_name: Option<String>,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
}

impl Store {
    pub fn insert_account(&self, a: &NewAccount) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO accounts (email, display_name, imap_host, imap_port, smtp_host, smtp_port)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                a.email,
                a.display_name,
                a.imap_host,
                a.imap_port,
                a.smtp_host,
                a.smtp_port
            ],
        )?;
        Ok(conn.last_insert_rowid())
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
