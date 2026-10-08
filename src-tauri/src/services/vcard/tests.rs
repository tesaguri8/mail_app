use super::*;

#[test]
fn parses_icloud_company_card() {
    let vcf = "BEGIN:VCARD\nVERSION:3.0\nFN:\nN:;;;;\nPRODID:-//Apple Inc.//Mac OS X 10.12.6//EN\nORG:アークデータ研究所;\nNOTE:ASCAL\nTEL:05037543196\nX-ABShowAs:COMPANY\nEND:VCARD\n";
    let r = parse(vcf);
    assert_eq!(r.total_cards, 1);
    let c = &r.contacts[0];
    assert_eq!(c.display_name, "アークデータ研究所"); // FN/N 空 → ORG
    assert_eq!(
        c.organizations[0].name.as_deref(),
        Some("アークデータ研究所")
    );
    assert_eq!(c.phones[0].value, "05037543196");
    assert!(c.show_as_company);
}

#[test]
fn parses_name_kana_emails_pref_and_address() {
    let vcf = "BEGIN:VCARD\nVERSION:3.0\nN:愛川翼;;;;\nFN:愛川翼\nX-PHONETIC-LAST-NAME:アイカワ\nORG:有限会社愛建工業;\nTITLE:専務取締役\nEMAIL;type=INTERNET:second@example.com\nEMAIL;type=INTERNET;type=pref:rabbit@key.ocn.ne.jp\nTEL;type=pref:0997-52-4187\nTEL:090-7929-9937\nADR;type=pref:;;;;鹿児島県奄美市名瀬佐大熊町17-10AKビル2F;8940005;\nUID:ABC-123\nEND:VCARD\n";
    let c = &parse(vcf).contacts[0];
    assert_eq!(c.display_name, "愛川翼");
    assert_eq!(c.family_name.as_deref(), Some("愛川翼"));
    assert_eq!(c.phonetic_family.as_deref(), Some("アイカワ"));
    // pref を先頭（主）に並べる。
    assert_eq!(c.emails.len(), 2);
    assert_eq!(c.emails[0].value, "rabbit@key.ocn.ne.jp");
    assert_eq!(c.emails[1].value, "second@example.com");
    assert_eq!(c.phones[0].value, "0997-52-4187");
    assert_eq!(c.phones.len(), 2);
    assert_eq!(c.organizations[0].title.as_deref(), Some("専務取締役"));
    assert_eq!(c.addresses.len(), 1);
    assert_eq!(
        c.addresses[0].region.as_deref(),
        Some("鹿児島県奄美市名瀬佐大熊町17-10AKビル2F")
    );
    assert_eq!(c.addresses[0].postal.as_deref(), Some("8940005"));
}

#[test]
fn builds_display_from_n_and_keeps_kana_parts() {
    let vcf = "BEGIN:VCARD\nVERSION:3.0\nFN:\nN:石川;かおり;;;\nX-PHONETIC-LAST-NAME:イシカワ\nX-PHONETIC-FIRST-NAME:カオリ\nEMAIL:a@b.jp\nEMAIL:c@d.jp\nEND:VCARD\n";
    let c = &parse(vcf).contacts[0];
    assert_eq!(c.display_name, "石川かおり"); // CJK は詰める
    assert_eq!(c.phonetic_family.as_deref(), Some("イシカワ"));
    assert_eq!(c.phonetic_given.as_deref(), Some("カオリ"));
    assert_eq!(c.emails[0].value, "a@b.jp");
}

#[test]
fn unfolds_and_unescapes_note() {
    let vcf = "BEGIN:VCARD\nVERSION:3.0\nFN:x\nNOTE:first line\\nlong word continu\n es here\nEND:VCARD\n";
    let c = &parse(vcf).contacts[0];
    assert_eq!(
        c.note.as_deref(),
        Some("first line\nlong word continues here")
    );
}

#[test]
fn bday_strips_time_and_western_name_spaced() {
    let vcf = "BEGIN:VCARD\nVERSION:3.0\nN:Smith;John;;;\nBDAY;VALUE=date:1987-10-06\nEND:VCARD\n";
    let c = &parse(vcf).contacts[0];
    assert_eq!(c.display_name, "Smith John");
    assert_eq!(c.birthday.as_deref(), Some("1987-10-06"));
}

#[test]
fn card_without_any_identity_is_skipped() {
    let vcf = "BEGIN:VCARD\nVERSION:3.0\nFN:\nN:;;;;\nNOTE:x\nEND:VCARD\n";
    let r = parse(vcf);
    assert_eq!(r.total_cards, 1);
    assert_eq!(r.contacts.len(), 0);
}

#[test]
fn types_become_labels_and_categories_become_tags() {
    let vcf = "BEGIN:VCARD\nVERSION:3.0\nFN:タグ 太郎\nTEL;type=CELL:090-1111\nTEL;type=WORK:03-2222\nCATEGORIES:施主,設計事務所\nEND:VCARD\n";
    let c = &parse(vcf).contacts[0];
    assert_eq!(c.phones[0].label.as_deref(), Some("携帯"));
    assert_eq!(c.phones[1].label.as_deref(), Some("職場"));
    assert_eq!(c.tags, vec!["施主".to_string(), "設計事務所".to_string()]);
}

/// iCloud が書き出す項目（docs/CONTACT_MODEL.md §1 の iCloud 列）を読む。
#[test]
fn reads_the_icloud_columns() {
    let vcf = "BEGIN:VCARD\r\nVERSION:3.0\r\n\
N:山田;太郎;一;Dr.;Jr.\r\n\
FN:山田 太郎\r\n\
NICKNAME:たろちゃん\r\n\
X-MAIDENNAME:佐藤\r\n\
X-PHONETIC-LAST-NAME:ヤマダ\r\n\
X-PHONETIC-MIDDLE-NAME:イチ\r\n\
X-PHONETIC-FIRST-NAME:タロウ\r\n\
ORG:株式会社テスト;営業部\r\n\
X-PHONETIC-ORG:テスト\r\n\
TITLE:部長\r\n\
item1.EMAIL;type=INTERNET:taro@example.com\r\n\
item1.X-ABLabel:実家\r\n\
item2.ADR;type=HOME:私書箱1;;番地1;那覇市;沖縄県;9000001;日本\r\n\
item2.X-ABADR:jp\r\n\
item3.URL;type=pref:https://example.com\r\n\
item3.X-ABLabel:_$!<HomePage>!$_\r\n\
BDAY;X-APPLE-OMIT-YEAR=1604:1604-03-09\r\n\
item4.X-ABDATE:2010-06-01\r\n\
item4.X-ABLabel:_$!<Anniversary>!$_\r\n\
item5.X-ABRELATEDNAMES:山田花子\r\n\
item5.X-ABLabel:_$!<Spouse>!$_\r\n\
IMPP;X-SERVICE-TYPE=Skype;type=HOME;type=pref:skype:taro.yamada\r\n\
X-SOCIALPROFILE;type=twitter;x-user=taro:http://twitter.com/taro\r\n\
END:VCARD\r\n";
    let c = &parse(vcf).contacts[0];
    assert_eq!(c.family_name.as_deref(), Some("山田"));
    assert_eq!(c.given_name.as_deref(), Some("太郎"));
    assert_eq!(c.middle_name.as_deref(), Some("一"));
    assert_eq!(c.name_prefix.as_deref(), Some("Dr."));
    assert_eq!(c.name_suffix.as_deref(), Some("Jr."));
    assert_eq!(c.nickname.as_deref(), Some("たろちゃん"));
    assert_eq!(c.maiden_name.as_deref(), Some("佐藤"));
    assert_eq!(c.phonetic_middle.as_deref(), Some("イチ"));
    let org = &c.organizations[0];
    assert_eq!(org.name.as_deref(), Some("株式会社テスト"));
    assert_eq!(org.department.as_deref(), Some("営業部"));
    assert_eq!(org.phonetic_name.as_deref(), Some("テスト"));
    assert_eq!(org.title.as_deref(), Some("部長"));
    // X-ABLabel は同じグループの値の見出しになる。
    assert_eq!(c.emails[0].label.as_deref(), Some("実家"));
    let a = &c.addresses[0];
    assert_eq!(a.po_box.as_deref(), Some("私書箱1"));
    assert_eq!(a.country_code.as_deref(), Some("JP"));
    assert_eq!(a.label.as_deref(), Some("自宅"));
    assert_eq!(c.urls[0].label.as_deref(), Some("ホームページ"));
    // 年なしの誕生日は vCard 4.0 と同じ表記。
    assert_eq!(c.birthday.as_deref(), Some("--03-09"));
    assert_eq!(c.dates[0].date, "2010-06-01");
    assert_eq!(c.dates[0].label.as_deref(), Some("記念日"));
    assert_eq!(c.relations[0].name, "山田花子");
    assert_eq!(c.relations[0].label.as_deref(), Some("配偶者"));
    assert_eq!(c.handles.len(), 2);
    assert_eq!(c.handles[0].kind, HandleKind::Im);
    assert_eq!(c.handles[0].service.as_deref(), Some("Skype"));
    assert_eq!(c.handles[0].value, "taro.yamada");
    assert_eq!(c.handles[1].kind, HandleKind::Social);
    assert_eq!(c.handles[1].service.as_deref(), Some("twitter"));
    assert_eq!(c.handles[1].value, "taro");
}

#[test]
fn anniversary_property_gets_the_anniversary_label() {
    let vcf = "BEGIN:VCARD\nVERSION:4.0\nFN:x\nANNIVERSARY:20100601\nEND:VCARD\n";
    let c = &parse(vcf).contacts[0];
    assert_eq!(c.dates[0].date, "2010-06-01");
    assert_eq!(c.dates[0].label.as_deref(), Some("記念日"));
}
