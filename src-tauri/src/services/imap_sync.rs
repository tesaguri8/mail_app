use crate::models::SyncResult;
use crate::services::parser;
use crate::services::store::{insert_email, NewEmail};
use chrono::{Duration, Utc};
use rusqlite::{params, Connection};
use std::path::Path;

/// 初回取得の安全上限（日数/全期間でも一度に取りすぎない）。
const SAFETY_MAX: usize = 2000;
/// uid_fetch のチャンクサイズ。
const CHUNK: usize = 200;

/// 同期範囲（accounts.sync_window をパース）。
/// "n50"=最新50件 / "3d"=3日 / "30d" / "3m" / "1y" / "all"
enum Scope {
    Count(u32),
    Days(i64),
    All,
}

fn parse_scope(w: &str) -> Scope {
    let w = w.trim().to_lowercase();
    if w == "all" {
        return Scope::All;
    }
    if let Some(n) = w.strip_prefix('n') {
        if let Ok(c) = n.parse::<u32>() {
            return Scope::Count(c);
        }
    }
    if let Some(n) = w.strip_suffix('d') {
        if let Ok(d) = n.parse::<i64>() {
            return Scope::Days(d);
        }
    }
    if let Some(n) = w.strip_suffix('m') {
        if let Ok(m) = n.parse::<i64>() {
            return Scope::Days(m * 30);
        }
    }
    if let Some(n) = w.strip_suffix('y') {
        if let Ok(y) = n.parse::<i64>() {
            return Scope::Days(y * 365);
        }
    }
    Scope::Days(180) // 既定 6ヶ月相当
}

fn since_date(days: i64) -> String {
    (Utc::now() - Duration::days(days))
        .format("%d-%b-%Y")
        .to_string()
}

type ImapSession = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

/// IMAP に接続し、初回は sync_window の範囲、以降は新着 UID だけを取得して保存する。
pub fn sync_account(
    db_path: &Path,
    account_id: i64,
    host: &str,
    port: u16,
    user: &str,
    password: &str,
) -> Result<SyncResult, String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let _ = conn.execute_batch("PRAGMA foreign_keys=ON;");

    // 同期状態を読む
    let (window, stored_validity, stored_last_uid): (String, Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT COALESCE(sync_window,'6m'), uid_validity, last_uid FROM accounts WHERE id=?1",
            params![account_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| e.to_string())?;

    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let client = imap::connect((host, port), host, &tls).map_err(|e| e.to_string())?;
    let mut session = client.login(user, password).map_err(|(e, _)| e.to_string())?;

    let mailbox = session.select("INBOX").map_err(|e| e.to_string())?;
    let uid_validity = mailbox.uid_validity;
    let total = mailbox.exists;

    let incremental = stored_validity.is_some()
        && stored_validity == uid_validity.map(|v| v as i64)
        && stored_last_uid.is_some();

    let mut fetched = 0i32;
    let mut stored = 0i32;
    let mut max_uid: u32 = stored_last_uid.unwrap_or(0) as u32;

    // 取得対象を決める
    if incremental {
        // 新着のみ: UID > last_uid
        let last = stored_last_uid.unwrap() as u32;
        let mut uids: Vec<u32> = session
            .uid_search(format!("UID {}:*", last + 1))
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|&u| u > last)
            .collect();
        uids.sort_unstable();
        fetch_uids(&mut session, &conn, account_id, &uids, &mut fetched, &mut stored, &mut max_uid)?;
    } else {
        match parse_scope(&window) {
            Scope::Count(n) if total > 0 => {
                // 最新 n 件（シーケンス範囲で効率的に）
                let low = total.saturating_sub(n.saturating_sub(1)).max(1);
                let seq = format!("{}:{}", low, total);
                let msgs = session
                    .fetch(seq, "(UID FLAGS BODY[])")
                    .map_err(|e| e.to_string())?;
                store_fetches(&conn, account_id, msgs.iter(), &mut fetched, &mut stored, &mut max_uid)?;
            }
            Scope::Count(_) => { /* 空 */ }
            scope => {
                // 日付/全期間: UID SEARCH → 新しい順に上限まで → チャンク取得
                let criterion = match scope {
                    Scope::Days(d) => format!("SINCE {}", since_date(d)),
                    _ => "ALL".to_string(),
                };
                let mut uids: Vec<u32> = session
                    .uid_search(criterion)
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .collect();
                uids.sort_unstable();
                if uids.len() > SAFETY_MAX {
                    uids = uids.split_off(uids.len() - SAFETY_MAX); // 新しい方を残す
                }
                fetch_uids(&mut session, &conn, account_id, &uids, &mut fetched, &mut stored, &mut max_uid)?;
            }
        }
    }

    // 同期状態を更新
    let _ = conn.execute(
        "UPDATE accounts SET uid_validity=?1, last_uid=?2 WHERE id=?3",
        params![uid_validity.map(|v| v as i64), max_uid as i64, account_id],
    );

    let _ = session.logout();
    Ok(SyncResult { fetched, stored })
}

fn fetch_uids(
    session: &mut ImapSession,
    conn: &Connection,
    account_id: i64,
    uids: &[u32],
    fetched: &mut i32,
    stored: &mut i32,
    max_uid: &mut u32,
) -> Result<(), String> {
    for chunk in uids.chunks(CHUNK) {
        let set = chunk
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let msgs = session
            .uid_fetch(set, "(UID FLAGS BODY[])")
            .map_err(|e| e.to_string())?;
        store_fetches(conn, account_id, msgs.iter(), fetched, stored, max_uid)?;
    }
    Ok(())
}

fn store_fetches<'a>(
    conn: &Connection,
    account_id: i64,
    msgs: impl Iterator<Item = &'a imap::types::Fetch>,
    fetched: &mut i32,
    stored: &mut i32,
    max_uid: &mut u32,
) -> Result<(), String> {
    for m in msgs {
        *fetched += 1;
        if let Some(u) = m.uid {
            if u > *max_uid {
                *max_uid = u;
            }
        }
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
            if insert_email(conn, &ne).map_err(|e| e.to_string())?.is_some() {
                *stored += 1;
            }
        }
    }
    Ok(())
}
