use crate::models::SyncResult;
use crate::services::parser;
use crate::services::store::{insert_email, InsertOutcome, NewAttachment, NewEmail};
use chrono::{Duration, Utc};
use mail_parser::MessageParser;
use rusqlite::{params, Connection, OptionalExtension};
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

/// 同期する標準フォルダの定義（ローカルタグ／特殊用途属性／よくある名前候補）。
/// 受信箱(INBOX)は固定なのでここには含めない。
struct FolderSpec {
    /// ローカルの保存タグ（emails.folder）。
    tag: &'static str,
    /// RFC 6154 の特殊用途属性（\Sent 等）。
    special_use: &'static str,
    /// 特殊用途で決まらない場合のよくあるフォルダ名（末端名で照合）。
    names: &'static [&'static str],
}

const SYNC_FOLDERS: &[FolderSpec] = &[
    FolderSpec {
        tag: "sent",
        special_use: "\\Sent",
        names: &[
            "Sent",
            "Sent Messages",
            "Sent Items",
            "送信済みトレイ",
            "送信済みメール",
            "送信済み",
        ],
    },
    FolderSpec {
        tag: "drafts",
        special_use: "\\Drafts",
        names: &["Drafts", "Draft", "下書き", "草稿"],
    },
    FolderSpec {
        tag: "trash",
        special_use: "\\Trash",
        names: &[
            "Trash",
            "Deleted",
            "Deleted Messages",
            "Deleted Items",
            "ごみ箱",
            "ゴミ箱",
        ],
    },
    FolderSpec {
        tag: "spam",
        special_use: "\\Junk",
        names: &[
            "Junk",
            "Spam",
            "Junk E-mail",
            "Junk Email",
            "迷惑メール",
            "迷惑",
            "スパム",
        ],
    },
];

/// フォルダ一覧から該当メールボックス名を判定する。
/// 1) 特殊用途属性（\Sent 等。RFC 6154 SPECIAL-USE。システム属性以外は Custom で来る）。
/// 2) よくある名前（末端名 or フルネームで大小無視の一致）。
fn detect_mailbox<'a>(
    names: impl IntoIterator<Item = &'a imap::types::Name>,
    spec: &FolderSpec,
) -> Option<String> {
    use imap::types::NameAttribute;
    let list: Vec<&imap::types::Name> = names.into_iter().collect();
    for n in &list {
        let hit = n.attributes().iter().any(
            |a| matches!(a, NameAttribute::Custom(c) if c.eq_ignore_ascii_case(spec.special_use)),
        );
        if hit {
            return Some(n.name().to_string());
        }
    }
    for n in &list {
        let full = n.name();
        let leaf = full.rsplit(['/', '.']).next().unwrap_or(full);
        if spec
            .names
            .iter()
            .any(|c| leaf.eq_ignore_ascii_case(c) || full.eq_ignore_ascii_case(c))
        {
            return Some(full.to_string());
        }
    }
    None
}

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

/// IMAP に接続し、受信箱＋標準フォルダ（送信済/下書き/ゴミ箱/迷惑）を同期する。
/// 各フォルダは初回は sync_window の範囲、以降は新着 UID だけを取得して保存する。
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

    let window: String = conn
        .query_row(
            "SELECT COALESCE(sync_window,'6m') FROM accounts WHERE id=?1",
            params![account_id],
            |r| r.get(0),
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

    let mut result = SyncResult {
        fetched: 0,
        stored: 0,
        backfilled: 0,
    };

    // 受信箱（必須）。
    sync_folder(
        &mut session,
        &conn,
        account_id,
        "INBOX",
        "inbox",
        &window,
        &mut result,
    )?;

    // その他の標準フォルダ（存在すれば best-effort。無ければスキップ）。
    match session.list(Some(""), Some("*")) {
        Ok(names) => {
            let targets: Vec<(String, &'static str)> = SYNC_FOLDERS
                .iter()
                .filter_map(|spec| detect_mailbox(names.iter(), spec).map(|n| (n, spec.tag)))
                .collect();
            drop(names);
            for (mbox, tag) in targets {
                if let Err(e) = sync_folder(
                    &mut session,
                    &conn,
                    account_id,
                    &mbox,
                    tag,
                    &window,
                    &mut result,
                ) {
                    log::warn!("フォルダ '{mbox}' ({tag}) の同期に失敗: {e}");
                }
            }
        }
        Err(e) => log::warn!("フォルダ一覧の取得に失敗（受信箱のみ同期）: {e}"),
    }

    let _ = session.logout();
    Ok(result)
}

/// 1 フォルダを同期する（select → folder_sync 状態確認 → 取得 → 状態更新）。
/// 集計はアカウント全体の result に加算する。
fn sync_folder(
    session: &mut ImapSession,
    conn: &Connection,
    account_id: i64,
    imap_name: &str,
    tag: &str,
    window: &str,
    result: &mut SyncResult,
) -> Result<(), String> {
    let mailbox = session.select(imap_name).map_err(|e| e.to_string())?;
    let uid_validity = mailbox.uid_validity;
    let total = mailbox.exists;

    let (stored_validity, stored_last_uid): (Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT uid_validity, last_uid FROM folder_sync WHERE account_id=?1 AND folder=?2",
            params![account_id, tag],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or((None, None));

    let incremental = stored_validity.is_some()
        && stored_validity == uid_validity.map(|v| v as i64)
        && stored_last_uid.is_some();

    let mut c = Counters {
        fetched: 0,
        stored: 0,
        backfilled: 0,
        max_uid: stored_last_uid.unwrap_or(0) as u32,
    };

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
        fetch_uids(session, conn, account_id, tag, &uids, &mut c)?;
    } else {
        match parse_scope(window) {
            Scope::Count(n) if total > 0 => {
                // 最新 n 件（シーケンス範囲で効率的に）
                let low = total.saturating_sub(n.saturating_sub(1)).max(1);
                let seq = format!("{}:{}", low, total);
                let msgs = session
                    .fetch(seq, "(UID FLAGS BODY[])")
                    .map_err(|e| e.to_string())?;
                store_fetches(conn, account_id, tag, msgs.iter(), &mut c)?;
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
                fetch_uids(session, conn, account_id, tag, &uids, &mut c)?;
            }
        }
    }

    // フォルダ別の同期状態を更新（upsert）。
    conn.execute(
        "INSERT INTO folder_sync (account_id, folder, uid_validity, last_uid)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(account_id, folder)
         DO UPDATE SET uid_validity=excluded.uid_validity, last_uid=excluded.last_uid",
        params![
            account_id,
            tag,
            uid_validity.map(|v| v as i64),
            c.max_uid as i64
        ],
    )
    .map_err(|e| e.to_string())?;

    result.fetched += c.fetched;
    result.stored += c.stored;
    result.backfilled += c.backfilled;
    Ok(())
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
    folder: &str,
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
        store_fetches(conn, account_id, folder, msgs.iter(), c)?;
    }
    Ok(())
}

fn store_fetches<'a>(
    conn: &Connection,
    account_id: i64,
    folder: &str,
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
                folder: folder.to_string(),
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

/// 送信済みメッセージを IMAP の Sent フォルダへ保存する（APPEND）。best-effort。
/// Sent フォルダ名はサーバーで異なるため、特殊用途属性(\Sent)→よくある名前 の順で判定する。
/// Sent が見つからないときはエラーを返す（呼び出し側で送信自体は成功扱いにする）。
pub fn append_to_sent(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    raw: &[u8],
) -> Result<(), String> {
    use imap::types::Flag;
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let client = imap::connect((host, port), host, &tls).map_err(|e| e.to_string())?;
    let mut session = client
        .login(user, password)
        .map_err(|(e, _)| e.to_string())?;

    let names = session
        .list(Some(""), Some("*"))
        .map_err(|e| e.to_string())?;
    let sent_spec = SYNC_FOLDERS
        .iter()
        .find(|s| s.tag == "sent")
        .expect("sent spec");
    let sent = detect_mailbox(names.iter(), sent_spec)
        .ok_or_else(|| "Sent（送信済み）フォルダが見つかりませんでした".to_string())?;
    drop(names);

    // 送信控えは自分で送ったものなので既読(\Seen)で入れる。
    let result = session
        .append_with_flags(&sent, raw, &[Flag::Seen])
        .map_err(|e| e.to_string());
    let _ = session.logout();
    result.map(|_| {
        log::info!("送信控えを Sent フォルダ '{sent}' に保存しました");
    })
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
