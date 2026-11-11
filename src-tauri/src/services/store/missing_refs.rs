//! 会話の References / In-Reply-To にあって手元に無いメール（欠けた ID）を集め、サーバーへ探しに
//! 行った結果を覚える（docs/THREADING.md §5「References で確かめ、欠けたメールを取りに行く」）。
//!
//! 探しに行くのは画面が会話を開いたときに 1 回だけ（`commands::thread_fetch_missing`）。
//! 見つからなかった ID は `absent` として覚え、30 日後に 1 回だけ探し直し、それでも無ければ
//! 二度と探さない。探した結果は `ref_lookups`（0073）に残す。
//!
//! 自分の送信の控えで Message-ID の無いもの（alpha.34 まで）には、相手の返信から推定した ID が
//! `emails.message_id` に入る（0072・`inferred_message_ids`）。入ったものは「手元にある」ので、
//! ここでは欠けた ID に数えない。控えの数で差し引くのは、推定しても ID が空のままの控えだけ。

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{MissingRef, MissingRefState, ThreadMessage, ThreadView};

/// 探し直すまでの日数（absent になってから）。
pub const RETRY_AFTER_SECS: i64 = 30 * 24 * 60 * 60;

/// 会話の参照（欠けた ID と会話の根）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ThreadRefs {
    /// References / In-Reply-To に出てくる ID のうち、手元（同じアカウント）に無いもの。出てきた順。
    pub missing: Vec<String>,
    /// 会話の根（いちばん古い、References を持つメールの References の先頭）。
    pub root: Option<String>,
    /// 自分の送信の控えで、Message-ID が空のままのもの（推定でも入らなかったもの）の数。
    pub own_without_id: usize,
}

impl ThreadRefs {
    /// 会話の根が手元に無いか。
    pub fn root_is_missing(&self) -> bool {
        self.root
            .as_deref()
            .is_some_and(|r| self.missing.iter().any(|m| m == r))
    }
}

/// ID の並び（空白区切り。山括弧があってもなくても）を、山括弧なしの ID にする。
fn split_ids(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split_whitespace()
        .map(|t| t.trim_matches(|c| c == '<' || c == '>' || c == ','))
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// 会話のメール（日時順）の参照を集める。
///
/// # Errors
/// 問い合わせに失敗したとき。
pub fn collect(conn: &Connection, messages: &[ThreadMessage]) -> rusqlite::Result<ThreadRefs> {
    let Some(account_id) = messages.first().map(|m| i64::from(m.account_id)) else {
        return Ok(ThreadRefs::default());
    };
    let mut refs_stmt =
        conn.prepare_cached("SELECT in_reply_to, references_ids FROM emails WHERE id = ?1")?;
    let mut root: Option<String> = None;
    let mut seen: HashSet<String> = HashSet::new();
    let mut ordered: Vec<String> = Vec::new();
    for m in messages {
        let (irt, refs): (Option<String>, Option<String>) = refs_stmt
            .query_row(params![m.id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
            .unwrap_or((None, None));
        let refs: Vec<String> = refs
            .as_deref()
            .map(split_ids)
            .into_iter()
            .flatten()
            .collect();
        if root.is_none() {
            root = refs.first().cloned();
        }
        let irt = irt.as_deref().map(split_ids).into_iter().flatten();
        for id in refs.into_iter().chain(irt) {
            if seen.insert(id.clone()) {
                ordered.push(id);
            }
        }
    }
    let here: HashSet<&str> = messages
        .iter()
        .filter_map(|m| m.message_id.as_deref())
        .collect();
    let mut known_stmt = conn
        .prepare_cached("SELECT 1 FROM emails WHERE account_id = ?1 AND message_id = ?2 LIMIT 1")?;
    let mut missing = Vec::new();
    for id in ordered {
        if here.contains(id.as_str()) {
            continue;
        }
        if known_stmt
            .query_row(params![account_id, id], |_| Ok(()))
            .optional()?
            .is_none()
        {
            missing.push(id);
        }
    }
    let own_without_id = own_without_id(messages);
    Ok(ThreadRefs {
        missing,
        root,
        own_without_id,
    })
}

/// 自分の送信の控えで、Message-ID が空のままのもの（推定でも入らなかったもの）の数。
fn own_without_id(messages: &[ThreadMessage]) -> usize {
    messages
        .iter()
        .filter(|m| m.direction == "out")
        .filter(|m| {
            m.message_id
                .as_deref()
                .map_or(true, |s| s.trim().is_empty())
        })
        .count()
}

/// 探した結果の状態（`ref_lookups` の 1 行）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Lookup {
    found: bool,
    checked_at: i64,
    retried: bool,
}

fn lookup(conn: &Connection, account_id: i64, id: &str) -> rusqlite::Result<Option<Lookup>> {
    conn.prepare_cached(
        "SELECT state, checked_at, retried FROM ref_lookups WHERE account_id = ?1 AND message_id = ?2",
    )?
    .query_row(params![account_id, id], |r| {
        Ok(Lookup {
            found: r.get::<_, String>(0)? == "found",
            checked_at: r.get(1)?,
            retried: r.get::<_, i64>(2)? != 0,
        })
    })
    .optional()
}

/// 欠けた ID それぞれを、いま探しに行くか（`now` は UNIX 秒）。
///
/// # Errors
/// 問い合わせに失敗したとき。
pub fn states(
    conn: &Connection,
    account_id: i64,
    missing: &[String],
    now: i64,
) -> rusqlite::Result<Vec<MissingRef>> {
    missing
        .iter()
        .map(|id| {
            let state = match lookup(conn, account_id, id)? {
                None => MissingRefState::Due,
                // 見つけて取り込んだのに手元に無い（消した等）: 取り直さない。
                Some(l) if l.found => MissingRefState::Absent,
                Some(l) if !l.retried && now - l.checked_at >= RETRY_AFTER_SECS => {
                    MissingRefState::Due
                }
                Some(_) => MissingRefState::Absent,
            };
            Ok(MissingRef {
                message_id: id.clone(),
                state,
            })
        })
        .collect()
}

/// 見つけて取り込んだことを覚える。
///
/// # Errors
/// 書き込みに失敗したとき。
pub fn record_found(
    conn: &Connection,
    account_id: i64,
    id: &str,
    folder: &str,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO ref_lookups (account_id, message_id, state, folder, checked_at, retried)
         VALUES (?1, ?2, 'found', ?3, ?4, 0)
         ON CONFLICT (account_id, message_id) DO UPDATE
           SET state = 'found', folder = excluded.folder, checked_at = excluded.checked_at",
        params![account_id, id, folder, now],
    )?;
    Ok(())
}

/// 探したが見つからなかったことを覚える。既に absent だった（30 日後の探し直し）なら、
/// 探し直しを済ませた印を付ける（以後は探さない）。
///
/// # Errors
/// 書き込みに失敗したとき。
pub fn record_absent(
    conn: &Connection,
    account_id: i64,
    id: &str,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO ref_lookups (account_id, message_id, state, folder, checked_at, retried)
         VALUES (?1, ?2, 'absent', NULL, ?3, 0)
         ON CONFLICT (account_id, message_id) DO UPDATE
           SET retried = CASE WHEN ref_lookups.state = 'absent' THEN 1 ELSE ref_lookups.retried END,
               state = 'absent', folder = NULL, checked_at = excluded.checked_at",
        params![account_id, id, now],
    )?;
    Ok(())
}

/// 根の ID が、探しに行って見つけ取り込んだものか。
///
/// # Errors
/// 問い合わせに失敗したとき。
pub fn was_fetched(conn: &Connection, account_id: i64, id: &str) -> rusqlite::Result<bool> {
    Ok(lookup(conn, account_id, id)?.is_some_and(|l| l.found))
}

/// 会話の欠けた ID のうち、いま探しに行くもの。数が未受信の枠と合わなければ INFO を 1 行残す
/// （判定は締めない。自分の控えで ID が空のままのものは、欠けた ID の側から差し引いて比べる）。
pub fn due_ids(view: &ThreadView) -> Vec<String> {
    let due: Vec<String> = view
        .missing_refs
        .iter()
        .filter(|m| m.state == MissingRefState::Due)
        .map(|m| m.message_id.clone())
        .collect();
    if due.is_empty() {
        return due;
    }
    let frames = view
        .messages
        .iter()
        .flat_map(|m| &m.unseen_quotes)
        .filter(|q| q.date.is_some() && !q.guessed)
        .count();
    let own_without_id = own_without_id(&view.messages);
    let expected = view.missing_refs.len().saturating_sub(own_without_id);
    if expected != frames {
        log::info!(
            "会話 {}: 欠けた ID {} 通（ID の無い自分の控え {} 通を除いて {} 通）と、日時のある未受信の枠 {} 通が合わない",
            view.thread.id,
            view.missing_refs.len(),
            own_without_id,
            expected,
            frames
        );
    }
    due
}

#[cfg(test)]
mod tests;
