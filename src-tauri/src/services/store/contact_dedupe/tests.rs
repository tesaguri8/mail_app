use super::*;
use crate::models::ContactValue;
use crate::services::store::test_support::{employee, person, value};

fn store() -> Store {
    Store::open_in_memory_for_test()
}

#[test]
fn find_matches_by_email_phone_name_and_excludes_shared() {
    let s = store();
    let mut a = person("田中一郎", &["taro@a.jp"]);
    a.fields.phones = vec![ContactValue {
        label: Some("携帯".into()),
        value: "090-1111-2222".into(),
        is_shared: false,
    }];
    let a = s.upsert_contact(&a).unwrap().id as i64;
    let mut b = person("鈴木花子", &[]);
    b.fields.emails = vec![ContactValue {
        label: Some("代表".into()),
        value: "info@acme.co.jp".into(),
        is_shared: true,
    }];
    s.upsert_contact(&b).unwrap();

    let m = s
        .find_contact_matches(&["taro@a.jp".into()], &[], None, None)
        .unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].id as i64, a);
    assert_eq!(m[0].matched_emails, vec!["taro@a.jp".to_string()]);
    assert_eq!(m[0].email.as_deref(), Some("taro@a.jp"));

    // 共有指定のメールは手掛かりにしない。
    assert!(s
        .find_contact_matches(&["info@acme.co.jp".into()], &[], None, None)
        .unwrap()
        .is_empty());
    // 電話は数字正規化で一致（別表記でも当たる）。
    let mp = s
        .find_contact_matches(&[], &["+81 90 1111 2222".into()], None, None)
        .unwrap();
    assert_eq!(mp.len(), 1);
    assert_eq!(mp[0].id as i64, a);
    // 氏名は畳んで空白除去した完全一致。
    let mn = s
        .find_contact_matches(&[], &[], Some("田中 一郎"), None)
        .unwrap();
    assert_eq!(mn.len(), 1);
    assert!(mn[0].matched_name);
    // 自分自身は除外。
    assert!(s
        .find_contact_matches(&["taro@a.jp".into()], &[], None, Some(a))
        .unwrap()
        .is_empty());
}

#[test]
fn find_matches_auto_excludes_values_shared_by_many() {
    let s = store();
    let office_email = "gkki06@city.example.lg.jp";
    let office_phone = "0980-53-1234";
    for name in ["山城千香子", "五十嵐梨花", "伊波裕樹"] {
        let mut input = person(name, &[office_email]);
        input.fields.phones = vec![value(office_phone)];
        s.upsert_contact(&input).unwrap();
    }
    assert!(s
        .find_contact_matches(&[office_email.into()], &[], None, None)
        .unwrap()
        .is_empty());
    assert!(s
        .find_contact_matches(&[], &[office_phone.into()], None, None)
        .unwrap()
        .is_empty());
    let mn = s
        .find_contact_matches(&[office_email.into()], &[], Some("山城千香子"), None)
        .unwrap();
    assert_eq!(mn.len(), 1);
    assert!(mn[0].matched_name);
}

#[test]
fn duplicates_are_grouped_and_merge_unions_and_keeps_flags() {
    let s = store();
    let mut a = person("田中太郎", &["taro@a.jp"]);
    a.fields.phones = vec![value("090-1111")];
    a.fields.is_favorite = true;
    let id_a = s.upsert_contact(&a).unwrap().id as i64;
    let mut b = employee("田中太郎", "B社");
    b.fields.emails = vec![value("taro@b.jp")];
    b.fields.phonetic_family = Some("タナカ".into());
    b.fields.is_business = true;
    b.fields.tags = vec!["取引先".into()];
    let id_b = s.upsert_contact(&b).unwrap().id as i64;

    let groups = s.find_duplicate_groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].contacts.len(), 2);
    assert!(
        !groups[0].contacts[0].fields.emails.is_empty(),
        "候補は中身まで充填する"
    );

    let merged = s.merge_contacts(id_a, &[id_b]).unwrap();
    assert_eq!(s.list_contacts(None, &[], false).unwrap().len(), 1);
    assert!(merged.fields.is_favorite && merged.fields.is_business);
    assert_eq!(merged.fields.phonetic_family.as_deref(), Some("タナカ"));
    assert_eq!(merged.primary_organization.as_deref(), Some("B社"));
    assert_eq!(
        merged.primary_email.as_deref(),
        Some("taro@a.jp"),
        "残す側の主メール"
    );
    assert_eq!(merged.fields.emails.len(), 2);
    assert_eq!(merged.fields.tags, vec!["取引先".to_string()]);
}

#[test]
fn merging_moves_every_link_to_the_survivor() {
    let s = store();
    let keep = s
        .upsert_contact(&person("末松信吾", &["s@x.jp"]))
        .unwrap()
        .id as i64;
    let drop_id = s
        .upsert_contact(&person("末松 信吾", &["g@x.jp"]))
        .unwrap()
        .id as i64;
    {
        let conn = s.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO contact_identities (provider, account_id, external_id, contact_id) \
             VALUES ('google', 1, 'people/c1', ?1)",
            params![drop_id],
        )
        .unwrap();
    }
    let merged = s.merge_contacts(keep, &[drop_id]).unwrap();
    assert_eq!(merged.links.len(), 1, "消える側のつながりは残す側へ移る");
    let conn = s.conn.lock().unwrap();
    let (cid, dirty): (i64, i64) = conn
        .query_row(
            "SELECT contact_id, dirty FROM contact_identities WHERE external_id = 'people/c1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(cid, keep);
    assert_eq!(dirty, 1, "まとめた内容を送り直す");
}
