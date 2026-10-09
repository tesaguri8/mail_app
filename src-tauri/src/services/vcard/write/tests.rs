use super::*;
use crate::models::{ContactCustomField, ContactDate, ContactRelation, ContactUrl, ContactValue};
use crate::services::vcard::parse;

fn value(label: Option<&str>, v: &str) -> ContactValue {
    ContactValue {
        label: label.map(str::to_string),
        value: v.into(),
        is_shared: false,
    }
}

/// 取り込みが扱う項目をひととおり持つ連絡先。
fn full() -> ContactFields {
    ContactFields {
        display_name: "山田 太郎".into(),
        name_prefix: Some("Dr.".into()),
        family_name: Some("山田".into()),
        middle_name: Some("一".into()),
        given_name: Some("太郎".into()),
        name_suffix: Some("Jr.".into()),
        phonetic_family: Some("ヤマダ".into()),
        phonetic_middle: Some("イチ".into()),
        phonetic_given: Some("タロウ".into()),
        nickname: Some("たろちゃん".into()),
        maiden_name: Some("佐藤".into()),
        birthday: Some("--12-25".into()),
        note: Some("1行目; セミコロン, カンマ\\バックスラッシュ\n2行目。長いメモは75オクテットで折り返す。長いメモは75オクテットで折り返す。".into()),
        show_as_company: true,
        organizations: vec![
            ContactOrganization {
                org_id: None,
                name: Some("株式会社テスト".into()),
                phonetic_name: Some("テスト".into()),
                title: Some("部長".into()),
                department: Some("営業部".into()),
            },
            ContactOrganization {
                org_id: None,
                name: Some("一般社団法人サンプル".into()),
                phonetic_name: None,
                title: Some("理事".into()),
                department: None,
            },
        ],
        emails: vec![
            value(Some("職場"), "taro@example.jp"),
            value(Some("実家"), "home@example.jp"),
            value(None, "plain@example.jp"),
        ],
        phones: vec![
            value(Some("携帯"), "090-1111-2222"),
            value(Some("代表"), "03-1234-5678"),
            value(Some("FAX"), "03-1234-5679"),
        ],
        addresses: vec![
            ContactAddress {
                label: Some("自宅".into()),
                po_box: None,
                postal: Some("060-0042".into()),
                region: Some("北海道".into()),
                city: Some("札幌市中央区".into()),
                street: Some("大通西3丁目".into()),
                extended: Some("テストビル 5F".into()),
                country: Some("日本".into()),
                country_code: Some("JP".into()),
            },
            ContactAddress {
                label: Some("別荘".into()),
                po_box: Some("私書箱 12".into()),
                city: Some("軽井沢町".into()),
                ..Default::default()
            },
        ],
        urls: vec![
            ContactUrl {
                label: Some("ホームページ".into()),
                value: "https://example.jp/".into(),
            },
            ContactUrl {
                label: None,
                value: "https://blog.example.jp/a,b".into(),
            },
        ],
        dates: vec![
            ContactDate {
                label: Some("記念日".into()),
                date: "2010-06-01".into(),
            },
            ContactDate {
                label: Some("入社日".into()),
                date: "--04-01".into(),
            },
        ],
        relations: vec![ContactRelation {
            label: Some("配偶者".into()),
            name: "山田 花子".into(),
        }],
        handles: vec![
            ContactHandle {
                kind: HandleKind::Im,
                service: Some("Skype".into()),
                value: "taro.yamada".into(),
                label: None,
            },
            ContactHandle {
                kind: HandleKind::Social,
                service: Some("twitter".into()),
                value: "https://twitter.com/taro".into(),
                label: Some("職場".into()),
            },
        ],
        custom_fields: vec![ContactCustomField {
            key: "社員番号".into(),
            value: "A-123".into(),
        }],
        tags: vec!["取引先".into(), "2026年".into()],
        ..Default::default()
    }
}

fn round_trip(version: VcardVersion) {
    let original = full();
    let text = generate(
        std::slice::from_ref(&original),
        version,
        "-//Tesaguri//Test//JA",
    );
    let back = parse(&text);
    assert_eq!(back.total_cards, 1);
    assert_eq!(back.contacts.len(), 1);
    assert_eq!(back.contacts[0], original, "\n{text}");
}

#[test]
fn round_trips_every_field_in_3_0() {
    round_trip(VcardVersion::V3);
}

#[test]
fn round_trips_every_field_in_4_0() {
    round_trip(VcardVersion::V4);
}

#[test]
fn lines_are_folded_within_75_octets_with_crlf() {
    let text = generate(&[full()], VcardVersion::V3, "-//Tesaguri//Test//JA");
    assert!(text.ends_with("END:VCARD\r\n"));
    assert!(
        !text.replace("\r\n", "").contains('\n'),
        "改行はすべて CRLF"
    );
    for line in text.split("\r\n") {
        assert!(line.len() <= 75, "長すぎる行: {line}");
    }
}

#[test]
fn labels_use_type_or_apple_form() {
    let text = generate(&[full()], VcardVersion::V3, "-//Tesaguri//Test//JA").replace("\r\n ", "");
    assert!(text.contains("TEL;TYPE=CELL,PREF:090-1111-2222"));
    assert!(text.contains(".X-ABLabel:_$!<Main>!$_"));
    assert!(text.contains("EMAIL;TYPE=INTERNET,WORK,PREF:taro@example.jp"));
    assert!(text.contains("BDAY;X-APPLE-OMIT-YEAR=1604:1604-12-25"));
    assert!(text.contains(".X-ABADR:jp"));

    let v4 = generate(&[full()], VcardVersion::V4, "-//Tesaguri//Test//JA").replace("\r\n ", "");
    assert!(v4.contains("VERSION:4.0"));
    assert!(v4.contains("TEL;TYPE=cell;PREF=1:090-1111-2222"));
    assert!(v4.contains("BDAY:--1225"));
    assert!(!v4.contains("INTERNET"));
}

#[test]
fn minimal_contact_round_trips() {
    let c = ContactFields {
        display_name: "info@example.jp".into(),
        emails: vec![value(None, "info@example.jp")],
        ..Default::default()
    };
    let back = parse(&generate(std::slice::from_ref(&c), VcardVersion::V3, "x"));
    assert_eq!(back.contacts, vec![c]);
}
