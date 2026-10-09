use super::*;
use crate::models::{
    ContactAddress, ContactCustomField, ContactDate, ContactHandle, ContactOrganization,
    ContactRelation, ContactUrl, ContactValue, HandleKind, OrganizationInput,
};
use crate::services::store::test_support::{employee, person, value};
use crate::services::vcard;

fn store() -> Store {
    Store::open_in_memory_for_test()
}

/// 全項目を埋めた入力。
fn full_input() -> ContactInput {
    ContactInput {
        id: None,
        fields: ContactFields {
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
            birthday: Some("--03-09".into()),
            note: Some("メモ".into()),
            show_as_company: true,
            is_favorite: true,
            is_business: true,
            allow_remote_images: true,
            organizations: vec![ContactOrganization {
                org_id: None,
                name: Some("株式会社テスト".into()),
                phonetic_name: Some("テスト".into()),
                title: Some("部長".into()),
                department: Some("営業部".into()),
            }],
            emails: vec![
                ContactValue {
                    label: Some("職場".into()),
                    value: "taro@x.jp".into(),
                    is_shared: false,
                },
                ContactValue {
                    label: Some("代表".into()),
                    value: "info@x.jp".into(),
                    is_shared: true,
                },
            ],
            phones: vec![value("090-1111-2222")],
            addresses: vec![ContactAddress {
                label: Some("自宅".into()),
                po_box: Some("私書箱1".into()),
                postal: Some("900-0001".into()),
                region: Some("沖縄県".into()),
                city: Some("那覇市".into()),
                street: Some("番地1".into()),
                extended: None,
                country: Some("日本".into()),
                country_code: Some("JP".into()),
            }],
            urls: vec![ContactUrl {
                label: Some("ホームページ".into()),
                value: "https://example.com".into(),
            }],
            dates: vec![ContactDate {
                label: Some("記念日".into()),
                date: "2010-06-01".into(),
            }],
            relations: vec![ContactRelation {
                label: Some("配偶者".into()),
                name: "山田花子".into(),
            }],
            handles: vec![ContactHandle {
                kind: HandleKind::Social,
                service: Some("Twitter".into()),
                value: "@taro".into(),
                label: None,
            }],
            custom_fields: vec![ContactCustomField {
                key: "社員番号".into(),
                value: "123".into(),
            }],
            tags: vec!["取引先".into()],
        },
    }
}

#[test]
fn every_field_round_trips_through_the_tables() {
    let s = store();
    let input = full_input();
    let saved = s.upsert_contact(&input).unwrap();
    let got = s.get_contact(saved.id as i64).unwrap();

    let mut expected = input.fields.clone();
    // 会社は組織カードにつながり、カードの ID を持つ（編集画面から入れた名前はカードを作る）。
    expected.organizations[0].org_id = got.fields.organizations[0].org_id;
    assert!(expected.organizations[0].org_id.is_some());
    assert_eq!(got.fields, expected);
    assert_eq!(got.sort_name.as_deref(), Some("ヤマダ イチ タロウ"));
    assert_eq!(got.primary_email.as_deref(), Some("taro@x.jp"));
    assert_eq!(got.primary_organization.as_deref(), Some("株式会社テスト"));
    assert!(got.links.is_empty(), "どのサービスにもつながっていない");

    // 一覧は主値の写しを持ち、子テーブルは空で返す。
    let listed = s.list_contacts(None, &[], false).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].primary_phone.as_deref(), Some("090-1111-2222"));
    assert!(listed[0].fields.emails.is_empty());

    // 連絡先タブの一覧は、一覧に出す分だけの軽い形で返す。
    let items = s.list_contact_items(None, &[], false).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, got.id);
    assert_eq!(items[0].display_name, got.fields.display_name);
    assert_eq!(items[0].primary_email.as_deref(), Some("taro@x.jp"));
    assert_eq!(
        items[0].primary_organization.as_deref(),
        Some("株式会社テスト")
    );
    assert!(items[0].deleted_at.is_none());
}

#[test]
fn saving_what_was_loaded_keeps_every_field() {
    // 画面が扱わない項目も、読み込んだ値をそのまま送れば保たれる。
    let s = store();
    let saved = s.upsert_contact(&full_input()).unwrap();
    let mut again = ContactInput {
        id: Some(saved.id),
        fields: saved.fields.clone(),
    };
    again.fields.note = Some("書き換えたメモ".into());
    let after = s.upsert_contact(&again).unwrap();
    assert_eq!(after.fields.urls, saved.fields.urls);
    assert_eq!(after.fields.middle_name, saved.fields.middle_name);
    assert_eq!(after.fields.handles, saved.fields.handles);
    assert_eq!(after.fields.note.as_deref(), Some("書き換えたメモ"));
}

#[test]
fn contact_tags_import_edit_and_filter() {
    let s = store();
    let p = vcard::parse(
        "BEGIN:VCARD\nVERSION:3.0\nFN:タグ 太郎\nCATEGORIES:施主,設計事務所\nEND:VCARD\n",
    );
    s.import_contacts(&p).unwrap();
    let id = s.list_contacts(None, &[], false).unwrap()[0].id as i64;
    let c = s.get_contact(id).unwrap();
    assert_eq!(
        c.fields.tags,
        vec!["施主".to_string(), "設計事務所".to_string()]
    );

    let mut input = ContactInput {
        id: Some(c.id),
        fields: c.fields.clone(),
    };
    input.fields.tags = vec!["VIP".to_string()];
    s.upsert_contact(&input).unwrap();
    assert_eq!(
        s.get_contact(id).unwrap().fields.tags,
        vec!["VIP".to_string()]
    );

    let tag_id: i64 = {
        let conn = s.conn.lock().unwrap();
        conn.query_row("SELECT id FROM tags WHERE name = 'VIP'", [], |r| r.get(0))
            .unwrap()
    };
    let filtered = s.list_contacts(None, &[tag_id], false).unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, c.id);
}

#[test]
fn import_dedups_by_email_and_preserves_user_flags() {
    let s = store();
    let first = vcard::parse(
        "BEGIN:VCARD\nVERSION:3.0\nFN:山田太郎\nEMAIL:taro@example.com\nORG:旧社名\nNOTE:メモ\nEND:VCARD\n",
    );
    let r1 = s.import_contacts(&first).unwrap();
    assert_eq!((r1.total, r1.imported, r1.updated), (1, 1, 0));

    // 利用者がお気に入り＆取引先に設定。
    let c = s.list_contacts(None, &[], false).unwrap().remove(0);
    let mut input = ContactInput {
        id: Some(c.id),
        fields: s.get_contact(c.id as i64).unwrap().fields,
    };
    input.fields.is_favorite = true;
    input.fields.is_business = true;
    s.upsert_contact(&input).unwrap();

    // 同じメールで再取り込み（会社名が変わり、電話が増えた）。
    let second = vcard::parse(
        "BEGIN:VCARD\nVERSION:3.0\nFN:山田太郎\nEMAIL:taro@example.com\nORG:新社名\nTEL:09011112222\nEND:VCARD\n",
    );
    let r2 = s.import_contacts(&second).unwrap();
    assert_eq!((r2.total, r2.imported, r2.updated), (1, 0, 1));

    let all = s.list_contacts(None, &[], false).unwrap();
    assert_eq!(all.len(), 1);
    let c = s.get_contact(all[0].id as i64).unwrap();
    assert!(c.fields.is_favorite && c.fields.is_business, "フラグは温存");
    assert_eq!(c.primary_organization.as_deref(), Some("新社名"));
    assert_eq!(c.primary_phone.as_deref(), Some("09011112222"));
    assert_eq!(
        c.fields.note.as_deref(),
        Some("メモ"),
        "入ってこなかった項目は残す"
    );
}

#[test]
fn shared_company_email_with_different_names_stays_separate() {
    let s = store();
    let p = vcard::parse(
        "BEGIN:VCARD\nVERSION:3.0\nFN:田中一郎\nEMAIL:info@acme.co.jp\nEND:VCARD\n\
         BEGIN:VCARD\nVERSION:3.0\nFN:鈴木花子\nEMAIL:info@acme.co.jp\nEND:VCARD\n",
    );
    let r = s.import_contacts(&p).unwrap();
    assert_eq!((r.imported, r.updated), (2, 0));
    assert_eq!(s.list_contacts(None, &[], false).unwrap().len(), 2);
}

#[test]
fn file_import_links_existing_cards_but_never_creates_them() {
    let s = store();
    let card = s
        .upsert_organization(&OrganizationInput {
            name: "株式会社テスト".into(),
            ..Default::default()
        })
        .unwrap();
    let p = vcard::parse(
        "BEGIN:VCARD\nVERSION:3.0\nFN:A\nORG:(株)テスト\nEND:VCARD\n\
         BEGIN:VCARD\nVERSION:3.0\nFN:B\nORG:取引の無い会社\nEND:VCARD\n",
    );
    s.import_contacts(&p).unwrap();
    let all = s.list_contacts(None, &[], false).unwrap();
    let a = s
        .get_contact(
            all.iter()
                .find(|c| c.fields.display_name == "A")
                .unwrap()
                .id as i64,
        )
        .unwrap();
    let b = s
        .get_contact(
            all.iter()
                .find(|c| c.fields.display_name == "B")
                .unwrap()
                .id as i64,
        )
        .unwrap();
    assert_eq!(
        a.fields.organizations[0].org_id,
        Some(card.id),
        "正規化名が一致すればつなぐ"
    );
    assert_eq!(
        a.fields.organizations[0].name.as_deref(),
        Some("株式会社テスト")
    );
    assert_eq!(b.fields.organizations[0].org_id, None, "カードは作らない");
    assert_eq!(s.list_organizations(None, false).unwrap().len(), 1);
}

#[test]
fn editor_creates_cards_only_for_names_the_user_typed() {
    let s = store();
    // 同期・取り込みで入った会社名（カード無し）を持つ連絡先。
    let p = vcard::parse("BEGIN:VCARD\nVERSION:3.0\nFN:A\nORG:同期で来た会社\nEND:VCARD\n");
    s.import_contacts(&p).unwrap();
    let id = s.list_contacts(None, &[], false).unwrap()[0].id;
    let loaded = s.get_contact(id as i64).unwrap();

    // 編集画面で保存しただけでは、その会社名からカードを作らない。
    let mut input = ContactInput {
        id: Some(id),
        fields: loaded.fields.clone(),
    };
    input.fields.note = Some("メモ".into());
    s.upsert_contact(&input).unwrap();
    assert!(s.list_organizations(None, false).unwrap().is_empty());

    // 新しく入れた会社名はカードを作ってつなぐ。
    input.fields.organizations.push(ContactOrganization {
        name: Some("新しく入れた会社".into()),
        ..Default::default()
    });
    let after = s.upsert_contact(&input).unwrap();
    assert_eq!(after.fields.organizations.len(), 2);
    assert_eq!(after.fields.organizations[0].org_id, None);
    assert!(after.fields.organizations[1].org_id.is_some());
    assert_eq!(s.list_organizations(None, false).unwrap().len(), 1);
}

#[test]
fn soft_delete_hides_restore_and_purge() {
    let s = store();
    let a = s
        .upsert_contact(&person("消える太郎", &["x@y.jp"]))
        .unwrap();
    s.delete_contact(a.id as i64).unwrap();
    assert!(s.list_contacts(None, &[], false).unwrap().is_empty());
    assert_eq!(s.list_contacts(None, &[], true).unwrap().len(), 1);
    assert!(s
        .find_contact_matches(&["x@y.jp".into()], &[], None, None)
        .unwrap()
        .is_empty());
    s.restore_contact(a.id as i64).unwrap();
    assert_eq!(s.list_contacts(None, &[], false).unwrap().len(), 1);
    s.delete_contact(a.id as i64).unwrap();
    s.purge_expired_trash(0).unwrap();
    assert!(s.list_contacts(None, &[], true).unwrap().is_empty());
}

#[test]
fn lookup_by_email_is_case_insensitive() {
    let s = store();
    s.upsert_contact(&person("山田", &["Taro@X.jp"])).unwrap();
    assert_eq!(s.lookup_contacts_by_email("taro@x.JP").unwrap().len(), 1);
    assert!(s.lookup_contacts_by_email("other@x.jp").unwrap().is_empty());
}

#[test]
fn search_matches_variant_kanji_typos_and_company() {
    let s = store();
    for name in ["銘刈太郎", "斎藤花子", "渡辺三郎"] {
        s.upsert_contact(&person(name, &[])).unwrap();
    }
    s.upsert_contact(&employee("会社員", "sngDESIGN")).unwrap();

    let hit = s.list_contacts(Some("銘苅"), &[], false).unwrap();
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].fields.display_name, "銘刈太郎");
    assert_eq!(s.list_contacts(Some("渡邊"), &[], false).unwrap().len(), 1);
    let fuzzy = s.list_contacts(Some("斉藤"), &[], false).unwrap();
    assert!(fuzzy.iter().any(|c| c.fields.display_name == "斎藤花子"));
    assert_eq!(
        s.list_contacts(Some("sngdesign"), &[], false)
            .unwrap()
            .len(),
        1,
        "主の会社名でも引ける"
    );
    assert!(s
        .list_contacts(Some("山田"), &[], false)
        .unwrap()
        .is_empty());
}

#[test]
fn relocate_moves_db_and_updates_path() {
    let root = std::env::temp_dir().join(format!("rondine_reloc_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let old_dir = root.join("old");
    let new_dir = root.join("new");
    let pointer = root.join(".data-location.txt");

    let s = Store::open(&old_dir.join("mail.db")).unwrap();
    let id = s
        .upsert_contact(&person("移転 太郎", &["a@b.jp"]))
        .unwrap()
        .id;
    s.relocate(&new_dir, &pointer).unwrap();

    assert_eq!(s.path(), new_dir.join("mail.db"));
    assert!(new_dir.join("mail.db").exists());
    assert!(!old_dir.join("mail.db").exists());
    assert_eq!(
        s.get_contact(id as i64).unwrap().fields.display_name,
        "移転 太郎"
    );
    assert_eq!(
        std::fs::read_to_string(&pointer).unwrap().trim(),
        new_dir.to_string_lossy()
    );
    s.upsert_contact(&person("追加 花子", &[])).unwrap();
    assert_eq!(s.list_contacts(None, &[], false).unwrap().len(), 2);
    drop(s);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn export_takes_everyone_or_the_given_ids_without_trash() {
    let s = store();
    s.upsert_contact(&person("書出 一郎", &[])).unwrap();
    let b = s.upsert_contact(&person("書出 二郎", &[])).unwrap();
    let gone = s.upsert_contact(&person("書出 削除", &[])).unwrap();
    s.delete_contact(gone.id as i64).unwrap();

    let names = |v: Vec<ContactFields>| v.into_iter().map(|c| c.display_name).collect::<Vec<_>>();
    let all = names(s.contacts_for_export(None).unwrap());
    assert_eq!(all.len(), 2, "ゴミ箱は書き出さない: {all:?}");
    let only_b = names(
        s.contacts_for_export(Some(&[b.id as i64, gone.id as i64]))
            .unwrap(),
    );
    assert_eq!(only_b, vec!["書出 二郎".to_string()]);
    assert!(s.contacts_for_export(Some(&[])).unwrap().is_empty());
}
