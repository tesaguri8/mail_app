//! スレッド一覧の絞り込み（メール画面のトグル・期間・タグ）を、SQL の条件にする。
//! docs/MAIL_LIST_TIMELINE.md §3。
//!
//! 一覧（`list_threads_filtered`）・日ごとの件数（`thread_day_counts_filtered`）・日ごとの未読
//! （`thread_day_unread_filtered`）に**同じ条件**を入れるので、絞り込み中も日ごとの件数の累計が
//! そのまま一覧の OFFSET になり、日付のつまみで遠い日へ跳べる。
//!
//! 条件は一覧の行（`ThreadListItem`）の値の出し方に合わせる（画面側の `matchesFilters` /
//! `matchesNoneOfFilters` / `matchesDate` / `matchesTags` と同じ結果になること。試験で確かめる）。

use rusqlite::types::Value;
use rusqlite::Connection;

use super::contact_lookup::contact_exists_sql;
use super::emails::folder_predicate;
use super::greendomain::green_domain_set;
use super::thread_activity::activity_ts;
use crate::models::{ThreadFilter, ThreadToggle};

/// 絞り込みの SQL 断片と、その `?N` に渡す値。
pub(super) struct FilterSql {
    /// `" AND (...)"`（絞り込みが無ければ空）。代表の行の WHERE の後ろへそのまま足す。
    pub clause: String,
    /// `clause` の `?N`（`first` から順）に渡す値。
    pub params: Vec<Value>,
}

impl FilterSql {
    /// 絞り込みを SQL にする。
    ///
    /// - `p`: 代表の行の列プレフィックス。副問い合わせから外の行を指すので**必ず修飾する**（`"emails."` / `"r."`）
    /// - `folder`: フォルダ（副問い合わせのフォルダは、呼び出し側と同じ `?1` で引く）
    /// - `first`: この断片で使う最初の `?N` の番号（呼び出し側の固定の引数の次）
    ///
    /// # Errors
    /// グリーンのドメイン集合を引けなかったとき。
    pub(super) fn build(
        conn: &Connection,
        filter: &ThreadFilter,
        folder: &str,
        p: &str,
        first: usize,
    ) -> rusqlite::Result<Self> {
        let mut params: Vec<Value> = Vec::new();
        // 値を積んで、その `?N` を返す。
        let mut bind = |v: Value| -> String {
            params.push(v);
            format!("?{}", first + params.len() - 1)
        };
        let fp_t = folder_predicate(folder, "t.", "?1");
        let green_ph = if filter.toggles.contains(&ThreadToggle::Green) {
            Some(bind(Value::Text(green_json(conn)?)))
        } else {
            None
        };
        let mut parts: Vec<String> = filter
            .toggles
            .iter()
            .map(|t| {
                let pred = toggle_sql(*t, p, &fp_t, green_ph.as_deref());
                // 列が NULL の行でも真偽を決める（画面側は NULL を偽として扱う）。
                if filter.invert {
                    format!("NOT COALESCE(({pred}), 0)")
                } else {
                    format!("COALESCE(({pred}), 0)")
                }
            })
            .collect();
        if let Some(d) = filter.date {
            let act = activity_ts(p);
            parts.push(format!("{act} IS NOT NULL"));
            if let Some(since) = d.since {
                parts.push(format!("{act} >= {}", bind(Value::Integer(since))));
            }
            if let Some(until) = d.until {
                parts.push(format!("{act} < {}", bind(Value::Integer(until))));
            }
        }
        if !filter.tag_ids.is_empty() {
            let ph = bind(Value::Text(int_json(&filter.tag_ids)));
            parts.push(format!(
                "EXISTS (SELECT 1 FROM email_tags et WHERE et.email_id = {p}id
                          AND et.tag_id IN (SELECT value FROM json_each({ph})))"
            ));
        }
        let clause = if parts.is_empty() {
            String::new()
        } else {
            format!(" AND ({})", parts.join(" AND "))
        };
        Ok(Self { clause, params })
    }

    /// 呼び出し側の固定の引数（`head`。`?1` から）・アカウント（あれば次の番号）・この断片の値を、
    /// `?N` の順に並べる。アカウントの指定が無くても、断片の値があれば番号を詰めないよう NULL を置く
    /// （SQL 側はアカウントの条件を書かないので、その値は読まれない）。
    pub(super) fn binds(&self, head: Vec<Value>, account_id: Option<i64>) -> Vec<Value> {
        let mut v = head;
        if account_id.is_some() || !self.params.is_empty() {
            v.push(account_id.map_or(Value::Null, Value::Integer));
        }
        v.extend(self.params.iter().cloned());
        v
    }
}

/// トグル 1 つの条件（反転する前）。値の出し方は `list_threads` の各列と同じ。
fn toggle_sql(t: ThreadToggle, p: &str, fp_t: &str, green_ph: Option<&str>) -> String {
    let from = format!("{p}from_address");
    match t {
        // 一覧の未読（unread_cnt > 0）: そのフォルダのスレッドのメールに未読が 1 通でもある。
        // 相関させずに未読の行から逆に引く（未読だけの部分索引 idx_emails_unread。一度だけ評価される）。
        ThreadToggle::Unread => format!(
            "CASE WHEN {p}logical_thread_id IS NULL THEN {p}is_read = 0
                  ELSE {p}logical_thread_id IN (
                      SELECT t.logical_thread_id FROM emails t
                      WHERE {fp_t} AND t.is_read = 0 AND t.logical_thread_id IS NOT NULL)
             END"
        ),
        // 一覧のスター（is_flagged_agg）: そのフォルダのスレッドのどれか 1 通に付いている。
        ThreadToggle::Star => format!(
            "CASE WHEN {p}logical_thread_id IS NULL THEN {p}is_flagged <> 0
                  ELSE EXISTS (SELECT 1 FROM emails t INDEXED BY idx_emails_thread_folder
                               WHERE t.logical_thread_id = {p}logical_thread_id AND {fp_t}
                                 AND t.is_flagged <> 0)
             END"
        ),
        // 一覧の has_real: 代表メールの添付（本文に埋め込んだ画像は数えない）。
        ThreadToggle::Attachment => format!(
            "{p}has_attachments = 1
             OR EXISTS (SELECT 1 FROM attachments a WHERE a.email_id = {p}id
                          AND COALESCE(a.kind, 'attachment') <> 'inline')"
        ),
        ThreadToggle::Known => contact_exists_sql(&from, false),
        ThreadToggle::Vip => contact_exists_sql(&from, true),
        ThreadToggle::Replied => {
            format!("EXISTS (SELECT 1 FROM sent_addresses sa WHERE sa.address = lower({from}))")
        }
        // 一覧の is_green: 住所録の本人か、差出人のドメインがグリーン集合（`green_domain_set`）にある。
        ThreadToggle::Green => {
            let known = contact_exists_sql(&from, false);
            match green_ph {
                Some(ph) => format!(
                    "{known} OR {} IN (SELECT value FROM json_each({ph}))",
                    domain_sql(&from)
                ),
                None => known,
            }
        }
    }
}

/// アドレスのドメイン（`greendomain::domain_of` と同じ: 最後の `@` の後ろ・前後の空白と末尾の `.` を
/// 落として小文字。`@` が無ければ NULL）。
///
/// 最後の `@` は `rtrim(a, <a から @ を除いた字>)` で `@` まで残し、その分を `replace` で落として取る
/// （残る後ろの部分に `@` は無いので、先頭の 1 回だけが置き換わる）。
fn domain_sql(addr: &str) -> String {
    let a = format!("trim({addr})");
    format!(
        "CASE WHEN instr({a}, '@') > 0
              THEN lower(rtrim(trim(replace({a}, rtrim({a}, replace({a}, '@', '')), '')), '.'))
         END"
    )
}

/// グリーンのドメイン集合を JSON の配列にする（`json_each` で引く）。
fn green_json(conn: &Connection) -> rusqlite::Result<String> {
    let set = green_domain_set(conn)?;
    let mut domains: Vec<&String> = set.iter().collect();
    domains.sort(); // 同じ集合なら同じ文字列に（試験・ログで見比べやすく）
    serde_json::to_string(&domains)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

/// 整数の並びを JSON の配列にする。
fn int_json(ids: &[i64]) -> String {
    let body = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    format!("[{body}]")
}

#[cfg(test)]
mod tests;
