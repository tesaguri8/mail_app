//! 組織カードの整理（組織タブの「整理」の裏側）。docs/CONTACT_MODEL.md §1-5-1。
//!
//! 同期の取り込みは組織カードを自動では作らない（取引の無い会社や表記ゆれまでカードになって
//! しまうため）。どれをカードにするか・誰をつなぐかは人が選ぶ。ここは**候補を出す**ことと、
//! 選ばれたものを**つなぐ・作る**ことだけを受け持つ。名前の物差しは組織の重複整理と同じ
//! 正規化（`dedupe::normalize_org`）。

use super::contact_rows::{query_summaries, SUMMARY_ORDER};
use super::contact_write::{find_org_by_key, mark_dirty};
use super::greendomain::{domain_of, is_unaffiliated_domain};
use super::organizations::{load_org, sync_member_names};
use super::Store;
use crate::models::{
    OrgChangeImpact, OrgLinkCandidate, OrgLinkSuggestion, OrganizationSummary, UnlinkedOrgName,
};
use crate::services::dedupe::normalize_org;
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, HashMap, HashSet};

/// 正規化名 1 つぶんの集計: (表記 → その表記を使う人, その会社名を持つ人)。
type NameGroup = (HashMap<String, HashSet<i64>>, HashSet<i64>);

/// 会社の行（contact_organizations）1 件。
struct OrgRow {
    row_id: i64,
    contact_id: i64,
    org_id: Option<i64>,
    name: Option<String>,
}

/// 削除されていない連絡先の会社の行をすべて読む。
fn org_rows(conn: &Connection) -> rusqlite::Result<Vec<OrgRow>> {
    let mut stmt = conn.prepare(
        "SELECT co.id, co.contact_id, co.org_id, co.name FROM contact_organizations co \
         JOIN contacts c ON c.id = co.contact_id WHERE c.deleted_at IS NULL \
         ORDER BY co.contact_id, co.position, co.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(OrgRow {
            row_id: r.get(0)?,
            contact_id: r.get(1)?,
            org_id: r.get(2)?,
            name: r.get(3)?,
        })
    })?;
    rows.collect()
}

/// 正規化名（空なら None）。
fn key_of(name: Option<&str>) -> Option<String> {
    name.map(normalize_org).filter(|k| !k.is_empty())
}

/// 削除されていない組織カードの (id, 名前, 代表メール)。
fn cards(conn: &Connection) -> rusqlite::Result<Vec<(i64, String, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, email FROM organizations WHERE deleted_at IS NULL ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    rows.collect()
}

/// 連絡先ごとのメールのドメイン（同じ組織の手掛かりにならないドメインを除く）。
fn contact_domains(conn: &Connection) -> rusqlite::Result<HashMap<i64, Vec<String>>> {
    let mut stmt = conn.prepare(
        "SELECT ce.contact_id, ce.value FROM contact_emails ce \
         JOIN contacts c ON c.id = ce.contact_id WHERE c.deleted_at IS NULL",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    for row in rows {
        let (cid, value) = row?;
        if let Some(d) = domain_of(&value).filter(|d| !is_unaffiliated_domain(d)) {
            let list = out.entry(cid).or_default();
            if !list.contains(&d) {
                list.push(d);
            }
        }
    }
    Ok(out)
}

/// 外部サービスとつながっている連絡先（変更すると次の同期で送り直しになる人）。
fn synced_contacts(conn: &Connection) -> rusqlite::Result<HashSet<i64>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT contact_id FROM contact_identities WHERE contact_id IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    rows.collect()
}

/// 変わる人のうち、送り直しになる人の数。
fn impact_of(
    conn: &Connection,
    changed: impl IntoIterator<Item = i64>,
) -> rusqlite::Result<OrgChangeImpact> {
    let synced = synced_contacts(conn)?;
    let resent: HashSet<i64> = changed.into_iter().filter(|c| synced.contains(c)).collect();
    Ok(OrgChangeImpact {
        resent: i32::try_from(resent.len()).unwrap_or(i32::MAX),
    })
}

/// 会社名から組織カードを作る（同じ正規化名のカードがあればそこへつなぐ）段取り。
struct CreatePlan {
    /// 同じ正規化名の既存カード。
    existing: Option<i64>,
    /// つないだ人の会社名をそろえる先（既存カードの名前、無ければ入力した名前）。
    card_name: String,
    /// つなぐ会社の行: (行 id, 連絡先 id)。
    rows: Vec<(i64, i64)>,
    /// 会社名がカードの名前に変わる人。
    renamed: Vec<i64>,
}

fn create_plan(conn: &Connection, name: &str) -> rusqlite::Result<CreatePlan> {
    let name = name.trim();
    let key = normalize_org(name);
    let existing = find_org_by_key(conn, &key)?;
    let card_name = existing
        .as_ref()
        .map_or_else(|| name.to_string(), |(_, n)| n.clone());
    let targets: Vec<OrgRow> = org_rows(conn)?
        .into_iter()
        .filter(|r| {
            r.org_id.is_none() && key_of(r.name.as_deref()).as_deref() == Some(key.as_str())
        })
        .collect();
    let renamed = targets
        .iter()
        .filter(|r| r.name.as_deref() != Some(card_name.as_str()))
        .map(|r| r.contact_id)
        .collect();
    Ok(CreatePlan {
        existing: existing.map(|(id, _)| id),
        card_name,
        rows: targets.iter().map(|r| (r.row_id, r.contact_id)).collect(),
        renamed,
    })
}

/// 1 人をカードへつなぐ手。
enum LinkStep {
    /// 既にある会社の行をつなぐ（`renamed`: 会社名がカードの名前に変わる）。
    Attach { row_id: i64, renamed: bool },
    /// 会社を 1 つ足してつなぐ。
    Add,
}

/// 選んだ人を組織カードへつなぐ段取り（既につながっている人は含まない）。
fn link_plan(
    conn: &Connection,
    card: &OrganizationSummary,
    contact_ids: &[i64],
) -> rusqlite::Result<Vec<(i64, LinkStep)>> {
    let org_id = i64::from(card.id);
    let key = key_of(Some(&card.name));
    let rows = org_rows(conn)?;
    let steps = contact_ids
        .iter()
        .filter_map(|cid| {
            let mine: Vec<&OrgRow> = rows.iter().filter(|r| r.contact_id == *cid).collect();
            if mine.iter().any(|r| r.org_id == Some(org_id)) {
                return None;
            }
            let target = mine
                .iter()
                .find(|r| r.org_id.is_none() && key.is_some() && key_of(r.name.as_deref()) == key)
                .or_else(|| {
                    mine.iter()
                        .find(|r| r.org_id.is_none() && key_of(r.name.as_deref()).is_none())
                });
            let step = match target {
                Some(r) => LinkStep::Attach {
                    row_id: r.row_id,
                    renamed: r.name.as_deref() != Some(card.name.as_str()),
                },
                None => LinkStep::Add,
            };
            Some((*cid, step))
        })
        .collect();
    Ok(steps)
}

impl Store {
    /// 組織カードになっていない会社名を、正規化名ごとにまとめて人数の多い順に返す。
    ///
    /// カードにつながっていない会社名（`org_id` が無い）のうち、同じ正規化名のカードが
    /// まだ無いものが対象（カードがあるのにつながっていない人は
    /// [`Store::org_link_suggestions`] が拾う）。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn list_unlinked_org_names(&self) -> rusqlite::Result<Vec<UnlinkedOrgName>> {
        let conn = self.conn.lock().unwrap();
        let card_keys: HashSet<String> = cards(&conn)?
            .into_iter()
            .filter_map(|(_, name, _)| key_of(Some(&name)))
            .collect();
        let mut groups: BTreeMap<String, NameGroup> = BTreeMap::new();
        for row in org_rows(&conn)?.into_iter().filter(|r| r.org_id.is_none()) {
            let Some(name) = row.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) else {
                continue;
            };
            let Some(key) = key_of(Some(name)).filter(|k| !card_keys.contains(k)) else {
                continue;
            };
            let entry = groups.entry(key).or_default();
            entry
                .0
                .entry(name.to_string())
                .or_default()
                .insert(row.contact_id);
            entry.1.insert(row.contact_id);
        }
        let mut out: Vec<UnlinkedOrgName> = groups
            .into_values()
            .map(|(variants, people)| {
                let mut variants: Vec<(String, usize)> = variants
                    .into_iter()
                    .map(|(n, ids)| (n, ids.len()))
                    .collect();
                // 多い表記 → 長い表記 → 名前順。
                variants.sort_by(|a, b| {
                    b.1.cmp(&a.1)
                        .then_with(|| b.0.chars().count().cmp(&a.0.chars().count()))
                        .then_with(|| a.0.cmp(&b.0))
                });
                UnlinkedOrgName {
                    name: variants[0].0.clone(),
                    variants: variants.into_iter().map(|(n, _)| n).collect(),
                    contact_count: people.len() as i32,
                }
            })
            .collect();
        out.sort_by(|a, b| {
            b.contact_count
                .cmp(&a.contact_count)
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(out)
    }

    /// 会社名から組織カードを作り、同じ正規化名でカードにつながっていない人を全員つなぐ。
    ///
    /// 同じ正規化名のカードが既にあれば、作らずにそのカードへつなぐ。つながった人の会社名は
    /// カードの名前にそろえる。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき（全体を巻き戻す）。
    pub fn create_org_from_name(&self, name: &str) -> rusqlite::Result<OrganizationSummary> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let plan = create_plan(&tx, name)?;
        let org_id = match plan.existing {
            Some(id) => id,
            None => {
                tx.execute(
                    "INSERT INTO organizations (name) VALUES (?1)",
                    params![plan.card_name],
                )?;
                tx.last_insert_rowid()
            }
        };
        for (row_id, _) in &plan.rows {
            tx.execute(
                "UPDATE contact_organizations SET org_id = ?1 WHERE id = ?2",
                params![org_id, row_id],
            )?;
        }
        sync_member_names(&tx, org_id)?;
        tx.commit()?;
        load_org(&conn, org_id)
    }

    /// [`Store::create_org_from_name`] の下見: 会社名がカードの名前にそろい、次の同期で
    /// 送り直しになる人の数（確認欄で知らせるため）。何も書き換えない。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn create_org_from_name_impact(&self, name: &str) -> rusqlite::Result<OrgChangeImpact> {
        let conn = self.conn.lock().unwrap();
        let plan = create_plan(&conn, name)?;
        impact_of(&conn, plan.renamed)
    }

    /// 組織カードごとに「つながっていないが同じ組織らしい人」を理由つきで返す（候補のある
    /// カードだけ、候補の多い順）。
    ///
    /// 理由は 2 つ: 会社名が一致（正規化後。カードにつながっていない会社名だけを見る）、
    /// メールのドメインが一致（カードの代表メール、または既につながっている人のメールと同じ
    /// ドメイン。フリーメール・プロバイダ・官公庁は除く＝`is_unaffiliated_domain`）。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn org_link_suggestions(&self) -> rusqlite::Result<Vec<OrgLinkSuggestion>> {
        let conn = self.conn.lock().unwrap();
        let rows = org_rows(&conn)?;
        let domains = contact_domains(&conn)?;
        let summaries: HashMap<i64, _> =
            query_summaries(&conn, "c.deleted_at IS NULL", SUMMARY_ORDER, &[])?
                .into_iter()
                .map(|c| (c.id as i64, c))
                .collect();
        let mut out = Vec::new();
        for (org_id, card_name, card_email) in cards(&conn)? {
            let key = key_of(Some(&card_name));
            let members: HashSet<i64> = rows
                .iter()
                .filter(|r| r.org_id == Some(org_id))
                .map(|r| r.contact_id)
                .collect();
            let card_domains: HashSet<String> = card_email
                .as_deref()
                .and_then(domain_of)
                .filter(|d| !is_unaffiliated_domain(d))
                .into_iter()
                .chain(
                    members
                        .iter()
                        .filter_map(|m| domains.get(m))
                        .flatten()
                        .cloned(),
                )
                .collect();
            // contact_id → (一致した会社名, 一致したドメイン)
            let mut found: BTreeMap<i64, (Option<String>, Option<String>)> = BTreeMap::new();
            for r in rows
                .iter()
                .filter(|r| r.org_id.is_none() && !members.contains(&r.contact_id))
            {
                if key.is_some() && key_of(r.name.as_deref()) == key {
                    found.entry(r.contact_id).or_default().0 = r.name.clone();
                }
            }
            for (cid, ds) in domains.iter().filter(|(cid, _)| !members.contains(cid)) {
                if let Some(d) = ds.iter().find(|d| card_domains.contains(*d)) {
                    found.entry(*cid).or_default().1 = Some(d.clone());
                }
            }
            if found.is_empty() {
                continue;
            }
            let candidates = found
                .into_iter()
                .filter_map(|(cid, (matched_name, matched_domain))| {
                    Some(OrgLinkCandidate {
                        contact: summaries.get(&cid)?.clone(),
                        matched_name,
                        matched_domain,
                    })
                })
                .collect::<Vec<_>>();
            out.push(OrgLinkSuggestion {
                org: load_org(&conn, org_id)?,
                candidates,
            });
        }
        out.sort_by(|a, b| {
            b.candidates
                .len()
                .cmp(&a.candidates.len())
                .then_with(|| a.org.name.cmp(&b.org.name))
        });
        Ok(out)
    }

    /// 選んだ人を組織カードにつなぎ、つないだ後のカードを返す。
    ///
    /// その人の会社のうち、カードと正規化名が同じもの（無ければ名前の無いもの）をカードへ
    /// つなぐ。どちらも無ければ会社を 1 つ足す（会社が無い人は主の会社になる）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき（全体を巻き戻す）。
    pub fn link_contacts_to_org(
        &self,
        org_id: i64,
        contact_ids: &[i64],
    ) -> rusqlite::Result<OrganizationSummary> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let card = load_org(&tx, org_id)?;
        for (cid, step) in link_plan(&tx, &card, contact_ids)? {
            match step {
                LinkStep::Attach { row_id, .. } => {
                    tx.execute(
                        "UPDATE contact_organizations SET org_id = ?1 WHERE id = ?2",
                        params![org_id, row_id],
                    )?;
                }
                LinkStep::Add => {
                    tx.execute(
                        "INSERT INTO contact_organizations (contact_id, position, org_id, name) \
                         VALUES (?1, (SELECT coalesce(max(position) + 1, 0) \
                                      FROM contact_organizations WHERE contact_id = ?1), ?2, ?3)",
                        params![cid, org_id, card.name],
                    )?;
                    mark_dirty(&tx, cid)?;
                }
            }
        }
        sync_member_names(&tx, org_id)?;
        tx.commit()?;
        load_org(&conn, org_id)
    }

    /// [`Store::link_contacts_to_org`] の下見: 会社名が変わる・会社が足される人のうち、次の
    /// 同期で送り直しになる人の数。何も書き換えない。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき（カードが無いときを含む）。
    pub fn link_contacts_to_org_impact(
        &self,
        org_id: i64,
        contact_ids: &[i64],
    ) -> rusqlite::Result<OrgChangeImpact> {
        let conn = self.conn.lock().unwrap();
        let card = load_org(&conn, org_id)?;
        let changed = link_plan(&conn, &card, contact_ids)?
            .into_iter()
            .filter(|(_, step)| {
                matches!(step, LinkStep::Attach { renamed: true, .. } | LinkStep::Add)
            })
            .map(|(cid, _)| cid);
        impact_of(&conn, changed)
    }
}

#[cfg(test)]
mod tests;
