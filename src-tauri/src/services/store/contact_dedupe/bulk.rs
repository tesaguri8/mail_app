//! 確実な重複をまとめて統合する（重複の整理の「確実な重複をまとめて統合」）。
//!
//! 基準は `services::sure_duplicates`（1 つの関数）。統合そのものは 1 件ずつの統合と同じ
//! `merge_in`（Google の余りを削除待ちにする規則 `merge_remote` もそこを通る）。新しい経路は作らない。
//! 「別人」と記録した対はまとめない（`contact_distinct`）。

use super::super::contact_distinct::load_distinct;
use super::super::contact_rows::load_all_full;
use super::super::Store;
use super::merge_in;
use super::merge_remote::{add_up, load_links, summarize};
use crate::models::{SureMergeGroup, SureMergePreview, SureMergeResult};
use crate::services::sure_duplicates::sure_groups;
use std::collections::{BTreeMap, HashMap};

impl Store {
    /// 確実な重複をまとめて統合したらどうなるか（組の一覧・件数・Google から消す件数）。読むだけ。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn sure_merge_preview(&self) -> rusqlite::Result<SureMergePreview> {
        let conn = self.conn.lock().unwrap();
        let contacts = load_all_full(&conn)?;
        let by_id: HashMap<i64, _> = contacts.iter().map(|c| (i64::from(c.id), c)).collect();
        let found = sure_groups(&contacts, &load_distinct(&conn)?);
        let mut remote = BTreeMap::new();
        let mut groups = Vec::with_capacity(found.len());
        for g in &found {
            add_up(
                &mut remote,
                summarize(&load_links(&conn, g.keep, &g.drops)?),
            );
            if let Some(keep) = by_id.get(&g.keep) {
                groups.push(SureMergeGroup {
                    keep_id: keep.id,
                    display_name: keep.fields.display_name.clone(),
                    count: (g.drops.len() + 1) as i32,
                    email: keep.fields.emails.first().map(|e| e.value.clone()),
                    phone: keep.fields.phones.first().map(|p| p.value.clone()),
                });
            }
        }
        Ok(SureMergePreview {
            contacts: groups.iter().map(|g| g.count).sum(),
            groups,
            remote_deletions: remote.into_values().collect(),
        })
    }

    /// 確実な重複をまとめて統合する。組は実行のときに数え直す（下見のあとに変わっていても、
    /// そのときの確実な組だけをまとめる）。全体を 1 つのトランザクションで行い、途中で失敗したら
    /// 何もまとめない。
    ///
    /// # Errors
    /// DB の読み書きに失敗したとき（全体を巻き戻す）。
    pub fn merge_sure_duplicates(&self) -> rusqlite::Result<SureMergeResult> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let found = sure_groups(&load_all_full(&tx)?, &load_distinct(&tx)?);
        let mut result = SureMergeResult::default();
        for g in &found {
            result.remote_deletions += merge_in(&tx, g.keep, &g.drops)? as i32;
            result.groups += 1;
            result.merged += g.drops.len() as i32;
        }
        tx.commit()?;
        log::info!(
            "確実な重複をまとめて統合: {} 組・{} 件をまとめ、Google 側の削除待ち {} 件",
            result.groups,
            result.merged,
            result.remote_deletions
        );
        Ok(result)
    }
}
