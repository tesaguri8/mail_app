//! Google の Person → 取り込み中間表現（`vcard::ImportedContact`）への変換。
//!
//! 中間表現は vCard / Google CSV の取り込みと共通のものを使う。同じ型に落としておけば、
//! 照合（`services::dedupe`）も保存（`store::contacts`）も取り込み元を問わず同じ道を通る。

use super::api::{GAddress, GPerson, GTypedValue};
use crate::services::vcard::{ImportedAddress, ImportedContact, ImportedValue};
use std::collections::HashMap;

/// People API の種別（`type`）を Rondine の見出しラベルへ。vCard 取り込みと同じ語彙に揃える。
/// 未知の種別は表示用の `formattedType`（ユーザーが付けたカスタム名を含む）をそのまま使う。
fn type_label(value_type: Option<&str>, formatted: Option<&str>) -> Option<String> {
    let label = match value_type.unwrap_or("") {
        "home" => "自宅",
        "work" => "職場",
        "mobile" => "携帯",
        "workFax" | "homeFax" | "otherFax" | "fax" => "FAX",
        "main" => "代表",
        // Google の既定値。付けても情報が増えないので無ラベルにする。
        "other" | "" => return formatted.and_then(non_empty).filter(|f| f != "その他"),
        _ => return formatted.and_then(non_empty).or_else(|| value_type.and_then(non_empty)),
    };
    Some(label.to_string())
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// メール・電話の共通変換。主値（primary）が先頭に来るよう並べ替える。
fn typed_values(src: &[GTypedValue]) -> Vec<ImportedValue> {
    let mut out: Vec<ImportedValue> = src
        .iter()
        .filter_map(|v| {
            let value = v.value.as_deref().and_then(non_empty)?;
            Some(ImportedValue {
                label: type_label(v.value_type.as_deref(), v.formatted_type.as_deref()),
                value,
                is_primary: v.metadata.primary,
            })
        })
        .collect();
    // 主値が無ければ先頭を主値にする（住所録は主値を一覧に出すため、必ず 1 つ立てる）。
    if !out.is_empty() && !out.iter().any(|v| v.is_primary) {
        out[0].is_primary = true;
    }
    out.sort_by_key(|v| !v.is_primary);
    out
}

fn addresses(src: &[GAddress]) -> Vec<ImportedAddress> {
    let mut out: Vec<ImportedAddress> = src
        .iter()
        .map(|a| ImportedAddress {
            label: type_label(a.value_type.as_deref(), a.formatted_type.as_deref()),
            postal: a.postal_code.as_deref().and_then(non_empty),
            region: a.region.as_deref().and_then(non_empty),
            city: a.city.as_deref().and_then(non_empty),
            street: a.street_address.as_deref().and_then(non_empty),
            extended: a.extended_address.as_deref().and_then(non_empty),
            country: a.country.as_deref().and_then(non_empty),
            is_primary: a.metadata.primary,
        })
        .filter(|a| {
            // 全項目が空の住所は捨てる。
            a.postal.is_some()
                || a.region.is_some()
                || a.city.is_some()
                || a.street.is_some()
                || a.extended.is_some()
                || a.country.is_some()
        })
        .collect();
    if !out.is_empty() && !out.iter().any(|a| a.is_primary) {
        out[0].is_primary = true;
    }
    out.sort_by_key(|a| !a.is_primary);
    out
}

/// 誕生日を住所録の表記（`YYYY-MM-DD`）へ。年が無い場合は vCard 4.0 と同じ `--MM-DD`。
fn birthday(p: &GPerson) -> Option<String> {
    let b = p.birthdays.first()?;
    if let Some(d) = b.date.as_ref() {
        if let (Some(m), Some(day)) = (d.month, d.day) {
            return Some(match d.year {
                Some(y) => format!("{y:04}-{m:02}-{day:02}"),
                None => format!("--{m:02}-{day:02}"),
            });
        }
    }
    b.text.as_deref().and_then(non_empty)
}

/// 主値（primary）の要素を優先し、無ければ先頭を返す。
fn primary_or_first<T>(items: &[T], is_primary: impl Fn(&T) -> bool) -> Option<&T> {
    items.iter().find(|i| is_primary(i)).or_else(|| items.first())
}

/// Person 1 件を中間表現へ。表示名も主メールも主電話も無いものは連絡先として成立しないので None。
///
/// `group_names` は連絡先グループ ID → 名前の対応（`api::list_contact_groups` 由来）。
/// 未知の ID は捨てる（system グループを除外した結果もここに含まれない）。
pub fn imported_from_person(
    p: &GPerson,
    group_names: &HashMap<String, String>,
) -> Option<ImportedContact> {
    let name = primary_or_first(&p.names, |n| n.metadata.primary);
    let org = primary_or_first(&p.organizations, |o| o.metadata.primary);

    let all_emails = typed_values(&p.email_addresses);
    let all_phones = typed_values(&p.phone_numbers);
    let all_addresses = addresses(&p.addresses);

    let family = name.and_then(|n| n.family_name.as_deref()).and_then(non_empty);
    let given = name.and_then(|n| n.given_name.as_deref()).and_then(non_empty);
    let kana_family = name
        .and_then(|n| n.phonetic_family_name.as_deref())
        .and_then(non_empty);
    let kana_given = name
        .and_then(|n| n.phonetic_given_name.as_deref())
        .and_then(non_empty);
    // vCard 取り込みと同じ組み立て方（よみ姓 + 空白 + よみ名）。
    let name_kana = match (kana_family.as_deref(), kana_given.as_deref()) {
        (Some(l), Some(f)) => Some(format!("{l} {f}")),
        (Some(l), None) => Some(l.to_string()),
        (None, Some(f)) => Some(f.to_string()),
        (None, None) => None,
    };

    let display_name = name
        .and_then(|n| n.display_name.as_deref())
        .and_then(non_empty)
        .or_else(|| match (family.as_deref(), given.as_deref()) {
            (Some(l), Some(f)) => Some(format!("{l} {f}")),
            (Some(l), None) => Some(l.to_string()),
            (None, Some(f)) => Some(f.to_string()),
            (None, None) => None,
        })
        // 名前が無い連絡先（会社の代表アドレスだけ等）は組織名／主値で代用する。
        .or_else(|| org.and_then(|o| o.name.as_deref()).and_then(non_empty))
        .or_else(|| all_emails.first().map(|e| e.value.clone()))
        .or_else(|| all_phones.first().map(|p| p.value.clone()))?;

    let labels = p
        .memberships
        .iter()
        .filter_map(|m| m.contact_group_membership.as_ref()?.contact_group_id.as_deref())
        .filter_map(|id| group_names.get(id).cloned())
        .collect();

    Some(ImportedContact {
        display_name,
        family_name: family,
        given_name: given,
        phonetic_family: kana_family,
        phonetic_given: kana_given,
        name_kana,
        email: all_emails.first().map(|e| e.value.clone()),
        phone: all_phones.first().map(|p| p.value.clone()),
        organization: org.and_then(|o| o.name.as_deref()).and_then(non_empty),
        org_title: org.and_then(|o| o.title.as_deref()).and_then(non_empty),
        org_department: org.and_then(|o| o.department.as_deref()).and_then(non_empty),
        address: all_addresses.first().map(crate::services::vcard::format_address),
        birthday: birthday(p),
        note: p
            .biographies
            .first()
            .and_then(|b| b.value.as_deref())
            .and_then(non_empty),
        all_emails,
        all_phones,
        all_addresses,
        labels,
        source: "google".to_string(),
        external_id: p.resource_name.as_deref().and_then(non_empty),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(json: &str) -> GPerson {
        serde_json::from_str(json).expect("Person の JSON を読めること")
    }

    fn groups() -> HashMap<String, String> {
        HashMap::from([("1a2b".to_string(), "取引先".to_string())])
    }

    #[test]
    fn converts_a_typical_japanese_contact() {
        let p = person(
            r#"{
              "resourceName": "people/c123",
              "etag": "%EgU...",
              "names": [{
                "displayName": "愛川 翼", "familyName": "愛川", "givenName": "翼",
                "phoneticFamilyName": "アイカワ", "phoneticGivenName": "ツバサ",
                "metadata": {"primary": true}
              }],
              "emailAddresses": [
                {"value": "second@example.com", "type": "home"},
                {"value": "rabbit@key.ocn.ne.jp", "type": "work", "metadata": {"primary": true}}
              ],
              "phoneNumbers": [
                {"value": "0997-52-4187", "type": "work", "metadata": {"primary": true}},
                {"value": "090-7929-9937", "type": "mobile"}
              ],
              "organizations": [{"name": "有限会社愛建工業", "title": "専務取締役",
                                 "department": "営業部", "metadata": {"primary": true}}],
              "birthdays": [{"date": {"year": 1987, "month": 10, "day": 6}}],
              "biographies": [{"value": "備考です"}],
              "memberships": [{"contactGroupMembership": {"contactGroupId": "1a2b"}}]
            }"#,
        );
        let c = imported_from_person(&p, &groups()).unwrap();

        assert_eq!(c.display_name, "愛川 翼");
        assert_eq!(c.name_kana.as_deref(), Some("アイカワ ツバサ"));
        assert_eq!(c.organization.as_deref(), Some("有限会社愛建工業"));
        assert_eq!(c.org_department.as_deref(), Some("営業部"));
        assert_eq!(c.birthday.as_deref(), Some("1987-10-06"));
        assert_eq!(c.note.as_deref(), Some("備考です"));
        assert_eq!(c.external_id.as_deref(), Some("people/c123"));
        assert_eq!(c.source, "google");
        assert_eq!(c.labels, vec!["取引先".to_string()]);

        // 主値が先頭に来て、flat な代表値と一致する。
        assert_eq!(c.email.as_deref(), Some("rabbit@key.ocn.ne.jp"));
        assert_eq!(c.all_emails[0].label.as_deref(), Some("職場"));
        assert_eq!(c.all_emails[1].value, "second@example.com");
        assert_eq!(c.phone.as_deref(), Some("0997-52-4187"));
        assert_eq!(c.all_phones[1].label.as_deref(), Some("携帯"));
    }

    #[test]
    fn primary_defaults_to_the_first_value() {
        let p = person(
            r#"{"resourceName":"people/c1",
                "names":[{"displayName":"名無し"}],
                "emailAddresses":[{"value":"a@example.com"},{"value":"b@example.com"}]}"#,
        );
        let c = imported_from_person(&p, &HashMap::new()).unwrap();
        assert!(c.all_emails[0].is_primary);
        assert_eq!(c.email.as_deref(), Some("a@example.com"));
    }

    #[test]
    fn falls_back_to_organization_then_email_for_the_display_name() {
        let p = person(
            r#"{"resourceName":"people/c2",
                "organizations":[{"name":"アークデータ研究所"}],
                "phoneNumbers":[{"value":"05037543196"}]}"#,
        );
        let c = imported_from_person(&p, &HashMap::new()).unwrap();
        assert_eq!(c.display_name, "アークデータ研究所");

        let p = person(r#"{"resourceName":"people/c3","emailAddresses":[{"value":"x@example.com"}]}"#);
        let c = imported_from_person(&p, &HashMap::new()).unwrap();
        assert_eq!(c.display_name, "x@example.com");
    }

    #[test]
    fn skips_a_person_with_nothing_usable() {
        let p = person(r#"{"resourceName":"people/c4"}"#);
        assert!(imported_from_person(&p, &HashMap::new()).is_none());
    }

    #[test]
    fn birthday_without_a_year_uses_the_vcard4_form() {
        let p = person(
            r#"{"resourceName":"people/c5","names":[{"displayName":"誕生日のみ"}],
                "birthdays":[{"date":{"month":3,"day":9}}]}"#,
        );
        let c = imported_from_person(&p, &HashMap::new()).unwrap();
        assert_eq!(c.birthday.as_deref(), Some("--03-09"));
    }

    #[test]
    fn other_type_produces_no_label_and_custom_type_is_kept() {
        let p = person(
            r#"{"resourceName":"people/c6","names":[{"displayName":"ラベル"}],
                "phoneNumbers":[
                  {"value":"111","type":"other","formattedType":"その他"},
                  {"value":"222","type":"実家","formattedType":"実家"}]}"#,
        );
        let c = imported_from_person(&p, &HashMap::new()).unwrap();
        assert_eq!(c.all_phones[0].label, None);
        assert_eq!(c.all_phones[1].label.as_deref(), Some("実家"));
    }
}
