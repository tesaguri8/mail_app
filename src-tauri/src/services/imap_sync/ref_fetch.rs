//! 会話の References にあって手元に無いメールを、サーバーで Message-ID から探して取り込む
//! （docs/THREADING.md §5「References で確かめ、欠けたメールを取りに行く」）。
//!
//! - 探すのは会話を開いたときに 1 回だけ（呼び出し側が `store::missing_refs` で決める）。
//!   同じ会話の探しが走っている間は、2 回目を起こさない（[`InFlight`]）。
//! - 接続は画面からの取得用の使い回し（`ImapPool::fetch_slot`）を使い、alpha.63 の門
//!   （ホストごとの後退と直列化）を通す。開いたついでの自動の取得なので `Urgency::Auto`
//!   （後退中は張らずに断り、探さなかった ID は次に開いたときに探す）。
//! - 探す順: `\All`（Gmail の「すべてのメール」）があれば最初に 1 回 → 送信済み → 受信箱 → その他。
//!   迷惑メール・ゴミ箱・下書き・選べないフォルダ・Gmail の仮想フォルダ（\Flagged・\Important）は探さない。
//!   全部見つかった時点で止める。
//! - 1 フォルダにつき `UID SEARCH OR HEADER Message-ID … …` を 1 回（10 件ずつ）。見つかったものは
//!   同期と同じ軽量取得で、そのフォルダのタグに取り込む（`folder_sync` の last_uid は動かさない）。
//!   受信箱・送信済みはそのタグ、それ以外（`\All` を含む）は `archive`（サーバー側の操作はしない）。
//! - 同じアカウント・フォルダ・UID の行が既にあれば取り込まない（ID の無い自分の控えと二重にしない）。
//! - 全フォルダを探しても無ければ `absent` として覚える。途中で失敗したら何も覚えない（次に探し直す）。

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, OnceLock};

use rusqlite::{params, Connection, OptionalExtension};

use super::{
    account_self_secret, detect_listed, ensure_live_session, fetch_light_chunk, ChunkOpts,
    Counters, ImapError, ImapSession, Listed, Urgency, SYNC_FOLDERS,
};
use crate::services::store::missing_refs;

/// 1 回の SEARCH に入れる ID の数。
const SEARCH_BATCH: usize = 10;

/// 受信箱・送信済み以外から取り込んだメールのローカルタグ。
pub const ARCHIVE_TAG: &str = "archive";

/// 探して取り込む処理の失敗。
#[derive(Debug, thiserror::Error)]
pub enum RefFetchError {
    /// 接続・ログイン（後退中で張らずに断った場合を含む）。
    #[error(transparent)]
    Connect(#[from] ImapError),
    /// 接続した後の IMAP の命令。
    #[error("IMAP の命令に失敗しました: {0}")]
    Imap(String),
    /// 手元の DB。
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

/// 探した結果。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RefFetchOutcome {
    /// 見つけて取り込んだ ID と、そのフォルダのタグ。
    pub found: Vec<(String, &'static str)>,
    /// 全フォルダを探しても無かった ID。
    pub absent: Vec<String>,
}

/// 同じ会話の探しが走っているか（会話の id）。
pub struct InFlight;

/// 走っている間だけ持つ札。落とすと会話の印を外す。
pub struct InFlightGuard(i64);

impl InFlight {
    fn set() -> &'static Mutex<HashSet<i64>> {
        static SET: OnceLock<Mutex<HashSet<i64>>> = OnceLock::new();
        SET.get_or_init(|| Mutex::new(HashSet::new()))
    }

    /// 会話の探しを始める。既に走っていれば None（2 回目を起こさない）。
    pub fn try_begin(thread_id: i64) -> Option<InFlightGuard> {
        let mut set = Self::set()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // then_some にしない: 札を先に作ると、入れられなかったときに札が落ちて（ロックを持ったまま）
        // 走っている会話の印を外してしまう。
        set.insert(thread_id).then(|| InFlightGuard(thread_id))
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        InFlight::set()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.0);
    }
}

/// フォルダ一覧から、探すフォルダとそのローカルタグを探す順に並べる。
fn search_order(list: &[Listed]) -> Vec<(String, &'static str)> {
    let spec = |tag: &str| SYNC_FOLDERS.iter().find(|s| s.tag == tag);
    let at = |tag: &str| spec(tag).and_then(|s| detect_listed(list, s));
    let sent = at("sent");
    // 探さないフォルダ（同期と同じ判定で決まるもの＋その属性を持つもの）。
    let skipped: HashSet<usize> = ["drafts", "trash", "spam"]
        .iter()
        .filter_map(|t| at(t))
        .collect();
    let skip_attr = |n: &Listed| {
        [
            "\\Noselect",
            "\\NonExistent",
            "\\Drafts",
            "\\Trash",
            "\\Junk",
            "\\Flagged",
            "\\Important",
        ]
        .iter()
        .any(|a| n.has_attr(a))
    };
    let is_inbox = |n: &Listed| n.name.eq_ignore_ascii_case("INBOX");
    let mut all = Vec::new();
    let mut rest = Vec::new();
    for (i, n) in list.iter().enumerate() {
        if skipped.contains(&i) || skip_attr(n) || Some(i) == sent || is_inbox(n) {
            continue;
        }
        if n.has_attr("\\All") {
            all.push((n.name.clone(), ARCHIVE_TAG));
        } else {
            rest.push((n.name.clone(), ARCHIVE_TAG));
        }
    }
    let mut order = all;
    if let Some(i) = sent {
        order.push((list[i].name.clone(), "sent"));
    }
    if let Some(n) = list.iter().find(|n| is_inbox(n)) {
        order.push((n.name.clone(), "inbox"));
    }
    order.extend(rest);
    order
}

/// SEARCH に入れてよい ID か（引用符・改行・非 ASCII を含むものは命令を壊すので探さない）。
fn searchable(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_graphic() && c != '"' && c != '\\')
}

/// `ids` のどれかを Message-ID に持つメールを探す SEARCH の条件（`OR a OR b c`）。
fn search_query(ids: &[&str]) -> Option<String> {
    let (last, init) = ids.split_last()?;
    let key = |id: &str| format!("HEADER Message-ID \"<{id}>\"");
    Some(
        init.iter()
            .rev()
            .fold(key(last), |acc, id| format!("OR {} {acc}", key(id))),
    )
}

/// 欠けた ID をサーバーで探し、見つかったものを取り込む。取り込んだら会話を組み直す。
///
/// 全フォルダを探し終えたときだけ、見つからなかった ID を absent として覚える。
///
/// # Errors
/// 接続（後退中で断った場合を含む）・IMAP の命令・DB に失敗したとき。途中で失敗した接続は捨てる。
#[allow(clippy::too_many_arguments)] // 接続先と資格情報は同期の入口と同じ形で受ける
pub fn fetch_missing(
    db_path: &Path,
    account_id: i64,
    ids: &[String],
    slot: &Mutex<Option<ImapSession>>,
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    now: i64,
) -> Result<RefFetchOutcome, RefFetchError> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
    let mut remaining: Vec<String> = ids.iter().filter(|id| searchable(id)).cloned().collect();
    if remaining.is_empty() {
        return Ok(RefFetchOutcome::default());
    }
    let mut guard = slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    ensure_live_session(&mut guard, host, port, user, password, Urgency::Auto)?;
    let Some(session) = guard.as_mut() else {
        return Err(RefFetchError::Imap("接続を用意できませんでした".into()));
    };
    let mut found = Vec::new();
    let searched = search_folders(session, &conn, account_id, &mut remaining, &mut found, now);
    if let Err(e) = searched {
        // 読み残しで次の命令を読み違えないよう、途中で失敗した接続は捨てる。
        *guard = None;
        return Err(e);
    }
    drop(guard);
    for id in &remaining {
        missing_refs::record_absent(&conn, account_id, id, now)?;
    }
    if !found.is_empty() {
        // 取り込んだメールを会話へ束ねる（同期の後の加工と同じ。別の接続で小分けに走る）。
        crate::services::store::process_pending_at(db_path, account_id)?;
    }
    log::info!(
        "欠けたメールを探した（アカウント {account_id}）: {} 通を取り込み・{} 通は見つからない",
        found.len(),
        remaining.len()
    );
    Ok(RefFetchOutcome {
        found,
        absent: remaining,
    })
}

/// 探す順にフォルダを回り、見つかったものを取り込む（`remaining` から外し、`found` に足す）。
fn search_folders(
    session: &mut ImapSession,
    conn: &Connection,
    account_id: i64,
    remaining: &mut Vec<String>,
    found: &mut Vec<(String, &'static str)>,
    now: i64,
) -> Result<(), RefFetchError> {
    let list: Vec<Listed> = session
        .list(Some(""), Some("*"))
        .map_err(|e| RefFetchError::Imap(e.to_string()))?
        .iter()
        .map(Listed::from_name)
        .collect();
    let self_secret = account_self_secret(conn, account_id);
    for (mailbox, tag) in search_order(&list) {
        if remaining.is_empty() {
            break;
        }
        session
            .examine(&mailbox)
            .map_err(|e| RefFetchError::Imap(e.to_string()))?;
        let mut uids: Vec<u32> = Vec::new();
        for batch in remaining.chunks(SEARCH_BATCH) {
            let refs: Vec<&str> = batch.iter().map(String::as_str).collect();
            let Some(query) = search_query(&refs) else {
                continue;
            };
            let hits = session
                .uid_search(query)
                .map_err(|e| RefFetchError::Imap(e.to_string()))?;
            uids.extend(hits);
        }
        uids.sort_unstable();
        uids.dedup();
        // 同じフォルダ・UID の行が既にあれば取り込まない（ID の無い自分の控え等と二重にしない）。
        let mut exists = conn.prepare_cached(
            "SELECT 1 FROM emails WHERE account_id = ?1 AND folder = ?2 AND uid = ?3 LIMIT 1",
        )?;
        let mut fresh = Vec::new();
        for uid in uids {
            if exists
                .query_row(params![account_id, tag, uid], |_| Ok(()))
                .optional()?
                .is_none()
            {
                fresh.push(uid.to_string());
            }
        }
        if !fresh.is_empty() {
            let mut c = Counters {
                fetched: 0,
                stored: 0,
                backfilled: 0,
                max_uid: 0,
            };
            fetch_light_chunk(
                session,
                conn,
                account_id,
                tag,
                &self_secret,
                &fresh.join(","),
                &ChunkOpts {
                    by_uid: true,
                    reverse: false,
                    headers_first: true,
                    listed: &|_| {},
                },
                &mut c,
                &AtomicBool::new(false),
            )
            .map_err(RefFetchError::Imap)?;
        }
        // 取り込めた（手元に ID が入った）ものを見つかったとする。
        let mut known = conn.prepare_cached(
            "SELECT 1 FROM emails WHERE account_id = ?1 AND message_id = ?2 LIMIT 1",
        )?;
        let mut still = Vec::new();
        for id in remaining.drain(..) {
            if known
                .query_row(params![account_id, id], |_| Ok(()))
                .optional()?
                .is_some()
            {
                missing_refs::record_found(conn, account_id, &id, tag, now)?;
                found.push((id, tag));
            } else {
                still.push(id);
            }
        }
        *remaining = still;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
