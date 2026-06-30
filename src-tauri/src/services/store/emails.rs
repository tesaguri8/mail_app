use super::Store;
use crate::models::{MailDetail, MailSummary};
use rusqlite::{params, Connection, OptionalExtension};

/// メール挿入用（内部）。
pub struct NewEmail {
    pub account_id: i64,
    pub message_id: Option<String>,
    pub canonical_key: String,
    pub subject: Option<String>,
    pub from_address: Option<String>,
    pub to_addresses: Option<String>,
    pub date: Option<String>,
    pub body_plain: Option<String>,
    pub clean_body: Option<String>,
    pub has_attachments: bool,
}

/// 接続を直接受け取る挿入（同期スレッドの別接続から使うため）。
/// 重複（account_id, canonical_key）は INSERT OR IGNORE で無視し、None を返す。
pub fn insert_email(conn: &Connection, e: &NewEmail) -> rusqlite::Result<Option<i64>> {
    let changed = conn.execute(
        "INSERT OR IGNORE INTO emails
           (account_id, message_id, canonical_key, subject, from_address, to_addresses, date, has_attachments, body_plain, clean_body)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            e.account_id,
            e.message_id,
            e.canonical_key,
            e.subject,
            e.from_address,
            e.to_addresses,
            e.date,
            e.has_attachments as i64,
            e.body_plain,
            e.clean_body,
        ],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    let id = conn.last_insert_rowid();
    // FTS5（rowid = emails.id）
    conn.execute(
        "INSERT INTO email_fts(rowid, subject, from_address, clean_body) VALUES (?1, ?2, ?3, ?4)",
        params![id, e.subject, e.from_address, e.clean_body],
    )?;
    Ok(Some(id))
}

impl Store {
    pub fn list_emails(&self, account_id: i64, limit: i64) -> rusqlite::Result<Vec<MailSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, subject, from_address, date, is_read, has_attachments,
                    substr(COALESCE(clean_body, body_plain, ''), 1, 140) AS preview,
                    is_flagged, is_bookmarked
             FROM emails WHERE account_id = ?1 ORDER BY date DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![account_id, limit], |r| {
            Ok(MailSummary {
                id: r.get::<_, i64>(0)? as i32,
                subject: r.get(1)?,
                from_address: r.get(2)?,
                date: r.get(3)?,
                is_read: r.get::<_, i64>(4)? != 0,
                has_attachments: r.get::<_, i64>(5)? != 0,
                preview: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                is_starred: r.get::<_, i64>(7)? != 0,
                is_bookmarked: r.get::<_, i64>(8)? != 0,
            })
        })?;
        rows.collect()
    }

    /// 指定 ID 群に対し、フラグ列（is_read / is_starred / is_bookmarked）を一括更新する。
    fn set_flag(&self, column: &str, ids: &[i64], value: bool) -> rusqlite::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let sql = format!("UPDATE emails SET {column} = ?1 WHERE id = ?2");
        {
            let mut stmt = tx.prepare(&sql)?;
            for id in ids {
                stmt.execute(params![value as i64, id])?;
            }
        }
        tx.commit()
    }

    pub fn set_emails_read(&self, ids: &[i64], read: bool) -> rusqlite::Result<()> {
        self.set_flag("is_read", ids, read)
    }

    pub fn set_emails_starred(&self, ids: &[i64], value: bool) -> rusqlite::Result<()> {
        // お気に入り（スター）は IMAP の \Flagged に対応する is_flagged を使う。
        self.set_flag("is_flagged", ids, value)
    }

    pub fn set_emails_bookmarked(&self, ids: &[i64], value: bool) -> rusqlite::Result<()> {
        self.set_flag("is_bookmarked", ids, value)
    }

    /// メールを一括削除（FTS インデックスも削除）。
    pub fn delete_emails(&self, ids: &[i64]) -> rusqlite::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut fts = tx.prepare("DELETE FROM email_fts WHERE rowid = ?1")?;
            let mut del = tx.prepare("DELETE FROM emails WHERE id = ?1")?;
            for id in ids {
                fts.execute(params![id])?;
                del.execute(params![id])?;
            }
        }
        tx.commit()
    }

    /// メール本文の取得（表示用）。
    pub fn get_email(&self, id: i64) -> rusqlite::Result<Option<MailDetail>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, subject, from_address, to_addresses, date, clean_body, body_plain, has_attachments
             FROM emails WHERE id = ?1",
            params![id],
            |r| {
                Ok(MailDetail {
                    id: r.get::<_, i64>(0)? as i32,
                    subject: r.get(1)?,
                    from_address: r.get(2)?,
                    to_addresses: r.get(3)?,
                    date: r.get(4)?,
                    clean_body: r.get(5)?,
                    body_plain: r.get(6)?,
                    has_attachments: r.get::<_, i64>(7)? != 0,
                })
            },
        )
        .optional()
    }

    /// 既読にする。
    pub fn mark_read(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE emails SET is_read = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }
}
