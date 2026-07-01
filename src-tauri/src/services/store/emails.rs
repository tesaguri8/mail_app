use super::Store;
use crate::models::{AttachmentSummary, MailDetail, MailSummary};
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
    pub body_html: Option<String>,
    pub has_attachments: bool,
    /// メッセージの IMAP UID（添付のオンデマンド再取得用）。
    pub uid: Option<i64>,
    /// 添付メタ（本体は未取得。ダウンロード時に再取得）。
    pub attachments: Vec<NewAttachment>,
}

/// 添付メタ挿入用（内部）。
pub struct NewAttachment {
    pub part_index: i64,
    pub filename: String,
    pub content_type: Option<String>,
    pub size: i64,
    pub kind: &'static str,
    pub content_id: Option<String>,
}

/// オンデマンド再取得に必要な情報（添付＋親メール）。
pub struct AttachmentFetchInfo {
    pub account_id: i64,
    /// 親メールの IMAP UID（None なら再取得不可＝要再同期）。
    pub email_uid: Option<i64>,
    pub part_index: i64,
    pub filename: String,
    /// 取得済みの保存先（未取得なら None）。
    pub file_path: Option<String>,
}

/// 添付メタ（本体は file_path NULL = 未取得）を一括挿入する。
fn insert_attachments(
    conn: &Connection,
    email_id: i64,
    atts: &[NewAttachment],
) -> rusqlite::Result<()> {
    if atts.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare(
        "INSERT INTO attachments (email_id, filename, content_type, size, part_index, kind, content_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for a in atts {
        stmt.execute(params![
            email_id,
            a.filename,
            a.content_type,
            a.size,
            a.part_index,
            a.kind,
            a.content_id
        ])?;
    }
    Ok(())
}

/// メール挿入の結果。
pub enum InsertOutcome {
    /// 新規挿入した（新しい email id）。
    Inserted(i64),
    /// 既存メールに uid/添付メタを埋め戻した。
    Backfilled,
    /// 既存メールで変更なし。
    Unchanged,
}

/// 接続を直接受け取る挿入（同期スレッドの別接続から使うため）。
/// 新規なら挿入して Inserted を返す。重複（account_id, canonical_key）の場合は
/// 新規挿入はしないが、機能追加前に取り込んだ古いメールでも添付が使えるよう、
/// uid と添付メタが未設定なら埋め戻して（バックフィル）Backfilled を返す。
pub fn insert_email(conn: &Connection, e: &NewEmail) -> rusqlite::Result<InsertOutcome> {
    // 表示専用の HTML 本文は zstd 圧縮して BLOB 列へ（TEXT の body_html は使わない）。
    let body_html_z = e
        .body_html
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(crate::services::compress::compress_text);
    let changed = conn.execute(
        "INSERT OR IGNORE INTO emails
           (account_id, message_id, canonical_key, subject, from_address, to_addresses, date, has_attachments, body_plain, clean_body, body_html_z, uid)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
            body_html_z,
            e.uid,
        ],
    )?;
    if changed == 0 {
        // 既存メール: uid / 添付メタを埋め戻す（再同期での後付け）。
        let did = backfill_existing(conn, e)?;
        return Ok(if did {
            InsertOutcome::Backfilled
        } else {
            InsertOutcome::Unchanged
        });
    }
    let id = conn.last_insert_rowid();
    // FTS5（rowid = emails.id）
    conn.execute(
        "INSERT INTO email_fts(rowid, subject, from_address, clean_body) VALUES (?1, ?2, ?3, ?4)",
        params![id, e.subject, e.from_address, e.clean_body],
    )?;
    insert_attachments(conn, id, &e.attachments)?;
    Ok(InsertOutcome::Inserted(id))
}

/// 既存メールに uid と添付メタを埋め戻す（再同期で古いメールを後付け対応）。
/// 何か変更したら true を返す。
fn backfill_existing(conn: &Connection, e: &NewEmail) -> rusqlite::Result<bool> {
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM emails WHERE account_id = ?1 AND canonical_key = ?2",
            params![e.account_id, e.canonical_key],
            |r| r.get(0),
        )
        .optional()?;
    let Some(id) = id else { return Ok(false) };
    let mut touched = false;

    // uid が未設定なら設定する（オンデマンド再取得に必要）。
    if e.uid.is_some() {
        let n = conn.execute(
            "UPDATE emails SET uid = ?1 WHERE id = ?2 AND uid IS NULL",
            params![e.uid, id],
        )?;
        touched |= n > 0;
    }
    // 添付行が無ければ挿入する（重複防止）。
    let existing: i64 = conn.query_row(
        "SELECT count(*) FROM attachments WHERE email_id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    if existing == 0 && !e.attachments.is_empty() {
        insert_attachments(conn, id, &e.attachments)?;
        touched = true;
    }
    Ok(touched)
}

impl Store {
    pub fn list_emails(&self, account_id: i64, limit: i64) -> rusqlite::Result<Vec<MailSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, subject, from_address, date, is_read, has_attachments,
                    substr(COALESCE(clean_body, body_plain, ''), 1, 140) AS preview,
                    is_flagged, is_bookmarked,
                    (SELECT group_concat(tag_id) FROM email_tags WHERE email_id = emails.id) AS tag_ids,
                    (emails.has_attachments = 1
                     OR EXISTS(SELECT 1 FROM attachments a WHERE a.email_id = emails.id AND COALESCE(a.kind, 'attachment') <> 'inline')) AS has_real
             FROM emails WHERE account_id = ?1 ORDER BY date DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![account_id, limit], |r| {
            // group_concat はカンマ区切り文字列。空（タグ無し）は None。
            let tag_ids = r
                .get::<_, Option<String>>(9)?
                .map(|s| s.split(',').filter_map(|p| p.parse::<i32>().ok()).collect())
                .unwrap_or_default();
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
                tag_ids,
                has_real_attachments: r.get::<_, i64>(10)? != 0,
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
            let mut etags = tx.prepare("DELETE FROM email_tags WHERE email_id = ?1")?;
            let mut att = tx.prepare("DELETE FROM attachments WHERE email_id = ?1")?;
            let mut del = tx.prepare("DELETE FROM emails WHERE id = ?1")?;
            for id in ids {
                fts.execute(params![id])?;
                etags.execute(params![id])?;
                att.execute(params![id])?; // FK 制約のため先に添付を削除
                del.execute(params![id])?;
            }
        }
        tx.commit()
    }

    /// メール本文の取得（表示用）。
    pub fn get_email(&self, id: i64) -> rusqlite::Result<Option<MailDetail>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, subject, from_address, to_addresses, date, clean_body, body_plain, body_html, body_html_z, has_attachments, body_compacted
             FROM emails WHERE id = ?1",
            params![id],
            |r| {
                // HTML 本文は圧縮列(body_html_z)を優先し展開。無ければ旧 TEXT 列を使う。
                let html_z: Option<Vec<u8>> = r.get(8)?;
                let body_html = match html_z {
                    Some(z) => crate::services::compress::decompress_text(&z).ok(),
                    None => r.get(7)?,
                };
                Ok(MailDetail {
                    id: r.get::<_, i64>(0)? as i32,
                    subject: r.get(1)?,
                    from_address: r.get(2)?,
                    to_addresses: r.get(3)?,
                    date: r.get(4)?,
                    clean_body: r.get(5)?,
                    body_plain: r.get(6)?,
                    body_html,
                    has_attachments: r.get::<_, i64>(9)? != 0,
                    body_compacted: r.get::<_, i64>(10)? != 0,
                })
            },
        )
        .optional()
    }

    /// 全文再取得に必要な情報（親メールの account_id と IMAP UID）。
    /// UID が None のメールは再取得不可（要再同期）。
    pub fn email_refetch_info(&self, email_id: i64) -> rusqlite::Result<Option<(i64, Option<i64>)>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT account_id, uid FROM emails WHERE id = ?1",
            params![email_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
    }

    /// 本文を上書きして全文キャッシュを復元する（要約保存の解除）。
    /// HTML は再圧縮して body_html_z に格納し、body_compacted=0 に戻す。FTS も更新。
    pub fn update_email_body(
        &self,
        id: i64,
        body_plain: Option<&str>,
        clean_body: Option<&str>,
        body_html: Option<&str>,
    ) -> rusqlite::Result<()> {
        let body_html_z = body_html
            .filter(|s| !s.is_empty())
            .map(crate::services::compress::compress_text);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE emails
             SET body_plain = ?1, clean_body = ?2, body_html_z = ?3, body_html = NULL, body_compacted = 0
             WHERE id = ?4",
            params![body_plain, clean_body, body_html_z, id],
        )?;
        // FTS5（clean_body 索引）も更新する。
        conn.execute(
            "UPDATE email_fts SET clean_body = ?1 WHERE rowid = ?2",
            params![clean_body, id],
        )?;
        Ok(())
    }

    /// 既読にする。
    pub fn mark_read(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE emails SET is_read = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 旧 TEXT 列に残る body_html を一度だけ zstd 圧縮して body_html_z へ移す。
    /// 起動時に呼ぶ。処理済み（body_html IS NULL）の行は対象外なので2回目以降は no-op。
    /// 圧縮した件数を返す。
    pub fn compress_legacy_bodies(&self) -> rusqlite::Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        let rows: Vec<(i64, String)> = {
            let mut stmt = conn.prepare(
                "SELECT id, body_html FROM emails
                 WHERE body_html IS NOT NULL AND body_html <> '' AND body_html_z IS NULL",
            )?;
            let mapped =
                stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        };
        if rows.is_empty() {
            return Ok(0);
        }
        let tx = conn.transaction()?;
        {
            let mut up =
                tx.prepare("UPDATE emails SET body_html_z = ?1, body_html = NULL WHERE id = ?2")?;
            for (id, html) in &rows {
                let z = crate::services::compress::compress_text(html);
                up.execute(params![z, id])?;
            }
        }
        tx.commit()?;
        Ok(rows.len())
    }

    fn map_attachment(r: &rusqlite::Row) -> rusqlite::Result<AttachmentSummary> {
        let file_path: Option<String> = r.get(4)?;
        Ok(AttachmentSummary {
            id: r.get::<_, i64>(0)? as i32,
            filename: r.get(1)?,
            content_type: r.get(2)?,
            size: r.get::<_, Option<i64>>(3)?.unwrap_or(0) as i32,
            is_downloaded: file_path.is_some(),
            file_path,
            kind: r.get(5)?,
            content_id: r.get(6)?,
        })
    }

    /// メールの添付メタ一覧（序数順）。
    pub fn list_attachments(&self, email_id: i64) -> rusqlite::Result<Vec<AttachmentSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, filename, content_type, size, file_path, kind, content_id
             FROM attachments WHERE email_id = ?1 ORDER BY part_index",
        )?;
        let rows = stmt.query_map(params![email_id], Self::map_attachment)?;
        rows.collect()
    }

    /// 添付 1 件のメタ。
    pub fn get_attachment(&self, id: i64) -> rusqlite::Result<Option<AttachmentSummary>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, filename, content_type, size, file_path, kind, content_id FROM attachments WHERE id = ?1",
            params![id],
            Self::map_attachment,
        )
        .optional()
    }

    /// オンデマンド再取得に必要な情報を取得する。
    pub fn attachment_fetch_info(
        &self,
        attachment_id: i64,
    ) -> rusqlite::Result<Option<AttachmentFetchInfo>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT e.account_id, e.uid, a.part_index, a.filename, a.file_path
             FROM attachments a JOIN emails e ON e.id = a.email_id
             WHERE a.id = ?1",
            params![attachment_id],
            |r| {
                Ok(AttachmentFetchInfo {
                    account_id: r.get(0)?,
                    email_uid: r.get(1)?,
                    part_index: r.get(2)?,
                    filename: r.get(3)?,
                    file_path: r.get(4)?,
                })
            },
        )
        .optional()
    }

    /// ダウンロード完了を記録（保存先・簡易チェックサム・最終アクセス時刻）。
    pub fn set_attachment_downloaded(
        &self,
        attachment_id: i64,
        path: &str,
        checksum: Option<&str>,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE attachments SET file_path = ?1, checksum = ?2, accessed_at = datetime('now') WHERE id = ?3",
            params![path, checksum, attachment_id],
        )?;
        Ok(())
    }

    /// 添付の最終アクセス時刻を更新（表示/オープン時。LRU エビクションの保護に使う）。
    pub fn touch_attachment(&self, attachment_id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE attachments SET accessed_at = datetime('now') WHERE id = ?1",
            params![attachment_id],
        )?;
        Ok(())
    }
}
