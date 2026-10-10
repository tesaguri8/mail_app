//! 連絡先の重複整理（重複候補の検出・入力中の一致確認・統合）。
//!
//! 判定の物差しは `services::dedupe`（record linkage）。照合（Google から取り込んだ人を住所録の
//! 誰と結び付けるか）も同じ物差しを使うので、照合で決めきれずに新規にした人はここで拾える。

use super::contact_distinct::{inherit, load_distinct, record};
use super::contact_rows::{load_all_full, load_contact};
use super::contact_tags::set_tags;
use super::contact_targets::drop_redundant_create_requests;
use super::contact_write::{write_contact, OrgLinking, WriteOptions};
use super::Store;
use crate::models::{ContactMatch, ContactSummary, DuplicateGroup};
use crate::services::contact_fields::union_merge;
use crate::services::dedupe::{digits, fold, fold_remove_ws, mobile_number};
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

/// この人数以上で使い回されている値（会社/役所の代表メール・代表電話など）は、共有の印が
/// 無くても共有とみなして手掛かりから除く（前任からの引き継ぎで同じ代表値を持つ別人を
/// 重複扱いしないため）。
const SHARED_MIN_CONTACTS: usize = 3;

/// 照合用に電話番号を正規化する。携帯は国番号(+81)を吸収して 0 始まり 11 桁へ、
/// それ以外（固定電話/FAX 等）は数字のみへ。
fn normalize_phone_for_match(raw: &str) -> String {
    mobile_number(raw).unwrap_or_else(|| digits(raw))
}

/// 共有とみなす値の集合（正規化済み）。共有の印が付いた値と、多人数で使い回されている値。
fn shared_values(
    conn: &Connection,
    table: &str,
    norm: fn(&str) -> String,
) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT v.value, v.contact_id, v.is_shared FROM {table} v \
         JOIN contacts c ON c.id = v.contact_id WHERE c.deleted_at IS NULL"
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)? != 0,
        ))
    })?;
    let mut owners: HashMap<String, HashSet<i64>> = HashMap::new();
    let mut shared: HashSet<String> = HashSet::new();
    for row in rows {
        let (value, cid, is_shared) = row?;
        let key = norm(&value);
        if key.is_empty() {
            continue;
        }
        if is_shared {
            shared.insert(key.clone());
        }
        owners.entry(key).or_default().insert(cid);
    }
    shared.extend(
        owners
            .into_iter()
            .filter(|(_, ids)| ids.len() >= SHARED_MIN_CONTACTS)
            .map(|(k, _)| k),
    );
    Ok(shared)
}

/// (入力の原文, 正規化) の対。空になるものは除く。
fn normalized_pairs(values: &[String], norm: fn(&str) -> String) -> Vec<(String, String)> {
    values
        .iter()
        .map(|v| (v.clone(), norm(v)))
        .filter(|(_, n)| !n.is_empty())
        .collect()
}

/// 入力値（原文, 正規化）のどれかと一致する値を持つ連絡先を (contact_id, 入力の原文) で返す。
/// 共有の印が付いた値・共有とみなす値は手掛かりにしない。
fn value_hits(
    conn: &Connection,
    table: &str,
    wanted: &[(String, String)],
    shared: &HashSet<String>,
    norm: fn(&str) -> String,
) -> rusqlite::Result<Vec<(i64, String)>> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT v.contact_id, v.value FROM {table} v JOIN contacts c ON c.id = v.contact_id \
         WHERE v.is_shared = 0 AND c.deleted_at IS NULL"
    ))?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (cid, value) = row?;
        let key = norm(&value);
        if key.is_empty() || shared.contains(&key) {
            continue;
        }
        out.extend(
            wanted
                .iter()
                .filter(|(_, n)| *n == key)
                .map(|(orig, _)| (cid, orig.clone())),
        );
    }
    Ok(out)
}

fn email_norm(v: &str) -> String {
    fold(v).trim().to_string()
}

impl Store {
    /// 重複候補を record linkage で束ねて返す（2 件以上のみ、確信度順）。候補の連絡先は
    /// 子テーブルまで充填して返す（整理画面で中身を見比べるため）。「別人」と記録した対は
    /// 同じ組にしない。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn find_duplicate_groups(&self) -> rusqlite::Result<Vec<DuplicateGroup>> {
        let contacts = self.contacts_for_dedupe()?;
        let distinct = load_distinct(&self.conn.lock().unwrap())?;
        Ok(crate::services::dedupe::group(&contacts, &distinct))
    }

    /// 削除済みを除く全員を中身まで充填して返す（重複検出と照合で共有する材料）。
    pub(super) fn contacts_for_dedupe(&self) -> rusqlite::Result<Vec<ContactSummary>> {
        let conn = self.conn.lock().unwrap();
        load_all_full(&conn)
    }

    /// 入力（メール/電話/FAX/氏名）に一致する既存連絡先を返す。新規登録前チェック・
    /// 編集中の赤字警告・メールからの＋追加で使う。共有とみなす値は手掛かりから除く。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn find_contact_matches(
        &self,
        emails: &[String],
        phones: &[String],
        display_name: Option<&str>,
        exclude_id: Option<i64>,
    ) -> rusqlite::Result<Vec<ContactMatch>> {
        let conn = self.conn.lock().unwrap();
        let want_emails = normalized_pairs(emails, email_norm);
        let want_phones = normalized_pairs(phones, normalize_phone_for_match);
        let want_name = display_name.map(fold_remove_ws).filter(|s| !s.is_empty());
        let shared_emails = shared_values(&conn, "contact_emails", email_norm)?;
        let shared_phones = shared_values(&conn, "contact_phones", normalize_phone_for_match)?;

        // contact_id -> (一致メール, 一致電話, 氏名一致)
        let mut hits: HashMap<i64, (Vec<String>, Vec<String>, bool)> = HashMap::new();
        for (cid, orig) in value_hits(
            &conn,
            "contact_emails",
            &want_emails,
            &shared_emails,
            email_norm,
        )? {
            hits.entry(cid).or_default().0.push(orig);
        }
        for (cid, orig) in value_hits(
            &conn,
            "contact_phones",
            &want_phones,
            &shared_phones,
            normalize_phone_for_match,
        )? {
            hits.entry(cid).or_default().1.push(orig);
        }
        if let Some(name) = &want_name {
            let mut stmt =
                conn.prepare("SELECT id, display_name FROM contacts WHERE deleted_at IS NULL")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows {
                let (cid, dn) = row?;
                if &fold_remove_ws(&dn) == name {
                    hits.entry(cid).or_default().2 = true;
                }
            }
        }
        if let Some(ex) = exclude_id {
            hits.remove(&ex);
        }

        let mut out = hits
            .into_iter()
            .map(|(cid, (mut m_emails, mut m_phones, m_name))| {
                let c = load_contact(&conn, cid)?;
                m_emails.sort();
                m_emails.dedup();
                m_phones.sort();
                m_phones.dedup();
                Ok(ContactMatch {
                    id: c.id,
                    display_name: c.fields.display_name,
                    organization: c.primary_organization,
                    email: c.primary_email,
                    phone: c.primary_phone,
                    matched_emails: m_emails,
                    matched_phones: m_phones,
                    matched_name: m_name,
                })
            })
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // 強い一致（メール/電話）を先に、次いで氏名順。
        out.sort_by(|a, b| {
            let strong =
                |m: &ContactMatch| !m.matched_emails.is_empty() || !m.matched_phones.is_empty();
            strong(b)
                .cmp(&strong(a))
                .then_with(|| a.display_name.cmp(&b.display_name))
        });
        Ok(out)
    }

    /// 複数の連絡先を 1 件（`keep_id`）に統合し、統合後の連絡先を返す。
    ///
    /// 中身は和集合（[`union_merge`]）、フラグは OR。消える側のつながり（Google 等）とタグは
    /// 残す側へ寄せる — つながりを移さないと、次の同期で同じ人がもう一度新規として起こされる。
    ///
    /// 寄せた結果、同じ Google アカウントの ID が複数になるときは 1 つだけ残し（残す側がもともと
    /// 持っていたものを優先）、余りを削除待ちにする（次の同期で Google 側から削除する。
    /// 解除中のアカウントは除く。`merge_remote`）。
    ///
    /// `distinct_ids` は組にいたが統合でチェックを外した人。統合後の 1 人と「別人」として記録し、
    /// 次から同じ組に出さない（利用者の判断 2026-10-10）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき（全体を巻き戻す）。
    pub fn merge_contacts(
        &self,
        keep_id: i64,
        drop_ids: &[i64],
        distinct_ids: &[i64],
    ) -> rusqlite::Result<ContactSummary> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        // 消える人がいなければ中身は変えない（送信待ちにもしない）。
        if !drop_ids.is_empty() {
            merge_in(&tx, keep_id, drop_ids)?;
        }
        distinct_ids
            .iter()
            .try_for_each(|id| record(&tx, keep_id, *id).map(|_| ()))?;
        tx.commit()?;
        load_contact(&conn, keep_id)
    }
}

/// 統合の本体（呼び出し側のトランザクションの中で行う。1 件ずつの統合とまとめての統合で共有）。
/// Google 側から消すために削除待ちにしたつながりの数を返す。
fn merge_in(tx: &Connection, keep_id: i64, drop_ids: &[i64]) -> rusqlite::Result<usize> {
    // 同じ Google アカウントの ID は 1 つだけ残し、余りは次の同期で Google 側から消す
    // （寄せる前に、どれが残す側のものかを見て決める）。
    let marked = merge_remote::mark_surplus(tx, &merge_remote::load_links(tx, keep_id, drop_ids)?)?;
    let keep = load_contact(tx, keep_id)?;
    let drops = drop_ids
        .iter()
        .map(|id| load_contact(tx, *id))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let parts: Vec<_> = std::iter::once(&keep.fields)
        .chain(drops.iter().map(|d| &d.fields))
        .collect();
    let merged = union_merge(&parts);
    for id in drop_ids {
        tx.execute(
            "UPDATE contact_identities SET contact_id = ?1 WHERE contact_id = ?2",
            params![keep_id, id],
        )?;
        // 「別人」の記録も残す側へ付け替える（消すと CASCADE で消えるので先に）。
        inherit(tx, keep_id, *id)?;
        // 作成待ちも残す側へ寄せる（同じアカウントの重なりは主キーで 1 つになる）。
        tx.execute(
            "UPDATE OR IGNORE contact_create_requests SET contact_id = ?1 WHERE contact_id = ?2",
            params![keep_id, id],
        )?;
        tx.execute("DELETE FROM contacts WHERE id = ?1", params![id])?;
    }
    write_contact(
        tx,
        Some(keep_id),
        &merged,
        WriteOptions {
            mark_dirty: true,
            org_linking: OrgLinking::ExistingOnly,
        },
    )?;
    set_tags(tx, keep_id, &merged.tags)?;
    drop_redundant_create_requests(tx, keep_id)?;
    Ok(marked)
}

mod bulk;
mod merge_remote;

#[cfg(test)]
mod tests;
