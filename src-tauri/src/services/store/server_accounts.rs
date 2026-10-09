use super::Store;
use crate::models::ServerAccountSummary;
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;

/// メールサーバーアカウント設定（接続＋ログイン）の作成入力。
pub struct NewServerAccount {
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
}

impl Store {
    /// (imap_host, imap_port, username) が一致する設定があれば再利用、無ければ作成。
    pub fn find_or_create_server_account(&self, s: &NewServerAccount) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        if let Some(id) = conn
            .query_row(
                "SELECT id FROM server_accounts WHERE imap_host=?1 AND imap_port=?2 AND username=?3",
                params![s.imap_host, s.imap_port, s.username],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
        {
            return Ok(id);
        }
        conn.execute(
            "INSERT INTO server_accounts (name, imap_host, imap_port, smtp_host, smtp_port, username)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                s.imap_host,
                s.imap_host,
                s.imap_port,
                s.smtp_host,
                s.smtp_port,
                s.username
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_server_accounts(&self) -> rusqlite::Result<Vec<ServerAccountSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut users: HashMap<i64, Vec<i32>> = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT server_account_id, id FROM accounts \
                 WHERE server_account_id IS NOT NULL ORDER BY COALESCE(sort_order, id), id",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
            for row in rows {
                let (server, account) = row?;
                users.entry(server).or_default().push(account as i32);
            }
        }
        let mut stmt = conn.prepare(
            "SELECT id, name, imap_host, imap_port, smtp_host, smtp_port, username
             FROM server_accounts ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            let id: i64 = r.get(0)?;
            Ok(ServerAccountSummary {
                id: id as i32,
                name: r.get(1)?,
                imap_host: r.get(2)?,
                imap_port: r.get::<_, i64>(3)? as u16,
                smtp_host: r.get(4)?,
                smtp_port: r.get::<_, i64>(5)? as u16,
                username: r.get(6)?,
                account_ids: users.remove(&id).unwrap_or_default(),
            })
        })?;
        rows.collect()
    }
}
