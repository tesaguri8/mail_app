//! 「別人」の記録（`contact_distinct_pairs`・マイグレーション 0064・docs/CONTACT_MODEL.md §1-5）。
//!
//! 重複の整理で「別人（統合しない）」を押した組・統合でチェックを外した人を、uid の対で記録する。
//! 重複の検出とまとめての統合は、ここで読んだ対（連絡先 ID に引き直したもの）に従って同じ組に
//! しない（判定は `services::distinct`）。連絡先が消えたら対も消える（外部キーの CASCADE）。
//! 統合では、消える側の対を残る側へ付け替えてから消す（[`inherit`]）。

use super::Store;
use crate::models::DistinctPair;
use crate::services::distinct::DistinctPairs;
use rusqlite::{params, Connection};

/// 記録をすべて読み、連絡先 ID の対にして返す。
pub(super) fn load_distinct(conn: &Connection) -> rusqlite::Result<DistinctPairs> {
    let mut stmt = conn.prepare(
        "SELECT a.id, b.id FROM contact_distinct_pairs p \
         JOIN contacts a ON a.uid = p.uid_a JOIN contacts b ON b.uid = p.uid_b",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    Ok(DistinctPairs::new(
        rows.collect::<rusqlite::Result<Vec<_>>>()?,
    ))
}

/// 2 人を別人として記録する（同じ人・既にある対は何もしない）。記録した数を返す。
pub(super) fn record(conn: &Connection, a: i64, b: i64) -> rusqlite::Result<usize> {
    if a == b {
        return Ok(0);
    }
    conn.execute(
        "INSERT OR IGNORE INTO contact_distinct_pairs (uid_a, uid_b) \
         SELECT min(x.uid, y.uid), max(x.uid, y.uid) FROM contacts x, contacts y \
         WHERE x.id = ?1 AND y.id = ?2",
        params![a, b],
    )
}

/// 統合で消える人（`drop_id`）の「別人」の記録を、残る人（`keep_id`）へ付け替える。消える人の
/// 行を消す前に呼ぶ（消すと CASCADE で記録も消える）。残る人自身との対は付け替えない。
pub(super) fn inherit(conn: &Connection, keep_id: i64, drop_id: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT OR IGNORE INTO contact_distinct_pairs (uid_a, uid_b) \
         SELECT min(k.uid, o.other), max(k.uid, o.other) FROM \
           (SELECT CASE WHEN p.uid_a = d.uid THEN p.uid_b ELSE p.uid_a END AS other \
            FROM contact_distinct_pairs p JOIN contacts d ON d.id = ?2 \
            WHERE p.uid_a = d.uid OR p.uid_b = d.uid) o, \
           contacts k \
         WHERE k.id = ?1 AND o.other <> k.uid",
        params![keep_id, drop_id],
    )
}

impl Store {
    /// 組の人どうしを、すべて「別人」として記録する（重複の整理の「別人（統合しない）」）。
    /// 新しく記録した対の数を返す。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき（巻き戻す）。
    pub fn mark_contacts_distinct(&self, ids: &[i64]) -> rusqlite::Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let n = ids
            .iter()
            .enumerate()
            .flat_map(|(i, a)| ids[i + 1..].iter().map(move |b| (*a, *b)))
            .try_fold(0, |n, (a, b)| record(&tx, a, b).map(|k| n + k))?;
        tx.commit()?;
        Ok(n)
    }

    /// 「別人」として記録した対の一覧（新しい順。どちらかがゴミ箱の対は出さない）。取り消しの
    /// 入口に使う。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn distinct_pairs(&self) -> rusqlite::Result<Vec<DistinctPair>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT a.id, a.display_name, b.id, b.display_name FROM contact_distinct_pairs p \
             JOIN contacts a ON a.uid = p.uid_a AND a.deleted_at IS NULL \
             JOIN contacts b ON b.uid = p.uid_b AND b.deleted_at IS NULL \
             ORDER BY p.created_at DESC, a.display_name, b.display_name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(DistinctPair {
                a_id: r.get::<_, i64>(0)? as i32,
                a_name: r.get(1)?,
                b_id: r.get::<_, i64>(2)? as i32,
                b_name: r.get(3)?,
            })
        })?;
        rows.collect()
    }

    /// 「別人」の記録を取り消す（次の重複の検出から、また同じ組に出る）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn unmark_contacts_distinct(&self, a: i64, b: i64) -> rusqlite::Result<usize> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM contact_distinct_pairs WHERE (uid_a, uid_b) IN \
             (SELECT min(x.uid, y.uid), max(x.uid, y.uid) FROM contacts x, contacts y \
              WHERE x.id = ?1 AND y.id = ?2)",
            params![a, b],
        )
    }
}

#[cfg(test)]
mod tests;
