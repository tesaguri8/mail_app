use crate::models::SyncResult;
use crate::services::parser;
use crate::services::store::{insert_email, NewEmail};
use rusqlite::Connection;
use std::path::Path;

/// IMAP に接続して INBOX の直近 `limit` 件を取得し、解析して DB に保存する（PoC）。
/// ブロッキング処理のため、呼び出し側で spawn_blocking 等に載せること。
/// 同期スレッド用に DB へは新しい接続を開く（WAL のため並行可）。
pub fn sync_account(
    db_path: &Path,
    account_id: i64,
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    limit: u32,
) -> Result<SyncResult, String> {
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let client = imap::connect((host, port), host, &tls).map_err(|e| e.to_string())?;
    let mut session = client.login(user, password).map_err(|(e, _)| e.to_string())?;

    let mailbox = session.select("INBOX").map_err(|e| e.to_string())?;
    let total = mailbox.exists;

    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let _ = conn.execute_batch("PRAGMA foreign_keys=ON;");

    let mut fetched = 0i32;
    let mut stored = 0i32;

    if total > 0 {
        let start = total.saturating_sub(limit.saturating_sub(1)).max(1);
        let seq = format!("{}:{}", start, total);
        let messages = session
            .fetch(seq, "(FLAGS INTERNALDATE BODY[])")
            .map_err(|e| e.to_string())?;

        for m in messages.iter() {
            fetched += 1;
            let raw = match m.body() {
                Some(b) => b,
                None => continue,
            };
            if let Some(p) = parser::parse_message(raw) {
                let ne = NewEmail {
                    account_id,
                    message_id: p.message_id,
                    canonical_key: p.canonical_key,
                    subject: p.subject,
                    from_address: p.from_address,
                    to_addresses: p.to_addresses,
                    date: p.date,
                    body_plain: p.body_plain,
                    clean_body: p.clean_body,
                    has_attachments: p.has_attachments,
                };
                if insert_email(&conn, &ne).map_err(|e| e.to_string())?.is_some() {
                    stored += 1;
                }
            }
        }
    }

    let _ = session.logout();
    Ok(SyncResult { fetched, stored })
}
