//! 統合で余った Google の ID の扱い（docs/CONTACTS_SYNC.md §3-5「1 人のローカル連絡先を 2 つの
//! 外部 ID が掴まない」）。
//!
//! 統合すると、消える側のつながりは残す側へ寄せる。そのままだと同じ Google アカウントの ID が
//! 1 人に複数ぶら下がり、Google 側は 2 件とも残って同じ内容に更新され続ける。そこで
//! アカウントごとに 1 つだけ残し（残す側がもともと持っていた ID を優先、無ければ最初の 1 つ）、
//! 余りは「削除待ち」（`unlink_requested`）にする。次の同期の送信が、同期先を外すときと同じ
//! 経路で Google 側を削除し、送れたらつながりを片付ける。
//!
//! 解除中のアカウントの ID は削除待ちにしない（解除中は送らない作法に合わせる）。

use super::super::contact_sync::refresh_contact_dirty;
use super::super::Store;
use crate::models::MergeRemoteDeletion;
use rusqlite::{params_from_iter, Connection};
use std::collections::BTreeMap;

/// 統合に関わるつながり 1 本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MergeLink {
    /// `contact_identities.id`。
    pub id: i64,
    pub account_id: i64,
    /// 残す側の連絡先がもともと持っていたつながりか。
    pub on_keep: bool,
    /// アカウントが解除中か。
    pub disconnected: bool,
    /// アカウントの名前（カードの呼び名。無ければアドレス）。
    pub label: String,
}

/// 余るつながりを選ぶ。アカウントごとに 1 本残し（残す側のものを優先、無ければ最初の 1 本）、
/// 残りを返す。解除中のアカウントは選ばない。`links` は `id` の昇順で渡す。
pub(super) fn surplus(links: &[MergeLink]) -> Vec<&MergeLink> {
    let mut by_account: BTreeMap<i64, Vec<&MergeLink>> = BTreeMap::new();
    links
        .iter()
        .filter(|l| !l.disconnected)
        .for_each(|l| by_account.entry(l.account_id).or_default().push(l));
    by_account
        .into_values()
        .flat_map(|group| {
            let survivor = group
                .iter()
                .find(|l| l.on_keep)
                .or_else(|| group.first())
                .map(|l| l.id);
            group.into_iter().filter(move |l| Some(l.id) != survivor)
        })
        .collect()
}

/// 統合に関わる（残す側・消える側の）Google のつながりを読む。削除待ちのものは除く
/// （もう Google 側から消える予定なので、残す 1 本の候補にしない）。
pub(super) fn load_links(
    conn: &Connection,
    keep_id: i64,
    drop_ids: &[i64],
) -> rusqlite::Result<Vec<MergeLink>> {
    let ids: Vec<i64> = std::iter::once(keep_id)
        .chain(drop_ids.iter().copied())
        .collect();
    // 番号付きでそろえる（先頭の ?1 が残す側。番号なしの ? と混ぜると数がずれる）。
    let marks = (1..=ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut stmt = conn.prepare(&format!(
        "SELECT ci.id, ci.account_id, ci.contact_id = ?1, \
                COALESCE(ga.disconnected_at IS NOT NULL, 1), \
                COALESCE(p.display_name, ga.email, '') \
         FROM contact_identities ci \
         LEFT JOIN google_accounts ga ON ga.id = ci.account_id \
         LEFT JOIN account_profiles p ON p.id = ga.profile_id \
         WHERE ci.provider = 'google' AND ci.unlink_requested = 0 \
           AND ci.contact_id IN ({marks}) \
         ORDER BY ci.id"
    ))?;
    let rows = stmt.query_map(params_from_iter(ids.iter()), |r| {
        Ok(MergeLink {
            id: r.get(0)?,
            account_id: r.get(1)?,
            on_keep: r.get(2)?,
            disconnected: r.get(3)?,
            label: r.get(4)?,
        })
    })?;
    rows.collect()
}

/// 余るつながりを、アカウントごとの件数にまとめる（確認画面用）。
pub(super) fn summarize(links: &[MergeLink]) -> Vec<MergeRemoteDeletion> {
    let mut out: BTreeMap<i64, MergeRemoteDeletion> = BTreeMap::new();
    for l in surplus(links) {
        out.entry(l.account_id)
            .or_insert_with(|| MergeRemoteDeletion {
                account_id: l.account_id as i32,
                account_label: l.label.clone(),
                count: 0,
            })
            .count += 1;
    }
    out.into_values().collect()
}

/// 足し合わせる（アカウントごとの件数を合算する）。
pub(super) fn add_up(
    into: &mut BTreeMap<i64, MergeRemoteDeletion>,
    more: Vec<MergeRemoteDeletion>,
) {
    for d in more {
        into.entry(i64::from(d.account_id))
            .and_modify(|e| e.count += d.count)
            .or_insert(d);
    }
}

/// すでに同じ Google アカウントの ID が 2 つ以上ぶら下がっている連絡先（この取り決めより前の
/// 統合で寄せた人）。ゴミ箱の人は除く（削除の送信で全部消える）。
fn contacts_with_duplicate_ids(conn: &Connection) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT ci.contact_id FROM contact_identities ci \
         JOIN contacts c ON c.id = ci.contact_id AND c.deleted_at IS NULL \
         WHERE ci.provider = 'google' AND ci.unlink_requested = 0 \
         GROUP BY ci.contact_id, ci.account_id HAVING count(*) > 1 \
         ORDER BY ci.contact_id",
    )?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    rows.collect()
}

impl Store {
    /// 統合したら Google 側から消すことになる件数（アカウントごと）。読むだけ。統合の確認画面用。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn merge_remote_preview(
        &self,
        keep_id: i64,
        drop_ids: &[i64],
    ) -> rusqlite::Result<Vec<MergeRemoteDeletion>> {
        let conn = self.conn.lock().unwrap();
        Ok(summarize(&load_links(&conn, keep_id, drop_ids)?))
    }

    /// 統合より前から残っている Google の重複（1 人に同じアカウントの ID が 2 つ以上）を、
    /// 片付けたら消すことになる件数（アカウントごと）。読むだけ。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn duplicate_remote_ids(&self) -> rusqlite::Result<Vec<MergeRemoteDeletion>> {
        let conn = self.conn.lock().unwrap();
        let mut total = BTreeMap::new();
        for cid in contacts_with_duplicate_ids(&conn)? {
            add_up(&mut total, summarize(&load_links(&conn, cid, &[])?));
        }
        Ok(total.into_values().collect())
    }

    /// 統合より前から残っている Google の重複を、統合と同じ規則で片付ける（1 人・1 アカウントに
    /// 1 つ残し、余りを削除待ちにする。次の同期で Google 側から削除する）。削除待ちにした件数を返す。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき（全体を巻き戻す）。
    pub fn tidy_duplicate_remote_ids(&self) -> rusqlite::Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut marked = 0;
        for cid in contacts_with_duplicate_ids(&tx)? {
            marked += mark_surplus(&tx, &load_links(&tx, cid, &[])?)?;
            refresh_contact_dirty(&tx, cid)?;
        }
        tx.commit()?;
        Ok(marked)
    }
}

/// 余るつながりを削除待ちにする（統合のトランザクションの中で、つながりを寄せる前に呼ぶ）。
pub(super) fn mark_surplus(conn: &Connection, links: &[MergeLink]) -> rusqlite::Result<usize> {
    let mut stmt =
        conn.prepare("UPDATE contact_identities SET unlink_requested = 1 WHERE id = ?1")?;
    surplus(links)
        .iter()
        .try_fold(0, |n, l| stmt.execute([l.id]).map(|k| n + k))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(id: i64, account_id: i64, on_keep: bool, disconnected: bool) -> MergeLink {
        MergeLink {
            id,
            account_id,
            on_keep,
            disconnected,
            label: format!("acct{account_id}"),
        }
    }

    fn ids(v: Vec<&MergeLink>) -> Vec<i64> {
        v.into_iter().map(|l| l.id).collect()
    }

    #[test]
    fn keeps_the_id_the_survivor_already_had() {
        // 消える側の ID（1）のほうが古くても、残す側のもの（2）を残す。
        let links = [link(1, 7, false, false), link(2, 7, true, false)];
        assert_eq!(ids(surplus(&links)), vec![1]);
    }

    #[test]
    fn falls_back_to_the_first_when_the_survivor_had_none() {
        let links = [
            link(3, 7, false, false),
            link(5, 7, false, false),
            link(9, 7, false, false),
        ];
        assert_eq!(ids(surplus(&links)), vec![5, 9]);
    }

    #[test]
    fn different_accounts_each_keep_one() {
        let links = [link(1, 7, true, false), link(2, 8, false, false)];
        assert!(surplus(&links).is_empty());
    }

    #[test]
    fn disconnected_accounts_are_left_alone() {
        let links = [link(1, 7, true, true), link(2, 7, false, true)];
        assert!(surplus(&links).is_empty());
    }

    #[test]
    fn summary_counts_per_account() {
        let links = [
            link(1, 7, true, false),
            link(2, 7, false, false),
            link(3, 7, false, false),
            link(4, 8, false, false),
        ];
        assert_eq!(
            summarize(&links),
            vec![MergeRemoteDeletion {
                account_id: 7,
                account_label: "acct7".into(),
                count: 2
            }]
        );
    }
}
