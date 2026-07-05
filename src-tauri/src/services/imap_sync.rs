use crate::models::SyncResult;
use crate::services::parser;
use crate::services::store::{insert_email, InsertOutcome, NewAttachment, NewEmail, NewQuote};
use chrono::{Duration, Utc};
use mail_parser::MessageParser;
use rusqlite::{params, Connection, OptionalExtension};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// キャンセル要求が立っているか。
fn is_cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

/// 初回取得の安全上限（日数/全期間でも一度に取りすぎない）。
const SAFETY_MAX: usize = 2000;
/// uid_fetch のチャンクサイズ。進捗はチャンクごとに更新するため、小さめにして
/// 「フリーズに見える」のを防ぐ（大きいと一括DL中に無反応になる）。往復回数と
/// 応答性のバランスで 50 件。
const CHUNK: usize = 50;

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

pub type ImapSession = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

/// タイムアウト付きで IMAP に接続＋ログインし、Session を返す（接続の使い回し／プール用）。
/// imap::connect と違い TCP connect/read/write にタイムアウトを設定し、死んだ接続で固まらないようにする。
pub fn connect_login(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
) -> Result<ImapSession, String> {
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let addr = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|e| format!("名前解決に失敗: {e}"))?
        .next()
        .ok_or_else(|| "アドレスを解決できませんでした".to_string())?;
    let tcp = TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(20))
        .map_err(|e| format!("接続できませんでした: {e}"))?;
    let _ = tcp.set_read_timeout(Some(std::time::Duration::from_secs(60)));
    let _ = tcp.set_write_timeout(Some(std::time::Duration::from_secs(60)));
    let ssl = tls
        .connect(host, tcp)
        .map_err(|e| format!("TLS ハンドシェイクに失敗: {e}"))?;
    let mut client = imap::Client::new(ssl);
    client.read_greeting().map_err(|e| e.to_string())?;
    client.login(user, password).map_err(|(e, _)| e.to_string())
}

/// スロットのセッションが生きていれば再利用、なければ/死んでいれば張り直す（接続の使い回し）。
fn ensure_live_session(
    guard: &mut Option<ImapSession>,
    host: &str,
    port: u16,
    user: &str,
    password: &str,
) -> Result<(), String> {
    if let Some(s) = guard.as_mut() {
        if s.noop().is_ok() {
            return Ok(()); // 生きている → 再利用
        }
        // 死んでいる（サーバーが idle 切断した等）→ 下で張り直す。
    }
    *guard = Some(connect_login(host, port, user, password)?);
    Ok(())
}

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

/// 実際に IMAP ログインを試す（認証の確認）。成功なら Ok。タイムアウトつき。
pub fn test_login(host: &str, port: u16, user: &str, password: &str) -> Result<(), String> {
    log::info!("IMAP login test: host={host} port={port} user={user}");
    let mut session = connect_login(host, port, user, password).inspect_err(|e| {
        log::warn!("IMAP login test failed for user={user}: {e}");
    })?;
    log::info!("IMAP login OK: user={user}");
    let _ = session.logout();
    Ok(())
}

/// 既存セッション上で 受信箱＋標準フォルダ（送信済/下書き/ゴミ箱/迷惑）を同期する。
/// 接続の張り直し・ログアウトはしない（呼び出し側＝プールが管理する）。
#[allow(clippy::too_many_arguments)]
fn run_sync(
    session: &mut ImapSession,
    conn: &Connection,
    account_id: i64,
    window: &str,
    result: &mut SyncResult,
    progress: &dyn Fn(&str, i32, i32),
    cancel: &AtomicBool,
) -> Result<(), String> {
    // 受信箱（必須）。
    sync_folder(
        session, conn, account_id, "INBOX", "inbox", window, result, progress, cancel,
    )?;

    // その他の標準フォルダ（存在すれば best-effort。無ければスキップ）。
    // 中断要求があれば以降のフォルダはスキップして終了する。
    if !is_cancelled(cancel) {
        match session.list(Some(""), Some("*")) {
            Ok(names) => {
                let targets: Vec<(String, &'static str)> = SYNC_FOLDERS
                    .iter()
                    .filter_map(|spec| detect_mailbox(names.iter(), spec).map(|n| (n, spec.tag)))
                    .collect();
                drop(names);
                for (mbox, tag) in targets {
                    if is_cancelled(cancel) {
                        break;
                    }
                    if let Err(e) = sync_folder(
                        session, conn, account_id, &mbox, tag, window, result, progress, cancel,
                    ) {
                        log::warn!("フォルダ '{mbox}' ({tag}) の同期に失敗: {e}");
                    }
                }
            }
            Err(e) => log::warn!("フォルダ一覧の取得に失敗（受信箱のみ同期）: {e}"),
        }
    }
    Ok(())
}

/// IMAP に接続し、受信箱＋標準フォルダを同期する。接続は `slot` で使い回す（プール）。
/// 生きたセッションがあれば NOOP で確認して再利用し、無ければ張り直す。成功時はログアウトせず
/// セッションを slot に残す（次回の同期が接続＋ログインをやり直さない＝スムーズ）。失敗時は
/// セッションを捨てて次回張り直す。
#[allow(clippy::too_many_arguments)]
pub fn sync_account(
    db_path: &Path,
    account_id: i64,
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    progress: &dyn Fn(&str, i32, i32),
    cancel: &AtomicBool,
    slot: &Mutex<Option<ImapSession>>,
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

    let mut guard = slot.lock().unwrap();
    // 生きたセッションを用意（再利用 or 張り直し）。
    ensure_live_session(&mut guard, host, port, user, password)?;
    let session = guard.as_mut().unwrap();

    let mut result = SyncResult {
        fetched: 0,
        stored: 0,
        backfilled: 0,
    };
    match run_sync(
        session,
        &conn,
        account_id,
        &window,
        &mut result,
        progress,
        cancel,
    ) {
        Ok(()) => Ok(result), // セッションは slot に残して使い回す（ログアウトしない）。
        Err(e) => {
            // 壊れたセッションは捨てる（次回張り直す）。
            *guard = None;
            Err(e)
        }
    }
}

/// 1 フォルダを同期する（select → folder_sync 状態確認 → 取得 → 状態更新）。
/// 集計はアカウント全体の result に加算する。
#[allow(clippy::too_many_arguments)]
fn sync_folder(
    session: &mut ImapSession,
    conn: &Connection,
    account_id: i64,
    imap_name: &str,
    tag: &str,
    window: &str,
    result: &mut SyncResult,
    progress: &dyn Fn(&str, i32, i32),
    cancel: &AtomicBool,
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
        // 新着のみ: UID > last_uid。新しい方を先に取得・表示する（降順）。
        let last = stored_last_uid.unwrap() as u32;
        let mut uids: Vec<u32> = session
            .uid_search(format!("UID {}:*", last + 1))
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|&u| u > last)
            .collect();
        uids.sort_unstable();
        uids.reverse(); // 降順（新しい UID から）
        fetch_uids(
            session, conn, account_id, tag, &uids, &mut c, progress, cancel,
        )?;
    } else {
        match parse_scope(window) {
            Scope::Count(n) if total > 0 => {
                // 最新 n 件（シーケンス範囲で効率的に）。新しい順に保存する。
                let low = total.saturating_sub(n.saturating_sub(1)).max(1);
                let seq = format!("{}:{}", low, total);
                let msgs = session
                    .fetch(seq, "(UID FLAGS BODY[])")
                    .map_err(|e| e.to_string())?;
                let planned = msgs.len() as i32;
                progress(tag, 0, planned);
                if !is_cancelled(cancel) {
                    store_fetches(conn, account_id, tag, msgs.iter().rev(), &mut c)?;
                }
                progress(tag, c.fetched, planned);
            }
            Scope::Count(_) => { /* 空 */ }
            scope => {
                // 日付/全期間: UID SEARCH → 新しい順に上限まで → チャンク取得（降順）。
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
                uids.reverse(); // 降順（新しい UID から取得・表示）
                fetch_uids(
                    session, conn, account_id, tag, &uids, &mut c, progress, cancel,
                )?;
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

#[allow(clippy::too_many_arguments)]
fn fetch_uids(
    session: &mut ImapSession,
    conn: &Connection,
    account_id: i64,
    folder: &str,
    uids: &[u32],
    c: &mut Counters,
    progress: &dyn Fn(&str, i32, i32),
    cancel: &AtomicBool,
) -> Result<(), String> {
    let total = uids.len() as i32;
    progress(folder, 0, total);
    for chunk in uids.chunks(CHUNK) {
        // 中断要求があればチャンク境界で取得を止める（取得済みは保存済み）。
        if is_cancelled(cancel) {
            break;
        }
        let set = chunk
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let msgs = session
            .uid_fetch(set, "(UID FLAGS BODY[])")
            .map_err(|e| e.to_string())?;
        store_fetches(conn, account_id, folder, msgs.iter(), c)?;
        // チャンクごとに進捗を通知（取得済み / 予定件数）。
        progress(folder, c.fetched, total);
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
        // サーバー上の既読状態（\Seen）。未読数をサーバーと一致させる。
        let seen = m
            .flags()
            .iter()
            .any(|f| matches!(f, imap::types::Flag::Seen));
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
            let quotes = p
                .quotes
                .into_iter()
                .map(|q| NewQuote {
                    order: q.order,
                    quoted_from: q.quoted_from,
                    quoted_at: q.quoted_at,
                    fingerprint: q.fingerprint,
                })
                .collect();
            let ne = NewEmail {
                account_id,
                message_id: p.message_id,
                canonical_key: p.canonical_key,
                subject: p.subject,
                from_address: p.from_address,
                from_name: p.from_name,
                to_addresses: p.to_addresses,
                to_name: p.to_name,
                cc_addresses: p.cc_addresses,
                date: p.date,
                date_ts: p.date_ts,
                body_plain: p.body_plain,
                clean_body: p.clean_body,
                body_html: p.body_html,
                auth_result: p.auth_result,
                list_id: p.list_id,
                in_reply_to: p.in_reply_to,
                references_ids: p.references_ids,
                thread_index: p.thread_index,
                raw_headers: p.raw_headers,
                quotes,
                has_attachments: p.has_attachments,
                is_read: seen,
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

/// サーバーの Drafts フォルダを探して名前を返す（\Drafts→よくある名前 の順）。
fn find_drafts_mailbox<T: std::io::Read + std::io::Write>(
    session: &mut imap::Session<T>,
) -> Result<String, String> {
    let names = session
        .list(Some(""), Some("*"))
        .map_err(|e| e.to_string())?;
    let spec = SYNC_FOLDERS
        .iter()
        .find(|s| s.tag == "drafts")
        .expect("drafts spec");
    detect_mailbox(names.iter(), spec)
        .ok_or_else(|| "Drafts（下書き）フォルダが見つかりませんでした".to_string())
}

/// Drafts フォルダ内から、指定 Message-ID（山括弧なしの中身）の下書きを削除する。
/// 見つからなければ何もしない。フォルダは選択済みで呼ぶ。
fn expunge_draft_by_message_id<T: std::io::Read + std::io::Write>(
    session: &mut imap::Session<T>,
    message_id_inner: &str,
) -> Result<(), String> {
    // HEADER 検索は部分一致なので、山括弧なしの中身で自分の下書きだけを引ける。
    let uids = session
        .uid_search(format!("HEADER \"Message-ID\" \"{message_id_inner}\""))
        .map_err(|e| e.to_string())?;
    if uids.is_empty() {
        return Ok(());
    }
    let set = uids
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(",");
    session
        .uid_store(&set, "+FLAGS (\\Deleted)")
        .map_err(|e| e.to_string())?;
    // UIDPLUS が無いサーバーもあるので、uid_expunge がダメなら通常 expunge にフォールバック。
    let _ = session
        .uid_expunge(&set)
        .map(|_| ())
        .or_else(|_| session.expunge().map(|_| ()));
    Ok(())
}

/// 下書きをサーバーの Drafts フォルダへ APPEND する（既存の同 Message-ID は削除して入れ直す）。
/// これで「サーバー上に常に最新版が 1 通だけ」を保つ。best-effort。
pub fn upsert_draft(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    raw: &[u8],
    message_id_inner: &str,
) -> Result<(), String> {
    use imap::types::Flag;
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let client = imap::connect((host, port), host, &tls).map_err(|e| e.to_string())?;
    let mut session = client
        .login(user, password)
        .map_err(|(e, _)| e.to_string())?;

    let drafts = find_drafts_mailbox(&mut session)?;
    // 旧版を消してから新版を APPEND（IMAP は上書き不可のため）。
    session.select(&drafts).map_err(|e| e.to_string())?;
    let _ = expunge_draft_by_message_id(&mut session, message_id_inner);
    let result = session
        .append_with_flags(&drafts, raw, &[Flag::Draft, Flag::Seen])
        .map_err(|e| e.to_string());
    let _ = session.logout();
    result.map(|_| log::info!("下書きを Drafts フォルダ '{drafts}' に同期しました"))
}

/// サーバーの Drafts フォルダから、指定 Message-ID の下書きを削除する（送信済み/破棄時）。best-effort。
pub fn delete_draft_remote(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    message_id_inner: &str,
) -> Result<(), String> {
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let client = imap::connect((host, port), host, &tls).map_err(|e| e.to_string())?;
    let mut session = client
        .login(user, password)
        .map_err(|(e, _)| e.to_string())?;
    let drafts = find_drafts_mailbox(&mut session)?;
    session.select(&drafts).map_err(|e| e.to_string())?;
    let r = expunge_draft_by_message_id(&mut session, message_id_inner);
    let _ = session.logout();
    r
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
