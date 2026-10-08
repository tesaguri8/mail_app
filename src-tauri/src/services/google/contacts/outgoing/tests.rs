use super::*;
use crate::models::{
    ContactCustomField, ContactHandle, ContactOrganization, ContactValue, HandleKind,
};

fn contact(name: &str) -> ContactFields {
    ContactFields {
        display_name: name.into(),
        ..Default::default()
    }
}

fn value(label: Option<&str>, v: &str, shared: bool) -> ContactValue {
    ContactValue {
        label: label.map(str::to_string),
        value: v.into(),
        is_shared: shared,
    }
}

#[test]
fn a_display_only_name_goes_into_the_family_name() {
    let b = person_body(&contact("山田太郎"), None);
    assert_eq!(b["names"][0]["familyName"], "山田太郎");
    assert!(b["names"][0].get("givenName").is_none());
}

#[test]
fn a_structured_name_is_sent_as_is() {
    let mut c = contact("山田 太郎");
    c.family_name = Some("山田".into());
    c.middle_name = Some("一".into());
    c.given_name = Some("太郎".into());
    c.name_prefix = Some("Dr.".into());
    c.phonetic_family = Some("ヤマダ".into());
    let b = person_body(&c, None);
    let n = &b["names"][0];
    assert_eq!(n["familyName"], "山田");
    assert_eq!(n["middleName"], "一");
    assert_eq!(n["givenName"], "太郎");
    assert_eq!(n["honorificPrefix"], "Dr.");
    assert_eq!(n["phoneticFamilyName"], "ヤマダ");
}

#[test]
fn labels_map_back_to_google_types_and_shared_values_are_sent() {
    let mut c = contact("山田太郎");
    c.phones = vec![
        value(Some("携帯"), "090-1111-2222", false),
        value(Some("FAX"), "03-1111-2222", false),
        value(Some("直通"), "03-3333-4444", false),
    ];
    c.emails = vec![
        value(None, "taro@x.jp", false),
        value(Some("代表"), "info@x.jp", true),
    ];
    let b = person_body(&c, None);
    assert_eq!(b["phoneNumbers"][0]["type"], "mobile");
    assert_eq!(b["phoneNumbers"][1]["type"], "otherFax");
    assert_eq!(b["phoneNumbers"][2]["type"], "直通");
    // 共有の印は Rondine 固有なので送らないが、値そのものは送る（送らないと Google 側で消え、
    // 次の取り込みで Rondine からも消えてしまう）。
    assert_eq!(b["emailAddresses"].as_array().unwrap().len(), 2);
    assert!(b["emailAddresses"][1].get("is_shared").is_none());
}

#[test]
fn every_written_field_is_present_even_when_empty() {
    let b = person_body(&contact("山田太郎"), None);
    for key in WRITE_FIELDS {
        assert!(b[*key].is_array(), "{key} は配列で送る");
    }
    for key in [
        "emailAddresses",
        "phoneNumbers",
        "addresses",
        "organizations",
        "biographies",
        "birthdays",
        "nicknames",
        "urls",
        "events",
        "relations",
        "imClients",
        "userDefined",
    ] {
        assert_eq!(
            b[key].as_array().map(Vec::len),
            Some(0),
            "{key} は空配列で送る"
        );
    }
    assert!(
        b.get("memberships").is_none(),
        "所属は members:modify で送る"
    );
    assert!(b.get("etag").is_none(), "作成時は etag を送らない");
}

/// `WRITE_PERSON_FIELDS` に挙げた項目。
const WRITE_FIELDS: &[&str] = &[
    "names",
    "nicknames",
    "emailAddresses",
    "phoneNumbers",
    "addresses",
    "organizations",
    "biographies",
    "birthdays",
    "urls",
    "events",
    "relations",
    "imClients",
    "userDefined",
];

#[test]
fn write_fields_constant_matches_the_body() {
    let listed: Vec<&str> = super::super::WRITE_PERSON_FIELDS.split(',').collect();
    assert_eq!(listed, WRITE_FIELDS);
}

#[test]
fn birthdays_and_dates_convert_to_google_dates() {
    let mut c = contact("山田太郎");
    c.birthday = Some("1980-05-03".into());
    c.dates = vec![crate::models::ContactDate {
        label: Some("記念日".into()),
        date: "--06-01".into(),
    }];
    let b = person_body(&c, None);
    assert_eq!(b["birthdays"][0]["date"]["year"], 1980);
    assert_eq!(b["events"][0]["date"]["month"], 6);
    assert!(b["events"][0]["date"].get("year").is_none());
    assert_eq!(b["events"][0]["type"], "anniversary");
    c.birthday = Some("昭和55年ごろ".into());
    let b = person_body(&c, None);
    assert_eq!(b["birthdays"][0]["text"], "昭和55年ごろ");
}

/// 読み直した Person を土台にすると、Rondine が知らない項目・付帯情報は消えない。
#[test]
fn unknown_parts_of_the_base_person_survive() {
    let base = serde_json::json!({
        "resourceName": "people/c1",
        "etag": "fresh-etag",
        "names": [{
            "displayName": "山田 太郎", "unstructuredName": "山田 太郎",
            "familyName": "山田", "givenName": "太郎",
            "metadata": {"primary": true, "source": {"type": "CONTACT", "id": "1"}}
        }],
        "emailAddresses": [
            {"value": "Taro@X.jp", "type": "work", "formattedType": "職場",
             "displayName": "山田（会社）", "metadata": {"primary": true}},
            {"value": "old@x.jp"}
        ],
        "organizations": [{
            "name": "(株)テスト", "title": "課長", "location": "本社 3F",
            "jobDescription": "設計", "metadata": {"primary": true}
        }],
        "nicknames": [{"value": "たろ"}, {"value": "やまちゃん", "type": "ALTERNATE_NAME"}],
        "userDefined": [{"key": "社員番号", "value": "1"}],
        "imClients": [{"username": "taro", "protocol": "skype", "formattedProtocol": "Skype"}]
    });
    let mut c = contact("山田 太郎");
    c.family_name = Some("山田".into());
    c.given_name = Some("太郎".into());
    c.emails = vec![value(Some("職場"), "taro@x.jp", false)];
    c.organizations = vec![ContactOrganization {
        name: Some("株式会社テスト".into()),
        title: Some("部長".into()),
        ..Default::default()
    }];
    c.nickname = Some("たろう".into());
    c.custom_fields = vec![ContactCustomField {
        key: "社員番号".into(),
        value: "2".into(),
    }];
    c.handles = vec![
        ContactHandle {
            kind: HandleKind::Im,
            service: Some("Skype".into()),
            value: "taro".into(),
            label: None,
        },
        ContactHandle {
            kind: HandleKind::Social,
            service: Some("Twitter".into()),
            value: "@taro".into(),
            label: None,
        },
    ];

    let b = person_body(&c, Some(&base));
    assert_eq!(b["etag"], "fresh-etag", "読み直した版の etag を使う");
    // 読み取り専用・派生のキーは送らない。
    let name = &b["names"][0];
    assert!(name.get("displayName").is_none());
    assert!(name.get("unstructuredName").is_none());
    assert!(name.get("metadata").is_none());
    // メール: 同じ値（大文字小文字違い）の要素を引き継ぎ、知らないキー（displayName）は残る。
    let emails = b["emailAddresses"].as_array().unwrap();
    assert_eq!(emails.len(), 1, "Rondine で消した値は送らない");
    assert_eq!(emails[0]["value"], "taro@x.jp");
    assert_eq!(emails[0]["displayName"], "山田（会社）");
    assert!(emails[0].get("formattedType").is_none());
    // 会社: 正規化名で同じ要素を引き継ぎ、Rondine が持たない location / jobDescription は残る。
    let org = &b["organizations"][0];
    assert_eq!(org["name"], "株式会社テスト");
    assert_eq!(org["title"], "部長");
    assert_eq!(org["location"], "本社 3F");
    assert_eq!(org["jobDescription"], "設計");
    // ニックネーム: 先頭だけ置き換え、2 つ目以降（Rondine は持たない）は残す。
    let nick = b["nicknames"].as_array().unwrap();
    assert_eq!(nick[0]["value"], "たろう");
    assert_eq!(nick[1]["value"], "やまちゃん");
    assert_eq!(b["userDefined"][0]["value"], "2");
    // チャットだけを送る（SNS のハンドルは Google に無い）。
    let im = b["imClients"].as_array().unwrap();
    assert_eq!(im.len(), 1);
    assert_eq!(im[0]["protocol"], "skype");
    assert!(im[0].get("formattedProtocol").is_none());
}

/// 1 つの値の項目で残す 2 つ目以降の要素からも、読み取り専用のキーを落として送る。
#[test]
fn the_rest_of_a_single_value_field_is_sent_without_read_only_keys() {
    let base = serde_json::json!({
        "nicknames": [
            {"value": "たろ", "metadata": {"primary": true}},
            {"value": "やまちゃん", "type": "ALTERNATE_NAME",
             "metadata": {"source": {"type": "CONTACT", "id": "1"}}}
        ],
        "biographies": [
            {"value": "メモ", "contentType": "TEXT_PLAIN"},
            {"value": "古いメモ", "metadata": {"source": {"type": "CONTACT"}}}
        ]
    });
    let mut c = contact("山田 太郎");
    c.nickname = Some("たろう".into());
    let b = person_body(&c, Some(&base));
    let nick = b["nicknames"].as_array().unwrap();
    assert_eq!(nick.len(), 2);
    assert_eq!(nick[1]["value"], "やまちゃん");
    assert_eq!(
        nick[1]["type"], "ALTERNATE_NAME",
        "Rondine が知らないキーは残す"
    );
    assert!(nick[1].get("metadata").is_none());
    // メモを消しても、2 つ目以降は残しつつ metadata は落とす。
    let bio = b["biographies"].as_array().unwrap();
    assert_eq!(bio.len(), 1);
    assert_eq!(bio[0]["value"], "古いメモ");
    assert!(bio[0].get("metadata").is_none());
}
