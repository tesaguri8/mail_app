//! Google の Person ⇄ Rondine の連絡先の変換。
//!
//! 取り込み（Person → `vcard::ImportedContact`）は vCard / Google CSV と共通の中間表現へ落とす。
//! 同じ型に落としておけば、照合（`services::contact_match`）も保存（`store::contacts`）も
//! 取り込み元を問わず同じ道を通る。
//!
//! 送信（`ContactSummary` → People API の本文）は逆向き。**Rondine 固有の属性
//! （取引先フラグ・外部画像許可・組織レコードへのリンク）は送らない**。Google 側に対応概念が
//! 無く、往復で落ちるため（docs/CONTACTS_SYNC.md §3-4）。

use super::api::{GAddress, GPerson, GTypedValue};
use crate::models::ContactSummary;
use crate::services::vcard::{ImportedAddress, ImportedContact, ImportedValue};
use serde_json::{json, Value};
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

// ── 送信（Rondine → Google） ────────────────────────────────────────

/// Rondine の見出しラベル → People API の種別（`type`）。取り込み側 `type_label` の逆。
/// 語彙に無いユーザー独自のラベルは、そのままカスタム種別として送る。
fn write_type(label: Option<&str>) -> Option<String> {
    let l = label.map(str::trim).filter(|l| !l.is_empty())?;
    Some(
        match l {
            "自宅" => "home",
            "職場" => "work",
            "携帯" => "mobile",
            "FAX" => "otherFax",
            "代表" => "main",
            other => other,
        }
        .to_string(),
    )
}

/// メール／電話を送信用の配列にする。
///
/// - **共有指定の値は送らない**（会社の代表メール／代表電話。人ではなく組織のものなので）
/// - 主値を先頭に置く。People API は配列の先頭を主値として扱う
fn write_typed_values(values: &[crate::models::ContactValue], flat: Option<&str>) -> Vec<Value> {
    let mut kept: Vec<&crate::models::ContactValue> = values
        .iter()
        .filter(|v| !v.is_shared && !v.value.trim().is_empty())
        .collect();
    if kept.is_empty() {
        // 複数値を持たない連絡先は主値だけを送る。
        return flat
            .and_then(non_empty)
            .map(|v| vec![json!({ "value": v })])
            .unwrap_or_default();
    }
    kept.sort_by_key(|v| !v.is_primary);
    kept.iter()
        .map(|v| match write_type(v.label.as_deref()) {
            Some(t) => json!({ "value": v.value.trim(), "type": t }),
            None => json!({ "value": v.value.trim() }),
        })
        .collect()
}

fn write_addresses(addresses: &[crate::models::ContactAddress]) -> Vec<Value> {
    let mut kept: Vec<&crate::models::ContactAddress> = addresses.iter().collect();
    kept.sort_by_key(|a| !a.is_primary);
    kept.iter()
        .map(|a| {
            let mut o = serde_json::Map::new();
            let mut put = |k: &str, v: &Option<String>| {
                if let Some(v) = v.as_deref().and_then(non_empty) {
                    o.insert(k.to_string(), Value::String(v));
                }
            };
            put("postalCode", &a.postal);
            put("region", &a.region);
            put("city", &a.city);
            put("streetAddress", &a.street);
            put("extendedAddress", &a.extended);
            put("country", &a.country);
            if let Some(t) = write_type(a.label.as_deref()) {
                o.insert("type".to_string(), Value::String(t));
            }
            Value::Object(o)
        })
        .collect()
}

/// 住所録の誕生日表記（`YYYY-MM-DD` / 年なしの `--MM-DD`）を People API の形へ。
/// どちらでもない自由入力は `text` としてそのまま送る。
fn write_birthday(raw: &str) -> Option<Value> {
    let b = raw.trim();
    if b.is_empty() {
        return None;
    }
    let num = |s: &str| s.parse::<u32>().ok().filter(|n| *n > 0);
    if let Some(md) = b.strip_prefix("--") {
        // 年なし（vCard 4.0 と同じ表記）。
        if let Some((m, d)) = md.split_once('-') {
            if let (Some(m), Some(d)) = (num(m), num(d)) {
                return Some(json!({ "date": { "month": m, "day": d } }));
            }
        }
        return Some(json!({ "text": b }));
    }
    let parts: Vec<&str> = b.split('-').collect();
    if parts.len() == 3 {
        if let (Ok(y), Some(m), Some(d)) = (parts[0].parse::<i32>(), num(parts[1]), num(parts[2])) {
            return Some(json!({ "date": { "year": y, "month": m, "day": d } }));
        }
    }
    Some(json!({ "text": b }))
}

/// 連絡先 1 件を People API の書き込み本文にする。
///
/// `etag` は更新時に必須（読んだ版のものを渡す）。作成時は None。
///
/// `updatePersonFields`（[`super::WRITE_PERSON_FIELDS`]）に挙げた項目は**本文に無ければ
/// Google 側で消える**ので、空でもキー自体は必ず入れる（ローカルで消した項目が
/// Google 側にも反映されるように）。
pub fn person_write_from_contact(c: &ContactSummary, etag: Option<&str>) -> Value {
    // People API の displayName は読み取り専用。姓名に分けて送る必要がある。
    // Rondine 側が姓名を持たない（表示名だけの）連絡先は、表示名を姓に入れる
    // — 日本語の氏名は姓が先なので、Google 側の表示も元の並びのままになる。
    let mut name = serde_json::Map::new();
    let family = c.family_name.as_deref().and_then(non_empty);
    let given = c.given_name.as_deref().and_then(non_empty);
    match (family, given) {
        (None, None) => {
            if let Some(dn) = non_empty(&c.display_name) {
                name.insert("familyName".into(), Value::String(dn));
            }
        }
        (f, g) => {
            if let Some(f) = f {
                name.insert("familyName".into(), Value::String(f));
            }
            if let Some(g) = g {
                name.insert("givenName".into(), Value::String(g));
            }
        }
    }
    if let Some(v) = c.phonetic_family.as_deref().and_then(non_empty) {
        name.insert("phoneticFamilyName".into(), Value::String(v));
    }
    if let Some(v) = c.phonetic_given.as_deref().and_then(non_empty) {
        name.insert("phoneticGivenName".into(), Value::String(v));
    }

    let mut org = serde_json::Map::new();
    for (k, v) in [
        ("name", &c.organization),
        ("title", &c.org_title),
        ("department", &c.org_department),
    ] {
        if let Some(v) = v.as_deref().and_then(non_empty) {
            org.insert(k.to_string(), Value::String(v));
        }
    }

    let mut body = json!({
        "names": if name.is_empty() { vec![] } else { vec![Value::Object(name)] },
        "emailAddresses": write_typed_values(&c.emails, c.email.as_deref()),
        "phoneNumbers": write_typed_values(&c.phones, c.phone.as_deref()),
        "addresses": write_addresses(&c.addresses),
        "organizations": if org.is_empty() { vec![] } else { vec![Value::Object(org)] },
        "biographies": c.note.as_deref().and_then(non_empty)
            .map(|n| vec![json!({ "value": n, "contentType": "TEXT_PLAIN" })])
            .unwrap_or_default(),
        "birthdays": c.birthday.as_deref().and_then(write_birthday)
            .map(|b| vec![b]).unwrap_or_default(),
    });
    if let (Some(etag), Some(obj)) = (etag, body.as_object_mut()) {
        obj.insert("etag".to_string(), Value::String(etag.to_string()));
    }
    body
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

    // ── 送信（Rondine → Google） ──────────────────────────────────

    fn contact(name: &str) -> ContactSummary {
        ContactSummary {
            id: 1,
            display_name: name.into(),
            family_name: None,
            given_name: None,
            phonetic_family: None,
            phonetic_given: None,
            name_kana: None,
            email: None,
            phone: None,
            organization: None,
            org_id: None,
            org_title: None,
            org_department: None,
            address: None,
            birthday: None,
            note: None,
            is_favorite: false,
            is_business: false,
            allow_remote_images: false,
            deleted_at: None,
            emails: Vec::new(),
            phones: Vec::new(),
            addresses: Vec::new(),
            tags: Vec::new(),
        }
    }

    fn value(label: Option<&str>, v: &str, primary: bool, shared: bool) -> crate::models::ContactValue {
        crate::models::ContactValue {
            id: 0,
            label: label.map(str::to_string),
            value: v.into(),
            is_primary: primary,
            is_shared: shared,
        }
    }

    #[test]
    fn write_puts_a_display_only_name_into_the_family_name() {
        // People API の displayName は読み取り専用。姓名に分けて送るしかない。
        let b = person_write_from_contact(&contact("山田太郎"), None);
        assert_eq!(b["names"][0]["familyName"], "山田太郎");
        assert!(b["names"][0].get("givenName").is_none());
    }

    #[test]
    fn write_keeps_a_structured_name_as_is() {
        let mut c = contact("山田 太郎");
        c.family_name = Some("山田".into());
        c.given_name = Some("太郎".into());
        c.phonetic_family = Some("ヤマダ".into());
        c.phonetic_given = Some("タロウ".into());
        let b = person_write_from_contact(&c, None);
        assert_eq!(b["names"][0]["familyName"], "山田");
        assert_eq!(b["names"][0]["givenName"], "太郎");
        assert_eq!(b["names"][0]["phoneticFamilyName"], "ヤマダ");
        assert_eq!(b["names"][0]["phoneticGivenName"], "タロウ");
    }

    #[test]
    fn write_maps_labels_back_to_google_types() {
        let mut c = contact("山田太郎");
        c.phones = vec![
            value(Some("携帯"), "090-1111-2222", true, false),
            value(Some("FAX"), "03-1111-2222", false, false),
            value(Some("直通"), "03-3333-4444", false, false),
        ];
        let b = person_write_from_contact(&c, None);
        assert_eq!(b["phoneNumbers"][0]["type"], "mobile");
        assert_eq!(b["phoneNumbers"][1]["type"], "otherFax");
        // 語彙に無いラベルはカスタム種別としてそのまま送る。
        assert_eq!(b["phoneNumbers"][2]["type"], "直通");
    }

    #[test]
    fn write_puts_the_primary_value_first() {
        // People API は配列の先頭を主値として扱う。
        let mut c = contact("山田太郎");
        c.emails = vec![
            value(Some("職場"), "work@x.jp", false, false),
            value(Some("自宅"), "home@x.jp", true, false),
        ];
        let b = person_write_from_contact(&c, None);
        assert_eq!(b["emailAddresses"][0]["value"], "home@x.jp");
        assert_eq!(b["emailAddresses"][0]["type"], "home");
    }

    #[test]
    fn write_skips_values_shared_with_the_company() {
        // 代表メール・代表電話は人ではなく組織のもの。Google 側に対応概念が無いので送らない。
        let mut c = contact("山田太郎");
        c.emails = vec![
            value(None, "taro@x.jp", true, false),
            value(Some("代表"), "info@x.jp", false, true),
        ];
        let b = person_write_from_contact(&c, None);
        assert_eq!(b["emailAddresses"].as_array().unwrap().len(), 1);
        assert_eq!(b["emailAddresses"][0]["value"], "taro@x.jp");
    }

    #[test]
    fn write_falls_back_to_the_flat_value() {
        // 複数値を持たない連絡先（一覧から作った等）は主値だけを送る。
        let mut c = contact("山田太郎");
        c.email = Some("taro@x.jp".into());
        let b = person_write_from_contact(&c, None);
        assert_eq!(b["emailAddresses"][0]["value"], "taro@x.jp");
    }

    #[test]
    fn write_converts_the_three_birthday_shapes() {
        let mut c = contact("山田太郎");
        c.birthday = Some("1980-05-03".into());
        let b = person_write_from_contact(&c, None);
        assert_eq!(b["birthdays"][0]["date"]["year"], 1980);
        assert_eq!(b["birthdays"][0]["date"]["month"], 5);
        assert_eq!(b["birthdays"][0]["date"]["day"], 3);

        // 年なし（vCard 4.0 と同じ表記）。
        c.birthday = Some("--05-03".into());
        let b = person_write_from_contact(&c, None);
        assert!(b["birthdays"][0]["date"].get("year").is_none());
        assert_eq!(b["birthdays"][0]["date"]["month"], 5);

        // 日付として読めないものは原文のまま送る。
        c.birthday = Some("昭和55年ごろ".into());
        let b = person_write_from_contact(&c, None);
        assert_eq!(b["birthdays"][0]["text"], "昭和55年ごろ");
    }

    #[test]
    fn write_carries_the_etag_when_updating() {
        let b = person_write_from_contact(&contact("山田太郎"), Some("etag-1"));
        assert_eq!(b["etag"], "etag-1");
        // 作成時は etag を送らない（送ると弾かれる）。
        let b = person_write_from_contact(&contact("山田太郎"), None);
        assert!(b.get("etag").is_none());
    }

    #[test]
    fn write_sends_empty_arrays_so_cleared_fields_are_cleared() {
        // updatePersonFields に挙げた項目は「本文に無ければ消える」。ローカルで消した項目が
        // Google 側にも反映されるよう、空でもキーは必ず入れる。
        let b = person_write_from_contact(&contact("山田太郎"), None);
        for key in ["emailAddresses", "phoneNumbers", "addresses", "organizations", "biographies", "birthdays"] {
            assert_eq!(
                b[key].as_array().map(Vec::len),
                Some(0),
                "{key} は空配列で送る"
            );
        }
    }

    #[test]
    fn write_does_not_touch_labels() {
        // ラベル（memberships）の同期は後続の段。いま送ると Google 側のラベル分けを消す。
        assert!(!super::super::WRITE_PERSON_FIELDS.contains("memberships"));
        let b = person_write_from_contact(&contact("山田太郎"), None);
        assert!(b.get("memberships").is_none());
    }
}
