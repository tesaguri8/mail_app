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
/// 全件メタデータ索引（docs/SYNC.md §3.6）の 1 同期あたりの取得上限（ヘッダのみ）。
/// 「古いのは急がない」ので少しずつ。UI の快適さを優先し控えめに（挿入→process_pending の
/// スレッド割当が書き込みロックを取るため、多いと UI 操作が待たされる）。定期同期で徐々に埋める。
const BACKFILL_PER_SYNC: usize = 400;
/// 本文バックフィル（保証期間内の未取得本文を全文取得。docs/SYNC.md）の 1 同期あたりの取得上限。
/// 本文はヘッダより重いので控えめに。定期同期で少しずつ「保証期間ぶんの全文」を揃える。
const BODY_BACKFILL_PER_SYNC: usize = 60;

/// body_window（テキスト全文の保証期間）を日数に。'all'/'off'/'' は None（＝期間指定なし）。
/// 任意年数（'5y' 等）や '<n>d'/'<n>m'/'<n>y' も受ける（storage.rs の window_days と揃える）。
fn body_window_days(w: &str) -> Option<i64> {
    let w = w.trim().to_lowercase();
    match w.as_str() {
        "all" | "off" | "" => None,
        "7d" => Some(7),
        "30d" => Some(30),
        "3m" => Some(90),
        "6m" => Some(180),
        "1y" => Some(365),
        "2y" => Some(730),
        other => {
            let parse = |suffix: char| other.strip_suffix(suffix).and_then(|s| s.parse::<i64>().ok());
            if let Some(n) = parse('d') {
                Some(n)
            } else if let Some(n) = parse('m') {
                Some(n * 30)
            } else {
                parse('y').map(|n| n * 365)
            }
        }
    }
}

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
    // 同期対象フォルダ（受信箱＋存在する標準フォルダ）を一覧する。
    // ※ names はセッションを借用するので、必要な名前を owned で取り出してから drop する。
    let mut folders: Vec<(String, &'static str)> = vec![("INBOX".to_string(), "inbox")];
    match session.list(Some(""), Some("*")) {
        Ok(names) => {
            for spec in SYNC_FOLDERS {
                if let Some(n) = detect_mailbox(names.iter(), spec) {
                    folders.push((n, spec.tag));
                }
            }
            drop(names);
        }
        Err(e) => log::warn!("フォルダ一覧の取得に失敗（受信箱のみ同期）: {e}"),
    }

    // Pass 1: 新着（増分/初回）を全フォルダ分。新しいものを最優先で取り込む（docs/SYNC.md §3.6）。
    for (mbox, tag) in &folders {
        if is_cancelled(cancel) {
            return Ok(());
        }
        if let Err(e) = sync_folder(
            session, conn, account_id, mbox, tag, window, result, progress, cancel,
        ) {
            log::warn!("フォルダ '{mbox}' ({tag}) の同期に失敗: {e}");
        }
    }

    // Pass 2: 古い側のメタデータ索引を少しずつ（低優先・再開可能・本文なし）。docs/SYNC.md §3.6。
    // 新着（Pass 1）を全フォルダ終えてから走らせるので「新しいもの優先」を保つ。
    for (mbox, tag) in &folders {
        if is_cancelled(cancel) {
            break;
        }
        if let Err(e) =
            backfill_folder_metadata(session, conn, account_id, mbox, tag, result, progress, cancel)
        {
            log::warn!("フォルダ '{mbox}' ({tag}) のメタ索引に失敗: {e}");
        }
    }

    // Pass 3: 保証期間（body_window）内で本文が未取得のメールを少しずつ全文取得する
    // （低優先・再開可能・容量上限尊重・PEEK で既読にしない）。設定「テキスト全文を確実に
    // 残す期間」ぶんの本文を、開かなくても手元に揃える。docs/SYNC.md §3.6。
    let body_window: String = conn
        .query_row(
            "SELECT COALESCE(body_window,'off') FROM accounts WHERE id=?1",
            params![account_id],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| "off".to_string());
    for (mbox, tag) in &folders {
        if is_cancelled(cancel) {
            break;
        }
        if let Err(e) = backfill_folder_bodies(
            session,
            conn,
            account_id,
            mbox,
            tag,
            &body_window,
            result,
            progress,
            cancel,
        ) {
            log::warn!("フォルダ '{mbox}' ({tag}) の本文バックフィルに失敗: {e}");
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
    // busy_timeout: WAL でも書き込みロックは 1 つ。UI/ローカル加工/他アカウント同期と競合した
    // とき即エラー(database is locked)にせず待つ。他の接続（Store・process_pending）と揃える。
    // メタ索引バックフィルで書き込みが増えて競合が顕在化したため必須。
    let _ = conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;");

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
        "INSERT INTO folder_sync (account_id, folder, uid_validity, last_uid, server_total)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(account_id, folder)
         DO UPDATE SET uid_validity=excluded.uid_validity, last_uid=excluded.last_uid,
                       server_total=excluded.server_total",
        params![
            account_id,
            tag,
            uid_validity.map(|v| v as i64),
            c.max_uid as i64,
            // サーバ総数（IMAP SELECT の EXISTS）。左下「ローカル/サーバ」表示に使う。
            total as i64
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

/// 解析結果 ParsedEmail を挿入用 NewEmail へ写す（フル取得・ヘッダのみ取得で共通）。
/// ヘッダのみ解析なら本文3列は空になり、insert_email 側で body_state='absent' になる。
/// account の self_secret（16進）を取り出す（未生成なら None）。
fn account_self_secret(conn: &Connection, account_id: i64) -> Option<String> {
    conn.query_row(
        "SELECT self_secret FROM accounts WHERE id = ?1",
        params![account_id],
        |r| r.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

/// 受信メールが「本物の自分から」か（X-Rondine-Self を account の秘密で HMAC 検証。docs/SPAM.md）。
fn is_verified_self(secret: &Option<String>, p: &parser::ParsedEmail) -> bool {
    match (
        secret.as_deref(),
        p.x_rondine_self.as_deref(),
        p.message_id.as_deref(),
    ) {
        (Some(sec), Some(mark), Some(mid)) if !sec.is_empty() => {
            crate::services::selfmark::verify_mark(sec, mid, mark)
        }
        _ => false,
    }
}

fn parsed_to_new_email(
    p: parser::ParsedEmail,
    account_id: i64,
    folder: &str,
    seen: bool,
    uid: Option<i64>,
    verified_self: bool,
) -> NewEmail {
    // 「本物の自分から」がサーバーの迷惑フォルダに入っていたら受信箱に出す（ローカルで迷惑解除）。
    let folder = if verified_self && folder == "spam" {
        "inbox"
    } else {
        folder
    };
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
    NewEmail {
        account_id,
        message_id: p.message_id,
        canonical_key: p.canonical_key,
        subject: p.subject,
        from_address: p.from_address,
        from_name: p.from_name,
        to_addresses: p.to_addresses,
        to_name: p.to_name,
        reply_to: p.reply_to,
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
        verified_self,
        attachments,
    }
}

fn store_fetches<'a>(
    conn: &Connection,
    account_id: i64,
    folder: &str,
    msgs: impl Iterator<Item = &'a imap::types::Fetch>,
    c: &mut Counters,
) -> Result<(), String> {
    let self_secret = account_self_secret(conn, account_id);
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
            let verified = is_verified_self(&self_secret, &p);
            let ne = parsed_to_new_email(p, account_id, folder, seen, uid, verified);
            match insert_email(conn, &ne).map_err(|e| e.to_string())? {
                InsertOutcome::Inserted(_) => c.stored += 1,
                InsertOutcome::Backfilled => c.backfilled += 1,
                InsertOutcome::Unchanged => {}
            }
        }
    }
    Ok(())
}

/// メタのみ行の書き込み（BODY.PEEK[HEADER] をそのまま parse_message へ）。ヘッダのみなので
/// 本文3列は空 → insert_email 側で body_state='absent'。canonical_key 等はフル取得と一致し、
/// 後で本文取得しても同じ行に統合される（重複しない）。docs/SYNC.md §3.6。
fn store_header_fetches<'a>(
    conn: &Connection,
    account_id: i64,
    folder: &str,
    msgs: impl Iterator<Item = &'a imap::types::Fetch>,
    result: &mut SyncResult,
) -> Result<(), String> {
    let self_secret = account_self_secret(conn, account_id);
    for m in msgs {
        let raw = match m.header() {
            Some(h) => h,
            None => continue,
        };
        let uid = m.uid.map(|u| u as i64);
        let seen = m
            .flags()
            .iter()
            .any(|f| matches!(f, imap::types::Flag::Seen));
        if let Some(p) = parser::parse_message(raw) {
            let verified = is_verified_self(&self_secret, &p);
            let ne = parsed_to_new_email(p, account_id, folder, seen, uid, verified);
            if let InsertOutcome::Inserted(_) = insert_email(conn, &ne).map_err(|e| e.to_string())? {
                result.backfilled += 1;
            }
        }
    }
    Ok(())
}

/// このフォルダの全件メタ索引が完了した印を付ける（これ以上古い UID が無い）。
fn mark_index_complete(conn: &Connection, account_id: i64, tag: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE folder_sync SET index_complete=1 WHERE account_id=?1 AND folder=?2",
        params![account_id, tag],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 古い側のメタデータを少しずつ索引する（本文なし・再開可能）。docs/SYNC.md §3.6。
/// フロンティア = ローカル保存済みの最小 UID。これ未満の UID を新しい方から
/// BACKFILL_PER_SYNC 件だけヘッダ取得して挿入する。これ以上古い UID が無ければ完了印。
/// 30 秒間隔の定期同期で呼ばれ、毎回少しずつ過去を埋める（アプリ再起動も DB を見て続き）。
#[allow(clippy::too_many_arguments)]
fn backfill_folder_metadata(
    session: &mut ImapSession,
    conn: &Connection,
    account_id: i64,
    imap_name: &str,
    tag: &str,
    result: &mut SyncResult,
    progress: &dyn Fn(&str, i32, i32),
    cancel: &AtomicBool,
) -> Result<(), String> {
    // 完了済みフォルダ・中断要求はスキップ。
    let done: i64 = conn
        .query_row(
            "SELECT COALESCE(index_complete,0) FROM folder_sync WHERE account_id=?1 AND folder=?2",
            params![account_id, tag],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if done != 0 || is_cancelled(cancel) {
        return Ok(());
    }
    // フロンティア = 保存済み最小 UID（これより古い側を索引する）。まだ 1 件も無ければ次回に委ねる。
    let frontier: Option<i64> = conn
        .query_row(
            "SELECT MIN(uid) FROM emails WHERE account_id=?1 AND folder=?2 AND uid IS NOT NULL",
            params![account_id, tag],
            |r| r.get::<_, Option<i64>>(0),
        )
        .map_err(|e| e.to_string())?;
    let Some(frontier) = frontier else {
        return Ok(());
    };
    if frontier <= 1 {
        mark_index_complete(conn, account_id, tag)?;
        return Ok(());
    }
    session.select(imap_name).map_err(|e| e.to_string())?;
    // フロンティア未満の UID（サーバに存在する古い側）を検索。空なら完了。
    let mut older: Vec<u32> = session
        .uid_search(format!("UID 1:{}", frontier - 1))
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|&u| (u as i64) < frontier)
        .collect();
    if older.is_empty() {
        mark_index_complete(conn, account_id, tag)?;
        return Ok(());
    }
    older.sort_unstable();
    // フロンティア直下（新しい方）から BACKFILL_PER_SYNC 件だけ取る＝一覧が上から下へ連続して埋まる。
    let take = older.len().min(BACKFILL_PER_SYNC);
    let mut batch: Vec<u32> = older[older.len() - take..].to_vec();
    batch.reverse(); // 新しい UID から取得・保存（表示順を揃える）。
    let total = batch.len() as i32;
    let mut got = 0i32;
    for chunk in batch.chunks(CHUNK) {
        if is_cancelled(cancel) {
            break;
        }
        let set = chunk
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");
        // BODY.PEEK[HEADER]: 既読フラグを立てず、ヘッダのみ取得（本文は取らない＝軽い）。
        let msgs = session
            .uid_fetch(set, "(UID FLAGS BODY.PEEK[HEADER])")
            .map_err(|e| e.to_string())?;
        store_header_fetches(conn, account_id, tag, msgs.iter(), result)?;
        got += chunk.len() as i32;
        progress(tag, got, total);
    }
    Ok(())
}

/// 本文バックフィル: 保証期間（body_window）内で本文が未取得('absent')のメールを、少しずつ
/// 全文取得してローカルに揃える。新しい順・容量上限を尊重・中断/再開可（定期同期ごとに少しずつ）。
/// BODY.PEEK[] で取得するのでサーバー側は既読にしない。docs/SYNC.md §3.6。
#[allow(clippy::too_many_arguments)]
fn backfill_folder_bodies(
    session: &mut ImapSession,
    conn: &Connection,
    account_id: i64,
    imap_name: &str,
    tag: &str,
    body_window: &str,
    result: &mut SyncResult,
    progress: &dyn Fn(&str, i32, i32),
    cancel: &AtomicBool,
) -> Result<(), String> {
    // 'off' は先読みしない（開いたときだけ取得）。中断要求もスキップ。
    if body_window.trim().eq_ignore_ascii_case("off") || is_cancelled(cancel) {
        return Ok(());
    }
    // 容量上限に達していたら本文を増やさない（超過分は要約側に任せる）。
    let limit: i64 = conn
        .query_row(
            "SELECT COALESCE(storage_limit, 2147483648) FROM accounts WHERE id=?1",
            params![account_id],
            |r| r.get(0),
        )
        .unwrap_or(2_147_483_648);
    let used: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(COALESCE(length(body_html_z),0)+COALESCE(length(body_plain),0)),0)
               + (SELECT COALESCE(SUM(a.size),0) FROM attachments a JOIN emails e ON e.id=a.email_id
                  WHERE e.account_id=?1 AND a.file_path IS NOT NULL)
             FROM emails WHERE account_id=?1",
            params![account_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if used >= limit {
        return Ok(());
    }
    // 保証期間の下限（date_ts >= cutoff）。'all'（日数 None）は全期間。
    let cutoff_ts: Option<i64> = body_window_days(body_window).map(|days| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        now - days * 86400
    });
    // 未取得（absent）で uid のあるメールを新しい順に。期間指定があれば date_ts で絞る。
    let uids: Vec<u32> = {
        let mut stmt = conn
            .prepare(
                "SELECT uid FROM emails
                 WHERE account_id=?1 AND COALESCE(folder,'inbox')=?2
                   AND COALESCE(body_state,'present')='absent' AND uid IS NOT NULL
                   AND (?3 IS NULL OR date_ts >= ?3)
                 ORDER BY date_ts DESC, uid DESC LIMIT ?4",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                params![account_id, tag, cutoff_ts, BODY_BACKFILL_PER_SYNC as i64],
                |r| r.get::<_, i64>(0),
            )
            .map_err(|e| e.to_string())?;
        rows.filter_map(|r| r.ok()).map(|u| u as u32).collect()
    };
    if uids.is_empty() {
        return Ok(());
    }
    session.select(imap_name).map_err(|e| e.to_string())?;
    let total = uids.len() as i32;
    let mut got = 0i32;
    let mut c = Counters {
        fetched: 0,
        stored: 0,
        backfilled: 0,
        max_uid: 0,
    };
    for chunk in uids.chunks(CHUNK) {
        if is_cancelled(cancel) {
            break;
        }
        let set = chunk
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");
        // PEEK: 既読フラグを立てずに本文全体を取得する（過去メールを勝手に既読化しない）。
        let msgs = session
            .uid_fetch(set, "(UID FLAGS BODY.PEEK[])")
            .map_err(|e| e.to_string())?;
        store_fetches(conn, account_id, tag, msgs.iter(), &mut c)?;
        got += chunk.len() as i32;
        progress(tag, got, total);
    }
    result.backfilled += c.backfilled;
    Ok(())
}

/// SMTP 送信時にサーバー側が自動で送信控えを Sent に保存するプロバイダか（IMAP ホストで判定）。
/// Gmail は smtp.gmail.com からの送信で必ず「送信済みメール」に控えを残す（無効化不可）ため、
/// クライアントが APPEND すると同じ控えが 2 通になる。該当プロバイダでは APPEND をスキップし、
/// サーバーが保存した 1 通を次回の Sent 同期で取り込む。
pub fn server_saves_sent_copy(imap_host: &str) -> bool {
    let h = imap_host.trim().to_ascii_lowercase();
    // Gmail / Google Workspace はいずれも imap.gmail.com（旧 googlemail 表記も一応許容）。
    h == "imap.gmail.com" || h == "imap.googlemail.com"
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

/// 完全削除する 1 通の指定（サーバー上の元フォルダのローカルタグ＋Message-ID の中身）。
pub struct PurgeItem {
    /// 元のサーバーフォルダのローカルタグ（'inbox' | 'sent' | 'drafts' | 'spam' 等）。
    pub source_tag: String,
    /// Message-ID の中身（山括弧なし）。サーバー上の該当メールを HEADER 検索で引く。
    pub message_id_inner: String,
}

/// 選択済みメールボックスから、複数 Message-ID に一致する UID を集めてカンマ区切りにする。
fn collect_uids_for_message_ids(session: &mut ImapSession, message_ids: &[&str]) -> String {
    let mut uids: Vec<String> = Vec::new();
    for mid in message_ids {
        match session.uid_search(format!("HEADER \"Message-ID\" \"{mid}\"")) {
            Ok(found) => uids.extend(found.iter().map(|u| u.to_string())),
            Err(e) => log::warn!("purge: 検索失敗 mid={mid}: {e}"),
        }
    }
    uids.sort_unstable();
    uids.dedup();
    uids.join(",")
}

/// ローカルで完全削除したメールを、サーバー上でも「Trash へ移動 → 完全削除」する（best-effort）。
/// 元フォルダから Trash へコピー → 元フォルダで \Deleted＋expunge（＝移動）→ 最後に Trash からも
/// expunge して恒久削除する。Trash が無いサーバーでは元フォルダから直接 expunge する。
/// UID は uid_validity 変化に弱いので Message-ID 検索で解決する（下書き削除と同じ作法）。
/// 失敗しても呼び出し側は best-effort（ローカルは既に削除済み）。
pub fn purge_emails_remote(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    items: &[PurgeItem],
) -> Result<(), String> {
    if items.is_empty() {
        return Ok(());
    }
    let mut session = connect_login(host, port, user, password)?;

    // フォルダ名を解決: source タグ→サーバー名でグループ化。Trash 名も引く。
    // （names はセッションを借用しないが、owned に取り出してから以降の操作に進む。）
    let mut by_source: std::collections::HashMap<String, Vec<&str>> = std::collections::HashMap::new();
    let trash_name: Option<String>;
    {
        let names = session.list(Some(""), Some("*")).map_err(|e| e.to_string())?;
        let resolve = |tag: &str| -> Option<String> {
            if tag == "inbox" {
                return Some("INBOX".to_string());
            }
            SYNC_FOLDERS
                .iter()
                .find(|s| s.tag == tag)
                .and_then(|spec| detect_mailbox(names.iter(), spec))
        };
        for it in items {
            match resolve(&it.source_tag) {
                Some(mbox) => by_source
                    .entry(mbox)
                    .or_default()
                    .push(it.message_id_inner.as_str()),
                None => log::warn!(
                    "purge: 元フォルダ '{}' のサーバー名を解決できず（スキップ）",
                    it.source_tag
                ),
            }
        }
        trash_name = SYNC_FOLDERS
            .iter()
            .find(|s| s.tag == "trash")
            .and_then(|spec| detect_mailbox(names.iter(), spec));
    }

    // 元フォルダごとに: Message-ID から UID を集め、Trash へコピー → \Deleted → expunge。
    for (mbox, msgids) in &by_source {
        let same_as_trash = trash_name.as_deref() == Some(mbox.as_str());
        if let Err(e) = session.select(mbox) {
            log::warn!("purge: SELECT '{mbox}' 失敗（スキップ）: {e}");
            continue;
        }
        let set = collect_uids_for_message_ids(&mut session, msgids);
        if set.is_empty() {
            continue;
        }
        // Trash があり、元フォルダが Trash 自身でなければコピー（＝Trash へ移動の第一歩）。
        if let (Some(t), false) = (trash_name.as_deref(), same_as_trash) {
            if let Err(e) = session.uid_copy(&set, t) {
                log::warn!("purge: Trash '{t}' へのコピー失敗（元から直接削除にフォールバック）: {e}");
            }
        }
        if let Err(e) = session.uid_store(&set, "+FLAGS (\\Deleted)") {
            log::warn!("purge: \\Deleted 付与失敗 '{mbox}': {e}");
            continue;
        }
        let _ = session
            .uid_expunge(&set)
            .map(|_| ())
            .or_else(|_| session.expunge().map(|_| ()));
    }

    // Trash から恒久削除（「移動して更に削除」の“更に削除”）。Trash が無ければ元で削除済み。
    if let Some(t) = trash_name.as_deref() {
        if let Err(e) = session.select(t) {
            log::warn!("purge: Trash '{t}' の SELECT 失敗: {e}");
        } else {
            let ids: Vec<&str> = items.iter().map(|i| i.message_id_inner.as_str()).collect();
            let set = collect_uids_for_message_ids(&mut session, &ids);
            if !set.is_empty() {
                if let Err(e) = session.uid_store(&set, "+FLAGS (\\Deleted)") {
                    log::warn!("purge(trash): \\Deleted 付与失敗: {e}");
                } else {
                    let _ = session
                        .uid_expunge(&set)
                        .map(|_| ())
                        .or_else(|_| session.expunge().map(|_| ()));
                }
            }
        }
    }

    let _ = session.logout();
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
    // 保存時と同じ「実添付パート一覧」から part_index 番目を取り出す（ネストした添付にも届く）。
    let parts = parser::real_attachment_parts(&msg);
    let part = parts
        .get(part_index)
        .copied()
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

/// フォルダのローカルタグ（inbox/sent/…）から実際の IMAP メールボックス名を解決する。
/// inbox は "INBOX"。それ以外は LIST して特殊用途/名前で照合（sync と同じ検出）。
fn imap_mailbox_for_tag(session: &mut ImapSession, tag: &str) -> Result<String, String> {
    if tag == "inbox" {
        return Ok("INBOX".to_string());
    }
    let spec = SYNC_FOLDERS
        .iter()
        .find(|s| s.tag == tag)
        .ok_or_else(|| format!("未知のフォルダ: {tag}"))?;
    let names = session
        .list(Some(""), Some("*"))
        .map_err(|e| e.to_string())?;
    detect_mailbox(names.iter(), spec)
        .ok_or_else(|| format!("フォルダ '{tag}' がサーバに見つかりません"))
}

/// 指定 UID のメッセージ全体を再取得して解析する（本文の全文キャッシュ復元用）。
/// 要約保存('evicted')・メタのみ('absent')の本文をサーバーから取り直すときに使う。
/// `folder` はローカルタグ（inbox/sent/…）。正しい IMAP メールボックスを select する。
pub fn fetch_message(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    folder: &str,
    uid: u32,
) -> Result<parser::ParsedEmail, String> {
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let client = imap::connect((host, port), host, &tls).map_err(|e| e.to_string())?;
    let mut session = client
        .login(user, password)
        .map_err(|(e, _)| e.to_string())?;
    let mailbox = imap_mailbox_for_tag(&mut session, folder)?;
    session.select(&mailbox).map_err(|e| e.to_string())?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmail_hosts_auto_save_sent() {
        // Gmail はサーバーが自動保存するので APPEND しない。
        assert!(server_saves_sent_copy("imap.gmail.com"));
        assert!(server_saves_sent_copy("IMAP.GMAIL.COM")); // 大小無視
        assert!(server_saves_sent_copy("imap.googlemail.com"));
    }

    #[test]
    fn other_hosts_need_append() {
        // Gmail 以外はクライアントが APPEND する必要がある。
        assert!(!server_saves_sent_copy("outlook.office365.com"));
        assert!(!server_saves_sent_copy("imap.mail.me.com"));
        assert!(!server_saves_sent_copy("sngdesign.sakura.ne.jp"));
        assert!(!server_saves_sent_copy(""));
    }
}
