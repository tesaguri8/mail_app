//! 送信済みの控えに無い Message-ID を、相手の返信から推定して埋め戻す（docs/THREADING.md §2）。
//!
//! 0.1.0-alpha.34 までの送信は Message-ID を付けておらず、相手に届いた版にだけ中継が ID を付けた。
//! 送信済みへ APPEND した控え（C）には ID が無いので、相手の返信（R）の In-Reply-To（X）が
//! 手元のどのメールとも結び付かず、「手元に無い親」になる。C が新規のメールなら、C は件名の
//! フォールバックのスレッドに、R は X のスレッドに入り、会話が 2 つに割れる。
//!
//! R の X が手元に無いとき、ID の無い控えのうち次をすべて満たすものを探す:
//! - C の日時が R より前（上限は設けない。数か月後の返信もある）
//! - R の差出人が C の To / Cc にいる（自分自身ではない）
//! - 件名を正規化すると一致する
//! - R の References で X の 1 つ前が C の In-Reply-To と一致する（C が新規なら X が先頭か、
//!   References が空）
//! - R の引用の見出しに、C の差出人と日時が読めるなら、その日時が合う（読めなければ問わない）
//!
//! X に合う控えが**ちょうど 1 通**のときだけ、その控えの `emails.message_id` に X を入れ、出所を
//! `inferred_message_ids` に残す。同じ控えに別の X が 2 つ以上来たら結ばない（以後も試さない）。
//! 結んだら、控えと X を参照するメールの会話を割り当て直す。

use std::collections::{BTreeMap, BTreeSet, HashSet};

use rusqlite::{params, Connection, OptionalExtension};

use super::emails::recompute_reps_for_thread;
use super::quote_match::quotes_point_at;
use super::thread_activity::refresh_thread_sent_ts;
use super::threads::{assign_thread, normalize_subject, recompute_thread};
use super::Store;
use crate::services::addr::lowercase_addrs;

/// ID の無い控え（または、推定で ID を入れた控え）1 通。
struct SentCopy {
    id: i64,
    canonical_key: String,
    subject: Option<String>,
    from_address: Option<String>,
    from_name: Option<String>,
    /// To と Cc の素のアドレス（小文字）。
    recipients: HashSet<String>,
    date: Option<String>,
    date_ts: i64,
    in_reply_to: Option<String>,
    /// 既に推定で入れた Message-ID（まだなら None）。
    linked: Option<String>,
}

/// In-Reply-To が手元のどのメールにも無い、相手からのメール 1 通。
struct DanglingReply {
    id: i64,
    /// In-Reply-To（X）。
    in_reply_to: String,
    references_ids: Option<String>,
    subject: Option<String>,
    from_address: String,
    date_ts: i64,
}

/// 推定の範囲。
enum Scope<'a> {
    /// アカウントの、親が手元に無いメールをすべて見る（起動時・控えが新しく入ったとき）。
    All,
    /// 指定したメールだけを R として見る（取り込みのたび）。
    Replies(&'a [i64]),
}

/// 空白・山括弧を除いた ID。空なら None。
fn bare_id(s: Option<&str>) -> Option<&str> {
    s.map(|t| {
        t.trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim()
    })
    .filter(|t| !t.is_empty())
}

/// R の References の並びが、X の親として C の In-Reply-To を持つか。
///
/// X が References にあれば、その 1 つ前が C の In-Reply-To（X が先頭なら C は新規）と一致すること。
/// X が References に無いときは、References が空で C が新規のときだけ合うとする（確かめようが
/// 無いものは結ばない）。
fn chain_fits(r_refs: Option<&str>, x: &str, c_irt: Option<&str>) -> bool {
    let refs: Vec<&str> = r_refs.unwrap_or("").split_whitespace().collect();
    let c_irt = bare_id(c_irt);
    match refs.iter().position(|t| *t == x) {
        Some(0) => c_irt.is_none(),
        Some(pos) => c_irt == Some(refs[pos - 1]),
        None => refs.is_empty() && c_irt.is_none(),
    }
}

/// アカウントの口座アドレス（小文字）。
fn own_address(conn: &Connection, account_id: i64) -> rusqlite::Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT lower(trim(email)) FROM accounts WHERE id = ?1",
            params![account_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

/// 推定の対象になる控え（ID が無く、まだ記録の無いもの）と、推定で ID を入れた控え。
fn load_copies(conn: &Connection, account_id: i64) -> rusqlite::Result<Vec<SentCopy>> {
    let mut stmt = conn.prepare(
        "SELECT e.id, e.canonical_key, e.subject, e.from_address, e.from_name, e.to_addresses,
                e.cc_addresses, e.date, e.date_ts, e.in_reply_to, i.message_id
         FROM emails e
         LEFT JOIN inferred_message_ids i
           ON i.account_id = e.account_id AND i.canonical_key = e.canonical_key
         WHERE e.account_id = ?1 AND e.folder = 'sent' AND e.date_ts IS NOT NULL
           AND ((i.state IS NULL AND COALESCE(trim(e.message_id), '') = '')
                OR (i.state = 'linked' AND e.message_id = i.message_id))",
    )?;
    let rows = stmt.query_map(params![account_id], |r| {
        let to: Option<String> = r.get(5)?;
        let cc: Option<String> = r.get(6)?;
        let recipients = [to, cc]
            .iter()
            .flatten()
            .flat_map(|s| lowercase_addrs(s))
            .collect();
        Ok(SentCopy {
            id: r.get(0)?,
            canonical_key: r.get(1)?,
            subject: r.get(2)?,
            from_address: r.get(3)?,
            from_name: r.get(4)?,
            recipients,
            date: r.get(7)?,
            date_ts: r.get(8)?,
            in_reply_to: r.get(9)?,
            linked: r.get(10)?,
        })
    })?;
    rows.collect()
}

/// 親（In-Reply-To）が手元に無い、相手からのメール。迷惑・送信済み・下書きは見ない。
fn load_dangling(
    conn: &Connection,
    account_id: i64,
    scope: &Scope<'_>,
) -> rusqlite::Result<Vec<DanglingReply>> {
    const BASE: &str = "SELECT r.id, r.in_reply_to, r.references_ids, r.subject,
                lower(trim(r.from_address)), r.date_ts
         FROM emails r
         WHERE r.account_id = ?1 AND COALESCE(trim(r.in_reply_to), '') <> ''
           AND r.from_address IS NOT NULL AND r.date_ts IS NOT NULL
           AND r.folder NOT IN ('sent', 'drafts', 'spam')
           AND NOT EXISTS (SELECT 1 FROM emails p
                           WHERE p.account_id = r.account_id AND p.message_id = r.in_reply_to)";
    let map = |r: &rusqlite::Row| -> rusqlite::Result<DanglingReply> {
        Ok(DanglingReply {
            id: r.get(0)?,
            in_reply_to: r.get(1)?,
            references_ids: r.get(2)?,
            subject: r.get(3)?,
            from_address: r.get(4)?,
            date_ts: r.get(5)?,
        })
    };
    match scope {
        Scope::All => {
            let mut stmt = conn.prepare(BASE)?;
            let rows = stmt.query_map(params![account_id], map)?;
            rows.collect()
        }
        Scope::Replies(ids) => {
            let mut stmt = conn.prepare_cached(&format!("{BASE} AND r.id = ?2"))?;
            let mut out = Vec::new();
            for id in *ids {
                if let Some(row) = stmt.query_row(params![account_id, id], map).optional()? {
                    out.push(row);
                }
            }
            Ok(out)
        }
    }
}

/// R が C への返信だと読めるか（モジュール冒頭の条件。X が手元に無いことは呼び出し側で確かめ済み）。
fn reply_fits(
    conn: &Connection,
    own: Option<&str>,
    r: &DanglingReply,
    c: &SentCopy,
) -> rusqlite::Result<bool> {
    let x = r.in_reply_to.trim();
    let fits = c.date_ts < r.date_ts
        && own != Some(r.from_address.as_str())
        && c.recipients.contains(&r.from_address)
        && normalize_subject(c.subject.as_deref()) == normalize_subject(r.subject.as_deref())
        && chain_fits(r.references_ids.as_deref(), x, c.in_reply_to.as_deref());
    if !fits {
        return Ok(false);
    }
    // 引用の見出しが読めれば、C の日時と差出人を指していること（読めなければ問わない）。
    let quoted = quotes_point_at(
        conn,
        r.id,
        c.from_address.as_deref(),
        c.from_name.as_deref(),
        c.date.as_deref(),
        c.date_ts,
    )?;
    Ok(quoted != Some(false))
}

/// 推定の結果、控え 1 通をどうするか。
enum Decision<'a> {
    /// X を入れる（手がかりにした R の id）。
    Link(&'a str, i64),
    /// 別の X が 2 つ以上来た。まだ結んでいなければ記録だけ、結んでいれば取り消す。
    Conflict,
}

/// 照らし合わせて、控えごとの扱いを決める。キーは控えの添字。
fn decide<'a>(
    conn: &Connection,
    own: Option<&str>,
    copies: &[SentCopy],
    replies: &'a [DanglingReply],
) -> rusqlite::Result<BTreeMap<usize, Decision<'a>>> {
    // X ごとに、合う控えの集合と、手がかりにする R（最も古い id）。
    let mut by_x: BTreeMap<&str, (BTreeSet<usize>, i64)> = BTreeMap::new();
    for r in replies {
        let x = r.in_reply_to.trim();
        for (i, c) in copies.iter().enumerate() {
            if reply_fits(conn, own, r, c)? {
                let e = by_x.entry(x).or_insert_with(|| (BTreeSet::new(), r.id));
                e.0.insert(i);
                e.1 = e.1.min(r.id);
            }
        }
    }
    // 控えごとに、そこへ 1 通だけ合う X を集める（曖昧な X は誰にも結ばない）。
    let mut by_copy: BTreeMap<usize, Vec<(&str, i64)>> = BTreeMap::new();
    for (x, (set, src)) in &by_x {
        if let [only] = set.iter().copied().collect::<Vec<_>>()[..] {
            by_copy.entry(only).or_default().push((x, *src));
        }
    }
    Ok(by_copy
        .into_iter()
        .map(|(i, xs)| {
            // 既に結んだ控えに、別の X が来た（いま手元に無い X は、結んだ X とは必ず別）。
            let decision = match (copies[i].linked.is_some(), xs.as_slice()) {
                (false, [(x, src)]) => Decision::Link(x, *src),
                _ => Decision::Conflict,
            };
            (i, decision)
        })
        .collect())
}

/// 控えと、X を参照するメールの会話を割り当て直す（手動の割当は動かさない）。
///
/// 該当するメールの会話の auto メンバーを外して古い順に割り当て直し、空になった会話を掃除して、
/// 新しい会話の代表と「最後のやり取り」を貼り直す。
fn rethread(
    conn: &Connection,
    account_id: i64,
    copy_ids: &[i64],
    xs: &[String],
) -> rusqlite::Result<()> {
    let mut old_threads: BTreeSet<i64> = BTreeSet::new();
    {
        let mut by_id =
            conn.prepare_cached("SELECT logical_thread_id FROM emails WHERE id = ?1")?;
        for id in copy_ids {
            if let Some(Some(t)) = by_id
                .query_row(params![id], |r| r.get::<_, Option<i64>>(0))
                .optional()?
            {
                old_threads.insert(t);
            }
        }
        let mut by_ref = conn.prepare_cached(
            "SELECT DISTINCT logical_thread_id FROM emails
             WHERE account_id = ?1 AND logical_thread_id IS NOT NULL
               AND (in_reply_to = ?2
                    OR instr(' ' || COALESCE(references_ids, '') || ' ', ' ' || ?2 || ' ') > 0)",
        )?;
        for x in xs {
            let ts = by_ref.query_map(params![account_id, x], |r| r.get::<_, i64>(0))?;
            for t in ts {
                old_threads.insert(t?);
            }
        }
    }
    let mut members: BTreeSet<(i64, i64)> = BTreeSet::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT COALESCE(date_ts, 0), id FROM emails
             WHERE logical_thread_id = ?1 AND COALESCE(thread_assignment, 'auto') <> 'manual'",
        )?;
        for t in &old_threads {
            let rows = stmt.query_map(params![t], |r| Ok((r.get(0)?, r.get(1)?)))?;
            for row in rows {
                members.insert(row?);
            }
        }
        let mut one = conn.prepare_cached(
            "SELECT COALESCE(date_ts, 0), id FROM emails
             WHERE id = ?1 AND COALESCE(thread_assignment, 'auto') <> 'manual'",
        )?;
        for id in copy_ids {
            if let Some(row) = one
                .query_row(params![id], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?
            {
                members.insert(row);
            }
        }
    }
    let mut clear = conn.prepare_cached(
        "UPDATE emails SET logical_thread_id = NULL, thread_id = NULL WHERE id = ?1",
    )?;
    for (_, id) in &members {
        clear.execute(params![id])?;
    }
    let mut new_threads: BTreeSet<i64> = BTreeSet::new();
    for (_, id) in &members {
        if let Some(t) = assign_thread(conn, *id)? {
            new_threads.insert(t);
        }
    }
    for t in old_threads.union(&new_threads) {
        recompute_thread(conn, *t)?;
    }
    for t in &new_threads {
        recompute_reps_for_thread(conn, *t)?;
        refresh_thread_sent_ts(conn, *t)?;
    }
    Ok(())
}

/// 推定して、控えに Message-ID を入れる（会話の割り当て直しまで）。結んだ控えの数を返す。
fn link_in_scope(conn: &Connection, account_id: i64, scope: Scope<'_>) -> rusqlite::Result<usize> {
    let copies = load_copies(conn, account_id)?;
    if copies.is_empty() {
        // ID の無い控えも、推定で ID を入れた控えも無い（alpha.35 以降だけのアカウント）。
        // 結んだ控えは残す: あとから別の X が来たら取り消すため。
        return Ok(0);
    }
    let replies = load_dangling(conn, account_id, &scope)?;
    if replies.is_empty() {
        return Ok(0);
    }
    let own = own_address(conn, account_id)?;
    let decisions = decide(conn, own.as_deref(), &copies, &replies)?;
    if decisions.is_empty() {
        return Ok(0);
    }
    let mut linked = 0;
    let mut copy_ids = Vec::new();
    let mut xs = Vec::new();
    for (i, d) in &decisions {
        let c = &copies[*i];
        match d {
            Decision::Link(x, src) => {
                conn.execute(
                    "UPDATE emails SET message_id = ?2
                     WHERE id = ?1 AND COALESCE(trim(message_id), '') = ''",
                    params![c.id, x],
                )?;
                conn.execute(
                    "INSERT INTO inferred_message_ids
                       (account_id, canonical_key, state, message_id, source_email_id)
                     VALUES (?1, ?2, 'linked', ?3, ?4)",
                    params![account_id, c.canonical_key, x, src],
                )?;
                linked += 1;
                xs.push((*x).to_string());
            }
            Decision::Conflict => {
                if let Some(old) = &c.linked {
                    conn.execute(
                        "UPDATE emails SET message_id = NULL WHERE id = ?1 AND message_id = ?2",
                        params![c.id, old],
                    )?;
                    xs.push(old.clone());
                }
                conn.execute(
                    "INSERT INTO inferred_message_ids (account_id, canonical_key, state)
                     VALUES (?1, ?2, 'conflict')
                     ON CONFLICT(account_id, canonical_key)
                     DO UPDATE SET state = 'conflict', message_id = NULL",
                    params![account_id, c.canonical_key],
                )?;
            }
        }
        copy_ids.push(c.id);
    }
    rethread(conn, account_id, &copy_ids, &xs)?;
    Ok(linked)
}

/// 起動時の照らし合わせを済ませた印（app_settings のキー）。済ませたあとは、取り込みのたびの
/// 推定（[`link_for_pending`]）が新しく入った返信と控えを拾うので、起動のたびには流さない。
const KEY_SCANNED: &str = "inferred_message_ids_scanned";

/// 起動時に 1 回だけ: ID の無い控えのあるアカウントごとに、親が手元に無いメールすべてと
/// 照らし合わせる。結んだ控えの数を返す。対象は送信済みの ID の無い控えだけなので、無ければすぐ終わる。
/// 済ませたら印を付け、次の起動からは何もしない。
///
/// # Errors
/// 問い合わせ・更新に失敗したとき（そのアカウントの変更は取り消され、印も付けないので次回また試す）。
pub(crate) fn link_all_accounts(conn: &Connection) -> rusqlite::Result<usize> {
    let scanned = conn
        .query_row(
            "SELECT 1 FROM app_settings WHERE key = ?1",
            params![KEY_SCANNED],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if scanned {
        return Ok(0);
    }
    let accounts: Vec<i64> = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT account_id FROM emails
             WHERE folder = 'sent' AND COALESCE(trim(message_id), '') = ''",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut total = 0;
    for account_id in accounts {
        let tx = conn.unchecked_transaction()?;
        total += link_in_scope(&tx, account_id, Scope::All)?;
        tx.commit()?;
    }
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, '1')
         ON CONFLICT(key) DO UPDATE SET value = '1'",
        params![KEY_SCANNED],
    )?;
    Ok(total)
}

/// 取り込みのたび: まだ会話の無い（いま入った）メールを見て推定する。結んだ控えの数を返す。
///
/// ID の無い控えが新しく入っていれば（初回の同期・再同期）アカウント全体を、そうでなければ
/// いま入ったメールだけを R として照らし合わせる。呼び出し側のトランザクションの中で呼ぶ。
///
/// # Errors
/// 問い合わせ・更新に失敗したとき。
pub(crate) fn link_for_pending(
    conn: &Connection,
    account_id: i64,
    pending: &[i64],
) -> rusqlite::Result<usize> {
    if pending.is_empty() {
        return Ok(0);
    }
    let mut is_bare_copy = conn.prepare_cached(
        "SELECT 1 FROM emails
         WHERE id = ?1 AND folder = 'sent' AND COALESCE(trim(message_id), '') = ''",
    )?;
    let mut new_copy = false;
    for id in pending {
        if is_bare_copy.exists(params![id])? {
            new_copy = true;
            break;
        }
    }
    let scope = if new_copy {
        Scope::All
    } else {
        Scope::Replies(pending)
    };
    link_in_scope(conn, account_id, scope)
}

impl Store {
    /// 推定で控えに入れた Message-ID を、アカウントの分すべて取り消す（会話も割り当て直す）。
    /// 取り消した控えは記録に `reverted` として残り、以後は推定しない。取り消した数を返す。
    ///
    /// # Errors
    /// 問い合わせ・更新に失敗したとき（変更はすべて取り消される）。
    pub fn revert_inferred_message_ids(&self, account_id: i64) -> rusqlite::Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let rows: Vec<(i64, String)> = {
            let mut stmt = tx.prepare(
                "SELECT e.id, i.message_id FROM inferred_message_ids i
                 JOIN emails e ON e.account_id = i.account_id AND e.canonical_key = i.canonical_key
                 WHERE i.account_id = ?1 AND i.state = 'linked' AND e.message_id = i.message_id",
            )?;
            let out = stmt
                .query_map(params![account_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            out
        };
        for (id, _) in &rows {
            tx.execute(
                "UPDATE emails SET message_id = NULL WHERE id = ?1",
                params![id],
            )?;
        }
        tx.execute(
            "UPDATE inferred_message_ids SET state = 'reverted'
             WHERE account_id = ?1 AND state = 'linked'",
            params![account_id],
        )?;
        let ids: Vec<i64> = rows.iter().map(|(id, _)| *id).collect();
        let xs: Vec<String> = rows.into_iter().map(|(_, x)| x).collect();
        rethread(&tx, account_id, &ids, &xs)?;
        tx.commit()?;
        Ok(ids.len())
    }
}

#[cfg(test)]
mod tests;
