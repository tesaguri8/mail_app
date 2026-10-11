//! 会社名だけの連絡先を、組織の連絡先へ切り替える（docs/ORG_CONTACTS.md §2-2・§4-1）。
//!
//! 会社名だけの連絡先（姓名が空で、表示名が先頭の会社名と同じ人）は、もう組織の連絡先と同じ形を
//! している（自分の会社の行に名前がある）。消して作り直さずに**その場で**種別を変えるので、
//! Google のつながり・`uid`・別人の記録・メールとの関係はそのまま残る。
//!
//! - [`merge_cards_with_company_only`] — 0066 で組織カードから作った組織を、同じ名前の会社名だけの
//!   連絡先とまとめる（起動時に 1 回だけ）
//! - [`Store::company_only_candidates`] / [`Store::convert_to_organizations`] — 整理の候補と切り替え

use super::contact_dedupe::merge_in;
use super::contact_rows::load_contact;
use super::contact_write::{find_org_by_key, mark_dirty, write_contact, OrgLinking, WriteOptions};
use super::organizations::sync_member_names;
use super::Store;
use crate::models::{CompanyOnlyCandidate, ContactKind, ContactListItem, ContactSummary};
use crate::services::contact_fields::{is_blank, is_company_only};
use crate::services::dedupe::normalize_org;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

/// まとめの版。まとめ方を変えたら上げる（次回起動でもう一度見直す）。
const MERGE_VERSION: &str = "1";
/// 実施済みの版を控える設定キー（app_settings）。
const KEY_MERGED: &str = "org_contacts.merged_version";

/// 先頭の会社名の正規化名（無ければ空）。
fn company_key(c: &ContactSummary) -> String {
    c.fields
        .organizations
        .first()
        .and_then(|o| o.name.as_deref())
        .map(normalize_org)
        .unwrap_or_default()
}

/// 会社名だけの連絡先（ゴミ箱にない人）を、正規化した会社名ごとに集める（id の古い順）。
fn company_only_by_key(
    conn: &Connection,
) -> rusqlite::Result<HashMap<String, Vec<ContactSummary>>> {
    let ids: Vec<i64> = {
        let mut stmt = conn.prepare(
            "SELECT c.id FROM contacts c \
             WHERE c.kind = 'person' AND c.deleted_at IS NULL \
               AND COALESCE(trim(c.family_name), '') || COALESCE(trim(c.middle_name), '') || \
                   COALESCE(trim(c.given_name), '') = '' \
               AND EXISTS (SELECT 1 FROM contact_organizations co WHERE co.contact_id = c.id) \
             ORDER BY c.id",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut out: HashMap<String, Vec<ContactSummary>> = HashMap::new();
    for id in ids {
        let c = load_contact(conn, id)?;
        if is_company_only(&c.fields) {
            out.entry(company_key(&c)).or_default().push(c);
        }
    }
    Ok(out)
}

/// つながっているサービスの数。
fn identity_count(conn: &Connection, id: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT count(*) FROM contact_identities WHERE contact_id = ?1",
        params![id],
        |r| r.get(0),
    )
}

/// まとめるときに残す側（つながりの多い方、同点なら古い方）。
fn pick_survivor(conn: &Connection, ids: &[i64]) -> rusqlite::Result<Option<i64>> {
    let mut best: Option<(i64, i64)> = None;
    for &id in ids {
        let n = identity_count(conn, id)?;
        let better = match best {
            None => true,
            Some((bid, bn)) => n > bn || (n == bn && id < bid),
        };
        if better {
            best = Some((id, n));
        }
    }
    Ok(best.map(|(id, _)| id))
}

/// 0066 で組織カードから作った組織（どこにもつながっていない組織）を、正規化した名前が同じ
/// 会社名だけの連絡先とまとめる（`Store::open` から一度だけ）。
///
/// 残すのは会社名だけの連絡先の側（Google のつながり・`uid`・別人の記録・メールとの関係を持つ）。
/// 組織カードの代表の値とメモはそちらへ寄せ、人のつながりも付け替える。同じ名前の会社名だけの
/// 連絡先が 2 件以上あれば、つながりの多い方（同点なら古い方）とまとめ、残りは整理の候補に残す。
///
/// 戻り値はまとめた組織の数。
pub fn merge_cards_with_company_only(conn: &Connection) -> rusqlite::Result<usize> {
    let done: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![KEY_MERGED],
            |r| r.get(0),
        )
        .optional()?;
    if done.as_deref() == Some(MERGE_VERSION) {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let mut groups = company_only_by_key(&tx)?;
    let orgs: Vec<(i64, String)> = {
        let mut stmt = tx.prepare(
            "SELECT c.id, c.display_name FROM contacts c \
             WHERE c.kind = 'organization' AND c.deleted_at IS NULL \
               AND NOT EXISTS (SELECT 1 FROM contact_identities ci WHERE ci.contact_id = c.id) \
             ORDER BY c.id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut merged = 0;
    for (org_id, name) in orgs {
        let Some(cands) = groups.get_mut(&normalize_org(&name)) else {
            continue;
        };
        let ids: Vec<i64> = cands.iter().map(|c| i64::from(c.id)).collect();
        let Some(keep) = pick_survivor(&tx, &ids)? else {
            continue;
        };
        cands.retain(|c| i64::from(c.id) != keep);
        // 消える側（カードから作った組織）が組織なので、まとめた結果も組織になる。
        merge_in(&tx, keep, &[org_id])?;
        merged += 1;
    }
    tx.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![KEY_MERGED, MERGE_VERSION],
    )?;
    tx.commit()?;
    Ok(merged)
}

/// 正規化した会社名が `key` で、まだ組織につながっていない人の会社の行（行 id, 連絡先 id）。
fn unlinked_rows(conn: &Connection, key: &str, except: i64) -> rusqlite::Result<Vec<(i64, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT co.id, co.contact_id, co.name FROM contact_organizations co \
         JOIN contacts c ON c.id = co.contact_id \
         WHERE co.org_id IS NULL AND c.kind = 'person' AND c.deleted_at IS NULL \
           AND c.id <> ?1",
    )?;
    let rows = stmt.query_map(params![except], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (row_id, cid, name) = row?;
        if name.as_deref().map(normalize_org).as_deref() == Some(key) {
            out.push((row_id, cid));
        }
    }
    Ok(out)
}

impl Store {
    /// 組織の連絡先へ切り替える候補（会社名だけの連絡先）を、会社名の順に返す。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn company_only_candidates(&self) -> rusqlite::Result<Vec<CompanyOnlyCandidate>> {
        let conn = self.conn.lock().unwrap();
        let groups = company_only_by_key(&conn)?;
        let mut out = Vec::new();
        for (key, cands) in groups {
            if key.is_empty() {
                continue;
            }
            let existing_org = find_org_by_key(&conn, &key)?.map(|(id, _)| id as i32);
            for c in cands {
                let first = c.fields.organizations.first();
                let company = first.and_then(|o| o.name.clone()).unwrap_or_default();
                let has_title =
                    first.is_some_and(|o| !is_blank(&o.title) || !is_blank(&o.department));
                let same_company_people = unlinked_rows(&conn, &key, i64::from(c.id))?
                    .iter()
                    .map(|(_, cid)| *cid)
                    .collect::<std::collections::HashSet<_>>()
                    .len() as i32;
                out.push(CompanyOnlyCandidate {
                    contact: ContactListItem::from(c),
                    company,
                    has_title,
                    existing_org,
                    same_company_people,
                });
            }
        }
        out.sort_by(|a, b| {
            a.company
                .cmp(&b.company)
                .then_with(|| a.contact.id.cmp(&b.contact.id))
        });
        Ok(out)
    }

    /// 会社名だけの連絡先を、その場で組織の連絡先に切り替える（docs/ORG_CONTACTS.md §4-1）。
    ///
    /// 種別を変えるだけで、つながり・`uid`・別人の記録は残る。同じ名前の組織が既にあれば、
    /// その組織とまとめる（つながりの多い方を残す）。同じ会社名で組織につながっていない人は、
    /// 切り替えた組織へつなぐ（会社名は組織の名前にそろえ、変わった人は送り直す）。
    /// 会社名だけの連絡先でなくなっていた id は飛ばす。戻り値は切り替えた数。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき（全体を巻き戻す）。
    pub fn convert_to_organizations(&self, ids: &[i64]) -> rusqlite::Result<i32> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut converted = 0;
        for &id in ids {
            let mut c = load_contact(&tx, id)?;
            if c.deleted_at.is_some() || !is_company_only(&c.fields) {
                continue;
            }
            let key = company_key(&c);
            let org_id = match find_org_by_key(&tx, &key)? {
                Some((existing, _)) => {
                    let Some(keep) = pick_survivor(&tx, &[existing, id])? else {
                        continue;
                    };
                    let drop = if keep == existing { id } else { existing };
                    merge_in(&tx, keep, &[drop])?;
                    keep
                }
                None => {
                    c.fields.kind = ContactKind::Organization;
                    // 送り直す（Google では名前を空にした組織の形になる / §3-1）。
                    write_contact(
                        &tx,
                        Some(id),
                        &c.fields,
                        WriteOptions {
                            mark_dirty: true,
                            org_linking: OrgLinking::ExistingOnly,
                        },
                    )?;
                    id
                }
            };
            for (row_id, cid) in unlinked_rows(&tx, &key, org_id)? {
                tx.execute(
                    "UPDATE contact_organizations SET org_id = ?1 WHERE id = ?2",
                    params![org_id, row_id],
                )?;
                mark_dirty(&tx, cid)?;
            }
            sync_member_names(&tx, org_id)?;
            converted += 1;
        }
        tx.commit()?;
        Ok(converted)
    }
}

#[cfg(test)]
mod tests;
