//! 連絡先の会社名・メール・電話から、似ている既存の組織カードを理由つきで探す
//! （連絡先の詳細の「組織に登録」の候補）。
//!
//! 物差しは組織タブの「整理」と同じもの（正規化名の一致・メールのドメインの一致）に、
//! 組織一覧の「似た名前」の検索と、電話の一致を足したもの。

use super::{card_domain_sets, cards, contact_domains, key_of, members_of, org_rows, OrgRow};
use crate::models::{OrgMatch, OrgMatchReason};
use crate::services::dedupe::digits;
use crate::services::store::greendomain::{domain_of, is_unaffiliated_domain};
use crate::services::store::organizations::load_org;
use crate::services::store::Store;
use rusqlite::Connection;
use std::collections::{BTreeMap, HashMap, HashSet};

/// 候補の上限（ドロップダウンに出す数）。
const MAX_MATCHES: usize = 8;

/// 電話の照合キー（E.164 の `+81…` は国内の `0…` へ寄せ、ほかは数字だけ）。
/// 番号として短すぎるもの（6 桁未満）は手掛かりにしない。
pub(crate) fn phone_match_key(raw: &str) -> Option<String> {
    let d = digits(raw);
    if d.len() < 6 {
        return None;
    }
    if raw.trim_start().starts_with('+') {
        return Some(match d.strip_prefix("81") {
            Some(rest) => format!("0{rest}"),
            None => format!("+{d}"),
        });
    }
    Some(d)
}

/// カードごとの電話の照合キー（カードの代表電話・FAX と、つながっている人の電話。
/// `exclude` の人の電話は数えない＝自分の番号で自分の会社に当たらないように）。
pub(crate) fn card_phone_sets(
    conn: &Connection,
    rows: &[OrgRow],
    exclude: Option<i64>,
) -> rusqlite::Result<HashMap<i64, HashSet<String>>> {
    let mut out: HashMap<i64, HashSet<String>> = HashMap::new();
    let mut stmt = conn
        .prepare("SELECT id, phone, fax FROM organizations WHERE deleted_at IS NULL ORDER BY id")?;
    let cards = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    for card in cards {
        let (org_id, phone, fax) = card?;
        let keys = [phone, fax]
            .into_iter()
            .flatten()
            .filter_map(|v| phone_match_key(&v));
        out.entry(org_id).or_default().extend(keys);
    }
    let mut stmt = conn.prepare(
        "SELECT cp.contact_id, cp.value FROM contact_phones cp \
         JOIN contacts c ON c.id = cp.contact_id WHERE c.deleted_at IS NULL",
    )?;
    let phones = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut by_contact: HashMap<i64, Vec<String>> = HashMap::new();
    for row in phones {
        let (cid, value) = row?;
        if Some(cid) == exclude {
            continue;
        }
        if let Some(key) = phone_match_key(&value) {
            by_contact.entry(cid).or_default().push(key);
        }
    }
    for (org_id, set) in out.iter_mut() {
        let member_keys = members_of(rows, *org_id)
            .into_iter()
            .filter_map(|m| by_contact.get(&m))
            .flatten()
            .cloned();
        set.extend(member_keys);
    }
    Ok(out)
}

/// カードごとの理由を、出す順（名前が同じ → 理由の多い順 → 似た名前の検索の順）に並べる。
fn rank(
    found: BTreeMap<i64, Vec<OrgMatchReason>>,
    similar_order: &[i64],
) -> Vec<(i64, Vec<OrgMatchReason>)> {
    let pos = |id: i64| {
        similar_order
            .iter()
            .position(|s| *s == id)
            .unwrap_or(usize::MAX)
    };
    let mut ranked: Vec<(i64, Vec<OrgMatchReason>)> = found.into_iter().collect();
    ranked.sort_by(|(a_id, a), (b_id, b)| {
        let same = |r: &[OrgMatchReason]| r.contains(&OrgMatchReason::SameName);
        same(b)
            .cmp(&same(a))
            .then_with(|| b.len().cmp(&a.len()))
            .then_with(|| pos(*a_id).cmp(&pos(*b_id)))
            .then_with(|| a_id.cmp(b_id))
    });
    ranked
}

impl Store {
    /// 会社名・メール・電話から、似ている既存の組織カードを理由つきで返す（多くて 8 件）。
    ///
    /// 理由は 4 つ: 会社名が同じ（正規化後）・会社名が似ている（[`Store::list_organizations`]
    /// の「似た名前」）・メールのドメインが同じ（フリーメール等は除く）・電話が同じ（カードの
    /// 代表電話・FAX、またはつながっている人の電話）。同じカードは 1 件にまとめて理由を並べる。
    /// `exclude_contact` はこの連絡先自身（その人の電話・メールでは当てない）。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn org_matches(
        &self,
        name: &str,
        emails: &[String],
        phones: &[String],
        exclude_contact: Option<i64>,
    ) -> rusqlite::Result<Vec<OrgMatch>> {
        let name = name.trim();
        // 似た名前の検索は自分でロックを取るので、先に済ませる。
        let similar: Vec<i64> = if name.is_empty() {
            Vec::new()
        } else {
            self.list_organizations(Some(name), false)?
                .into_iter()
                .map(|o| i64::from(o.id))
                .collect()
        };
        let conn = self.conn.lock().unwrap();
        let rows = org_rows(&conn)?;
        let mut domains = contact_domains(&conn)?;
        if let Some(cid) = exclude_contact {
            domains.remove(&cid);
        }
        let all_cards = cards(&conn)?;
        let domain_sets = card_domain_sets(&all_cards, &rows, &domains);
        let phone_sets = card_phone_sets(&conn, &rows, exclude_contact)?;
        let key = key_of(Some(name));
        let my_domains: Vec<String> = emails
            .iter()
            .filter_map(|e| domain_of(e))
            .filter(|d| !is_unaffiliated_domain(d))
            .collect();
        let my_phones: Vec<(&String, String)> = phones
            .iter()
            .filter_map(|p| phone_match_key(p).map(|k| (p, k)))
            .collect();

        let mut found: BTreeMap<i64, Vec<OrgMatchReason>> = BTreeMap::new();
        for (org_id, card_name, _) in &all_cards {
            let mut reasons = Vec::new();
            if key.is_some() && key_of(Some(card_name)) == key {
                reasons.push(OrgMatchReason::SameName);
            } else if similar.contains(org_id) {
                reasons.push(OrgMatchReason::SimilarName);
            }
            if let Some(d) = domain_sets
                .get(org_id)
                .and_then(|set| my_domains.iter().find(|d| set.contains(*d)))
            {
                reasons.push(OrgMatchReason::Domain { domain: d.clone() });
            }
            if let Some((raw, _)) = phone_sets
                .get(org_id)
                .and_then(|set| my_phones.iter().find(|(_, k)| set.contains(k)))
            {
                reasons.push(OrgMatchReason::Phone {
                    phone: (*raw).clone(),
                });
            }
            if !reasons.is_empty() {
                found.insert(*org_id, reasons);
            }
        }
        rank(found, &similar)
            .into_iter()
            .take(MAX_MATCHES)
            .map(|(org_id, reasons)| {
                Ok(OrgMatch {
                    org: load_org(&conn, org_id)?,
                    reasons,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::OrganizationInput;
    use crate::services::store::test_support::{employee, value};

    fn store() -> Store {
        Store::open_in_memory_for_test()
    }

    fn card(s: &Store, name: &str, phone: Option<&str>, email: Option<&str>) -> i64 {
        let o = s
            .upsert_organization(&OrganizationInput {
                name: name.into(),
                phone: phone.map(Into::into),
                email: email.map(Into::into),
                ..Default::default()
            })
            .unwrap();
        i64::from(o.id)
    }

    #[test]
    fn phone_key_folds_japanese_e164_into_national() {
        assert_eq!(
            phone_match_key("+81980510322").as_deref(),
            Some("0980510322")
        );
        assert_eq!(
            phone_match_key("0980-51-0322").as_deref(),
            Some("0980510322")
        );
        assert_eq!(
            phone_match_key("+14155550100").as_deref(),
            Some("+14155550100")
        );
        assert_eq!(
            phone_match_key("110"),
            None,
            "短すぎる番号は手掛かりにしない"
        );
    }

    #[test]
    fn matches_by_name_domain_and_phone_and_merges_reasons() {
        let s = store();
        let nago = card(&s, "株式会社sngDESIGN名護", Some("+81980510322"), None);
        let other = card(&s, "別の会社", None, Some("info@other.co.jp"));
        let _unrelated = card(&s, "関係ない会社", Some("+81312345678"), None);

        let got = s
            .org_matches(
                "sngDESIGN名護",
                &["me@other.co.jp".into(), "me@gmail.com".into()],
                &["+81980510322".into()],
                None,
            )
            .unwrap();
        let ids: Vec<i64> = got.iter().map(|m| i64::from(m.org.id)).collect();
        assert_eq!(
            ids,
            vec![nago, other],
            "名前が同じカードが先、関係ない会社は出ない"
        );
        assert_eq!(
            got[0].reasons,
            vec![
                OrgMatchReason::SameName,
                OrgMatchReason::Phone {
                    phone: "+81980510322".into()
                }
            ],
            "同じカードの理由は 1 件にまとめる"
        );
        assert_eq!(
            got[1].reasons,
            vec![OrgMatchReason::Domain {
                domain: "other.co.jp".into()
            }]
        );
    }

    #[test]
    fn matches_member_phones_but_not_the_contact_itself() {
        let s = store();
        let org = card(&s, "テスト商会", None, None);
        let mut member = employee("メンバー", "テスト商会");
        member.fields.phones = vec![value("+819011112222")];
        let member_id = s.upsert_contact(&member).unwrap().id as i64;
        // 取り込みでつながらなければ、つないでおく。
        s.link_contacts_to_org(org, &[member_id]).unwrap();

        let by_member = s
            .org_matches("", &[], &["090-1111-2222".into()], None)
            .unwrap();
        assert_eq!(by_member.len(), 1);
        assert_eq!(i64::from(by_member[0].org.id), org);

        let itself = s
            .org_matches("", &[], &["090-1111-2222".into()], Some(member_id))
            .unwrap();
        assert!(itself.is_empty(), "自分の電話では自分の会社に当てない");
    }

    #[test]
    fn similar_names_come_from_the_organization_search() {
        let s = store();
        let org = card(&s, "沖縄テスト工業", None, None);
        let got = s.org_matches("テスト工業", &[], &[], None).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(i64::from(got[0].org.id), org);
        assert_eq!(got[0].reasons, vec![OrgMatchReason::SimilarName]);
    }
}
