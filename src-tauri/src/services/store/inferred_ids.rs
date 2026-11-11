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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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

/// 控え 1 通に対してすること（[`plan`] が決め、[`apply`] が書く）。
struct Action {
    copy_id: i64,
    canonical_key: String,
    kind: ActionKind,
}

enum ActionKind {
    /// X を入れる（手がかりにした R の id）。
    Link { x: String, src: i64 },
    /// 結ばない（既に結んでいた X があれば取り消す）。
    Conflict { old: Option<String> },
}

impl Action {
    /// 会話を割り当て直すときに、参照しているメールを探す X（入れる X か、取り消す X）。
    fn x(&self) -> Option<&str> {
        match &self.kind {
            ActionKind::Link { x, .. } => Some(x),
            ActionKind::Conflict { old } => old.as_deref(),
        }
    }
}

/// 照らし合わせて、控えごとにすることを決める（読むだけ。書き込みはしない）。
fn plan(conn: &Connection, account_id: i64, scope: Scope<'_>) -> rusqlite::Result<Vec<Action>> {
    let copies = load_copies(conn, account_id)?;
    if copies.is_empty() {
        // ID の無い控えも、推定で ID を入れた控えも無い（alpha.35 以降だけのアカウント）。
        // 結んだ控えは残す: あとから別の X が来たら取り消すため。
        return Ok(Vec::new());
    }
    let replies = load_dangling(conn, account_id, &scope)?;
    if replies.is_empty() {
        return Ok(Vec::new());
    }
    let own = own_address(conn, account_id)?;
    let decisions = decide(conn, own.as_deref(), &copies, &replies)?;
    Ok(decisions
        .into_iter()
        .map(|(i, d)| {
            let c = &copies[i];
            let kind = match d {
                Decision::Link(x, src) => ActionKind::Link {
                    x: x.to_string(),
                    src,
                },
                Decision::Conflict => ActionKind::Conflict {
                    old: c.linked.clone(),
                },
            };
            Action {
                copy_id: c.id,
                canonical_key: c.canonical_key.clone(),
                kind,
            }
        })
        .collect())
}

/// X ごとに、それを In-Reply-To か References に持つメール（会話の割当済み）の id を集める。
///
/// X ごとに表を引くと、References の中を探すので毎回全表走査になる（vaio の 5.4 万行で 1 回
/// 0.22 秒）。1 回だけ走査して、Rust 側で突き合わせる。
fn referencing(
    conn: &Connection,
    account_id: i64,
    xs: &HashSet<&str>,
) -> rusqlite::Result<HashMap<String, Vec<i64>>> {
    let mut out: HashMap<String, Vec<i64>> = HashMap::new();
    if xs.is_empty() {
        return Ok(out);
    }
    let mut stmt = conn.prepare(
        "SELECT id, in_reply_to, references_ids FROM emails
         WHERE account_id = ?1 AND logical_thread_id IS NOT NULL
           AND (in_reply_to IS NOT NULL OR references_ids IS NOT NULL)",
    )?;
    let rows = stmt.query_map(params![account_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    for row in rows {
        let (id, irt, refs) = row?;
        let hits: BTreeSet<&str> = irt
            .as_deref()
            .map(str::trim)
            .into_iter()
            .chain(refs.as_deref().unwrap_or("").split_whitespace())
            .filter(|t| xs.contains(t))
            .collect();
        for x in hits {
            out.entry(x.to_string()).or_default().push(id);
        }
    }
    Ok(out)
}

/// 控えと、X を参照するメールの会話を割り当て直す（手動の割当は動かさない）。
///
/// 該当するメールの会話の auto メンバーを外して古い順に割り当て直し、空になった会話を掃除して、
/// 新しい会話の代表と「最後のやり取り」を貼り直す。会話は主キーで引く。
fn rethread(conn: &Connection, copy_ids: &[i64], ref_ids: &[i64]) -> rusqlite::Result<()> {
    let mut old_threads: BTreeSet<i64> = BTreeSet::new();
    {
        let mut by_id =
            conn.prepare_cached("SELECT logical_thread_id FROM emails WHERE id = ?1")?;
        for id in copy_ids.iter().chain(ref_ids) {
            if let Some(Some(t)) = by_id
                .query_row(params![id], |r| r.get::<_, Option<i64>>(0))
                .optional()?
            {
                old_threads.insert(t);
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

/// 決めたことを書き、関係する会話を割り当て直す。結んだ控えの数を返す。
///
/// 決めてから書くまでの間に、別の経路（取り込み・起動時の背景処理）が同じ控えを書き換えて
/// いれば、その控えは飛ばす（Message-ID が空のときだけ入れ、記録が既にあれば足さない）。
fn apply(
    conn: &Connection,
    account_id: i64,
    actions: &[Action],
    refs: &HashMap<String, Vec<i64>>,
) -> rusqlite::Result<usize> {
    let mut linked = 0;
    let mut copy_ids = Vec::new();
    let mut ref_ids = Vec::new();
    for a in actions {
        match &a.kind {
            ActionKind::Link { x, src } => {
                let n = conn.execute(
                    "UPDATE emails SET message_id = ?2
                     WHERE id = ?1 AND COALESCE(trim(message_id), '') = ''",
                    params![a.copy_id, x],
                )?;
                if n == 0 {
                    continue;
                }
                let n = conn.execute(
                    "INSERT INTO inferred_message_ids
                       (account_id, canonical_key, state, message_id, source_email_id)
                     VALUES (?1, ?2, 'linked', ?3, ?4)
                     ON CONFLICT(account_id, canonical_key) DO NOTHING",
                    params![account_id, a.canonical_key, x, src],
                )?;
                if n == 0 {
                    // 記録が既にある（conflict / reverted）控えには入れない。
                    conn.execute(
                        "UPDATE emails SET message_id = NULL WHERE id = ?1 AND message_id = ?2",
                        params![a.copy_id, x],
                    )?;
                    continue;
                }
                linked += 1;
            }
            ActionKind::Conflict { old } => {
                if let Some(old) = old {
                    conn.execute(
                        "UPDATE emails SET message_id = NULL WHERE id = ?1 AND message_id = ?2",
                        params![a.copy_id, old],
                    )?;
                }
                conn.execute(
                    "INSERT INTO inferred_message_ids (account_id, canonical_key, state)
                     VALUES (?1, ?2, 'conflict')
                     ON CONFLICT(account_id, canonical_key)
                     DO UPDATE SET state = 'conflict', message_id = NULL",
                    params![account_id, a.canonical_key],
                )?;
            }
        }
        copy_ids.push(a.copy_id);
        if let Some(ids) = a.x().and_then(|x| refs.get(x)) {
            ref_ids.extend_from_slice(ids);
        }
    }
    rethread(conn, &copy_ids, &ref_ids)?;
    Ok(linked)
}

/// 決めて、書くまでを 1 つの接続（呼び出し側のトランザクション）で行う。結んだ控えの数を返す。
fn link_in_scope(conn: &Connection, account_id: i64, scope: Scope<'_>) -> rusqlite::Result<usize> {
    let actions = plan(conn, account_id, scope)?;
    if actions.is_empty() {
        return Ok(0);
    }
    let xs: HashSet<&str> = actions.iter().filter_map(Action::x).collect();
    let refs = referencing(conn, account_id, &xs)?;
    apply(conn, account_id, &actions, &refs)
}

/// 起動時の照らし合わせを済ませた印（app_settings のキー）。済ませたあとは、取り込みのたびの
/// 推定（[`link_for_pending`]）が新しく入った返信と控えを拾うので、起動のたびには流さない。
const KEY_SCANNED: &str = "inferred_message_ids_scanned";

/// 1 つのトランザクションで書く控えの数。UI の書き込みが間に割り込めるよう小さくする。
const APPLY_CHUNK: usize = 10;

/// 起動時に 1 回だけ（の本体）: ID の無い控えのあるアカウントごとに、親が手元に無いメールすべてと
/// 照らし合わせる。結んだ控えの数を返す。対象は送信済みの ID の無い控えだけなので、無ければすぐ終わる。
///
/// 決めるのは読むだけで済ませ、書くのは [`APPLY_CHUNK`] 通ずつの短いトランザクションに分けて、
/// 間で `pause` だけ書き込みのロックを手放す。済ませたら印を付け、次からは何もしない。
///
/// # Errors
/// 問い合わせ・更新に失敗したとき（書き終えたチャンクは残る。印は付けないので次回また試す）。
fn link_all_conn(conn: &Connection, pause: std::time::Duration) -> rusqlite::Result<usize> {
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
        let actions = plan(conn, account_id, Scope::All)?;
        if actions.is_empty() {
            continue;
        }
        let xs: HashSet<&str> = actions.iter().filter_map(Action::x).collect();
        let refs = referencing(conn, account_id, &xs)?;
        for chunk in actions.chunks(APPLY_CHUNK) {
            let tx = conn.unchecked_transaction()?;
            total += apply(&tx, account_id, chunk, &refs)?;
            tx.commit()?;
            std::thread::sleep(pause);
        }
    }
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, '1')
         ON CONFLICT(key) DO UPDATE SET value = '1'",
        params![KEY_SCANNED],
    )?;
    Ok(total)
}

/// 起動時に 1 回だけ: 送信済みの控えに無い Message-ID を推定して入れる（[`link_all_conn`]）。
/// UI 用の接続を塞がないよう、別の接続で背景のブロッキングスレッドから呼ぶ。結んだ控えの数を返す。
///
/// # Errors
/// DB を開けないとき、または問い合わせ・更新に失敗したとき。
pub fn link_sent_copies_at(db_path: &std::path::Path) -> rusqlite::Result<usize> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
    link_all_conn(&conn, std::time::Duration::from_millis(15))
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
        let xs: HashSet<&str> = rows.iter().map(|(_, x)| x.as_str()).collect();
        let refs = referencing(&tx, account_id, &xs)?;
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
        let ref_ids: Vec<i64> = rows
            .iter()
            .filter_map(|(_, x)| refs.get(x))
            .flatten()
            .copied()
            .collect();
        rethread(&tx, &ids, &ref_ids)?;
        tx.commit()?;
        Ok(ids.len())
    }
}

#[cfg(test)]
mod tests;
