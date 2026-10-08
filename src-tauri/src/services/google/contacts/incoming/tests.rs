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
    let c = fields_from_person(&p, &groups()).unwrap();
    assert_eq!(c.display_name, "愛川 翼");
    assert_eq!(c.phonetic_given.as_deref(), Some("ツバサ"));
    assert_eq!(c.organizations[0].name.as_deref(), Some("有限会社愛建工業"));
    assert_eq!(c.organizations[0].department.as_deref(), Some("営業部"));
    assert_eq!(c.birthday.as_deref(), Some("1987-10-06"));
    assert_eq!(c.note.as_deref(), Some("備考です"));
    assert_eq!(c.tags, vec!["取引先".to_string()]);
    assert!(!c.is_favorite);
    // 主値が先頭に来る。
    assert_eq!(c.emails[0].value, "rabbit@key.ocn.ne.jp");
    assert_eq!(c.emails[0].label.as_deref(), Some("職場"));
    assert_eq!(c.emails[1].value, "second@example.com");
    assert_eq!(c.phones[1].label.as_deref(), Some("携帯"));
}

#[test]
fn reads_the_union_fields() {
    let p = person(
        r#"{
          "resourceName": "people/c9",
          "names": [{"displayName": "Dr. 山田 一 太郎 Jr.", "familyName": "山田",
                     "middleName": "一", "givenName": "太郎", "honorificPrefix": "Dr.",
                     "honorificSuffix": "Jr.", "phoneticMiddleName": "イチ"}],
          "nicknames": [{"value": "たろちゃん"}],
          "organizations": [{"name": "A 社", "phoneticName": "エーシャ"}, {"name": "B 社"}],
          "addresses": [{"poBox": "私書箱1", "city": "那覇市", "countryCode": "JP", "type": "home"}],
          "urls": [{"value": "https://example.com", "type": "homePage"}],
          "events": [{"date": {"month": 6, "day": 1}, "type": "anniversary"}],
          "relations": [{"person": "山田花子", "type": "spouse"}],
          "imClients": [{"username": "taro", "protocol": "skype", "formattedProtocol": "Skype"}],
          "userDefined": [{"key": "社員番号", "value": "123"}],
          "memberships": [{"contactGroupMembership": {"contactGroupId": "starred"}}]
        }"#,
    );
    let c = fields_from_person(&p, &HashMap::new()).unwrap();
    assert_eq!(c.name_prefix.as_deref(), Some("Dr."));
    assert_eq!(c.middle_name.as_deref(), Some("一"));
    assert_eq!(c.name_suffix.as_deref(), Some("Jr."));
    assert_eq!(c.phonetic_middle.as_deref(), Some("イチ"));
    assert_eq!(c.nickname.as_deref(), Some("たろちゃん"));
    assert_eq!(c.organizations.len(), 2, "会社は複数持てる");
    assert_eq!(
        c.organizations[0].phonetic_name.as_deref(),
        Some("エーシャ")
    );
    assert_eq!(c.addresses[0].po_box.as_deref(), Some("私書箱1"));
    assert_eq!(c.addresses[0].country_code.as_deref(), Some("JP"));
    assert_eq!(c.addresses[0].label.as_deref(), Some("自宅"));
    assert_eq!(c.urls[0].label.as_deref(), Some("ホームページ"));
    assert_eq!(c.dates[0].date, "--06-01");
    assert_eq!(c.dates[0].label.as_deref(), Some("記念日"));
    assert_eq!(c.relations[0].name, "山田花子");
    assert_eq!(c.relations[0].label.as_deref(), Some("配偶者"));
    assert_eq!(c.handles[0].service.as_deref(), Some("Skype"));
    assert_eq!(c.handles[0].value, "taro");
    assert_eq!(c.custom_fields[0].key, "社員番号");
    assert!(c.is_favorite, "スター付きはお気に入り");
    assert!(c.tags.is_empty(), "スターはタグにしない");
}

#[test]
fn falls_back_to_organization_then_email_for_the_display_name() {
    let p = person(
        r#"{"resourceName":"people/c2",
            "organizations":[{"name":"アークデータ研究所"}],
            "phoneNumbers":[{"value":"05037543196"}]}"#,
    );
    assert_eq!(
        fields_from_person(&p, &HashMap::new())
            .unwrap()
            .display_name,
        "アークデータ研究所"
    );
    let p = person(r#"{"resourceName":"people/c3","emailAddresses":[{"value":"x@example.com"}]}"#);
    assert_eq!(
        fields_from_person(&p, &HashMap::new())
            .unwrap()
            .display_name,
        "x@example.com"
    );
    let p = person(r#"{"resourceName":"people/c4"}"#);
    assert!(fields_from_person(&p, &HashMap::new()).is_none());
}

#[test]
fn other_type_produces_no_label_and_custom_type_is_kept() {
    let p = person(
        r#"{"resourceName":"people/c6","names":[{"displayName":"ラベル"}],
            "phoneNumbers":[
              {"value":"111","type":"other","formattedType":"その他"},
              {"value":"222","type":"実家","formattedType":"実家"}]}"#,
    );
    let c = fields_from_person(&p, &HashMap::new()).unwrap();
    assert_eq!(c.phones[0].label, None);
    assert_eq!(c.phones[1].label.as_deref(), Some("実家"));
}
