use crate::models::SyncResult;
use crate::services::parser;
use crate::services::store::{insert_email, InsertOutcome, NewAttachment, NewEmail};
use chrono::{Duration, Utc};
use mail_parser::MessageParser;
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

/// 実際に IMAP ログインを試す（認証の確認）。成功なら Ok。
pub fn test_login(host: &str, port: u16, user: &str, password: &str) -> Result<(), String> {
    log::info!(
        "IMAP login test: host={} port={} user={} (pw_len={})",
        host,
        port,
        user,
        password.len()
    );
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let client = imap::connect((host, port), host, &tls).map_err(|e| {
        log::warn!("IMAP connect failed: {}", e);
        e.to_string()
    })?;
    let mut session = client.login(user, password).map_err(|(e, _)| {
        log::warn!("IMAP login failed for user={}: {}", user, e);
        e.to_string()
    })?;
    log::info!("IMAP login OK: user={}", user);
    let _ = session.logout();
    Ok(())
}

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
    log::info!("sync: connect host={} port={} user={}", host, port, user);
    let client = imap::connect((host, port), host, &tls).map_err(|e| e.to_string())?;
    let mut session = client.login(user, password).map_err(|(e, _)| {
        log::warn!("sync: IMAP login failed for user={}: {}", user, e);
        e.to_string()
    })?;

    let mailbox = session.select("INBOX").map_err(|e| e.to_string())?;
    let uid_validity = mailbox.uid_validity;
    let total = mailbox.exists;

    let incremental = stored_validity.is_some()
        && stored_validity == uid_validity.map(|v| v as i64)
        && stored_last_uid.is_some();

    let mut c = Counters {
        fetched: 0,
        stored: 0,
        backfilled: 0,
        max_uid: stored_last_uid.unwrap_or(0) as u32,
    };

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
        fetch_uids(&mut session, &conn, account_id, &uids, &mut c)?;
    } else {
        match parse_scope(&window) {
            Scope::Count(n) if total > 0 => {
                // 最新 n 件（シーケンス範囲で効率的に）
                let low = total.saturating_sub(n.saturating_sub(1)).max(1);
                let seq = format!("{}:{}", low, total);
                let msgs = session
                    .fetch(seq, "(UID FLAGS BODY[])")
                    .map_err(|e| e.to_string())?;
                store_fetches(&conn, account_id, msgs.iter(), &mut c)?;
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
                fetch_uids(&mut session, &conn, account_id, &uids, &mut c)?;
            }
        }
    }

    // 同期状態を更新
    let _ = conn.execute(
        "UPDATE accounts SET uid_validity=?1, last_uid=?2 WHERE id=?3",
        params![uid_validity.map(|v| v as i64), c.max_uid as i64, account_id],
    );

    let _ = session.logout();
    Ok(SyncResult {
        fetched: c.fetched,
        stored: c.stored,
        backfilled: c.backfilled,
    })
}

/// 同期中の集計（取得/新規保存/埋め戻し/最大UID）。
struct Counters {
    fetched: i32,
    stored: i32,
    backfilled: i32,
    max_uid: u32,
}

fn fetch_uids(
    session: &mut ImapSession,
    conn: &Connection,
    account_id: i64,
    uids: &[u32],
    c: &mut Counters,
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
        store_fetches(conn, account_id, msgs.iter(), c)?;
    }
    Ok(())
}

fn store_fetches<'a>(
    conn: &Connection,
    account_id: i64,
    msgs: impl Iterator<Item = &'a imap::types::Fetch>,
    c: &mut Counters,
) -> Result<(), String> {
    for m in msgs {
        c.fetched += 1;
        if let Some(u) = m.uid {
            if u > c.max_uid {
                c.max_uid = u;
            }
        }
        let raw = match m.body() {
            Some(b) => b,
            None => continue,
        };
        let uid = m.uid.map(|u| u as i64);
        if let Some(p) = parser::parse_message(raw) {
            let attachments = p
                .attachments
                .into_iter()
                .map(|a| NewAttachment {
                    part_index: a.part_index,
                    filename: a.filename,
                    content_type: a.content_type,
                    size: a.size,
                    kind: a.kind,
                    content_id: a.content_id,
                })
                .collect();
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
                body_html: p.body_html,
                auth_result: p.auth_result,
                list_id: p.list_id,
                has_attachments: p.has_attachments,
                uid,
                attachments,
            };
            match insert_email(conn, &ne).map_err(|e| e.to_string())? {
                InsertOutcome::Inserted(_) => c.stored += 1,
                InsertOutcome::Backfilled => c.backfilled += 1,
                InsertOutcome::Unchanged => {}
            }
        }
    }
    Ok(())
}

/// 取得した添付の本体（バイト列・ファイル名・MIME型）。
pub struct FetchedAttachment {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub content_type: Option<String>,
}

/// 指定 UID のメッセージを再取得し、part_index 番目の添付本体を取り出す（オンデマンド）。
pub fn fetch_attachment(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    uid: u32,
    part_index: usize,
) -> Result<FetchedAttachment, String> {
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let client = imap::connect((host, port), host, &tls).map_err(|e| e.to_string())?;
    let mut session = client
        .login(user, password)
        .map_err(|(e, _)| e.to_string())?;
    session.select("INBOX").map_err(|e| e.to_string())?;

    let msgs = session
        .uid_fetch(uid.to_string(), "(BODY[])")
        .map_err(|e| e.to_string())?;
    let raw = msgs
        .iter()
        .next()
        .and_then(|m| m.body())
        .ok_or_else(|| "メッセージが見つかりませんでした".to_string())?;

    let msg = MessageParser::default()
        .parse(raw)
        .ok_or_else(|| "メッセージを解析できませんでした".to_string())?;
    let part = msg
        .attachment(part_index)
        .ok_or_else(|| "添付が見つかりませんでした".to_string())?;

    let bytes = part.contents().to_vec();
    let filename = parser::part_filename(part, part_index);
    let content_type = parser::part_content_type(part);

    let _ = session.logout();
    Ok(FetchedAttachment {
        bytes,
        filename,
        content_type,
    })
}

/// 指定 UID のメッセージ全体を再取得して解析する（本文の全文キャッシュ復元用）。
/// 要約保存に落とした本文をサーバーから取り直すときに使う。
pub fn fetch_message(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    uid: u32,
) -> Result<parser::ParsedEmail, String> {
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let client = imap::connect((host, port), host, &tls).map_err(|e| e.to_string())?;
    let mut session = client
        .login(user, password)
        .map_err(|(e, _)| e.to_string())?;
    session.select("INBOX").map_err(|e| e.to_string())?;

    let msgs = session
        .uid_fetch(uid.to_string(), "(BODY[])")
        .map_err(|e| e.to_string())?;
    let raw = msgs
        .iter()
        .next()
        .and_then(|m| m.body())
        .ok_or_else(|| "メッセージが見つかりませんでした".to_string())?;
    let parsed =
        parser::parse_message(raw).ok_or_else(|| "メッセージを解析できませんでした".to_string())?;
    let _ = session.logout();
    Ok(parsed)
}
