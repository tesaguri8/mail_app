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

/// Google アカウント 1 つと、そこにつながった連絡先を作る（`links` は外部 ID）。
fn google_person(s: &Store, account_id: i64, name: &str, links: &[&str]) -> i64 {
    let id = s.upsert_contact(&person(name, &[])).unwrap().id as i64;
    let conn = s.conn.lock().unwrap();
    links.iter().for_each(|ext| {
        conn.execute(
            "INSERT INTO contact_identities (provider, account_id, external_id, contact_id) \
             VALUES ('google', ?1, ?2, ?3)",
            params![account_id, ext, id],
        )
        .unwrap();
    });
    id
}

/// 外部 ID ごとの（つながっている連絡先, 削除待ちか）。
fn link_state(s: &Store, ext: &str) -> Option<(i64, bool)> {
    let conn = s.conn.lock().unwrap();
    conn.query_row(
        "SELECT contact_id, unlink_requested FROM contact_identities WHERE external_id = ?1",
        [ext],
        |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)),
    )
    .ok()
}

/// 同じアカウントに 2 ID: 残す側の ID を残し、消える側の ID は削除待ち。送信の計画は
/// 「残した 1 件を更新・余りを削除」になり、削除を送れたら残した 1 件だけがつながる。
#[test]
fn merging_two_ids_of_one_account_keeps_one_and_deletes_the_other() {
    let s = store();
    let a = s.upsert_google_account("a@gmail.com", None, None).unwrap();
    let keep = google_person(&s, a, "末松信吾", &["people/keep"]);
    let drop_id = google_person(&s, a, "末松 信吾", &["people/drop"]);

    let preview = s.merge_remote_preview(keep, &[drop_id]).unwrap();
    assert_eq!(preview.len(), 1);
    assert_eq!((preview[0].account_id as i64, preview[0].count), (a, 1));
    assert_eq!(preview[0].account_label, "a@gmail.com");

    s.merge_contacts(keep, &[drop_id]).unwrap();
    assert_eq!(link_state(&s, "people/keep"), Some((keep, false)));
    assert_eq!(link_state(&s, "people/drop"), Some((keep, true)));

    // 同じ人の更新と削除の順は問わない。
    let mut plan: Vec<(Option<String>, bool)> = s
        .list_contacts_to_push(a)
        .unwrap()
        .into_iter()
        .map(|p| (p.external_id, p.deleted))
        .collect();
    plan.sort();
    assert_eq!(
        plan,
        vec![
            (Some("people/drop".into()), true),
            (Some("people/keep".into()), false)
        ],
        "残した 1 件へ統合後の内容を送り、余りは削除する"
    );

    // 削除を送れた（送信側と同じ片付け）→ 残した 1 件だけがつながる。
    s.forget_contact_identity(a, "people/drop").unwrap();
    let links = s.get_contact(keep).unwrap().links;
    assert_eq!(links.len(), 1);
    assert_eq!(link_state(&s, "people/drop"), None);
}

/// 残す側に ID が無ければ、最初の 1 つ（古いつながり）を残す。
#[test]
fn merging_keeps_the_first_id_when_the_survivor_had_none() {
    let s = store();
    let a = s.upsert_google_account("a@gmail.com", None, None).unwrap();
    let keep = google_person(&s, a, "山田", &[]);
    let d1 = google_person(&s, a, "山田太郎", &["people/1"]);
    let d2 = google_person(&s, a, "山田 太郎", &["people/2"]);
    s.merge_contacts(keep, &[d1, d2]).unwrap();
    assert_eq!(link_state(&s, "people/1"), Some((keep, false)));
    assert_eq!(link_state(&s, "people/2"), Some((keep, true)));
}

/// 別のアカウント・片方だけ Google・解除中は、Google 側から何も消さない。
#[test]
fn merging_deletes_nothing_across_accounts_one_sided_or_disconnected() {
    let s = store();
    let a = s.upsert_google_account("a@gmail.com", None, None).unwrap();
    let b = s.upsert_google_account("b@gmail.com", None, None).unwrap();

    // 別アカウント。
    let keep = google_person(&s, a, "佐藤", &["people/a"]);
    let drop_id = google_person(&s, b, "佐藤 一", &["people/b"]);
    assert!(s.merge_remote_preview(keep, &[drop_id]).unwrap().is_empty());
    s.merge_contacts(keep, &[drop_id]).unwrap();
    assert_eq!(link_state(&s, "people/a"), Some((keep, false)));
    assert_eq!(link_state(&s, "people/b"), Some((keep, false)));

    // 片方だけ Google。
    let keep = google_person(&s, a, "鈴木", &[]);
    let drop_id = google_person(&s, a, "鈴木 花子", &["people/s"]);
    s.merge_contacts(keep, &[drop_id]).unwrap();
    assert_eq!(link_state(&s, "people/s"), Some((keep, false)));

    // 解除中のアカウントは送らない作法なので、削除待ちにしない（2 つともつながったまま）。
    let keep = google_person(&s, b, "高橋", &["people/t1"]);
    let drop_id = google_person(&s, b, "高橋 次郎", &["people/t2"]);
    s.disconnect_google_account(b).unwrap();
    assert!(s.merge_remote_preview(keep, &[drop_id]).unwrap().is_empty());
    s.merge_contacts(keep, &[drop_id]).unwrap();
    assert_eq!(link_state(&s, "people/t1"), Some((keep, false)));
    assert_eq!(link_state(&s, "people/t2"), Some((keep, false)));
}

/// この取り決めより前の統合で残った多重 ID を、同じ規則で片付ける。数えるだけでは何も変えない。
#[test]
fn tidying_existing_duplicate_ids_uses_the_same_rule() {
    let s = store();
    let a = s.upsert_google_account("a@gmail.com", None, None).unwrap();
    let b = s.upsert_google_account("b@gmail.com", None, None).unwrap();
    let p = google_person(&s, a, "伊藤", &["people/i1", "people/i2", "people/i3"]);
    // 別アカウントに 1 つずつは重複ではない。
    let q = google_person(&s, a, "加藤", &["people/k1"]);
    {
        let conn = s.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO contact_identities (provider, account_id, external_id, contact_id) \
             VALUES ('google', ?1, 'people/k2', ?2)",
            params![b, q],
        )
        .unwrap();
    }

    let found = s.duplicate_remote_ids().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].account_id as i64, found[0].count), (a, 2));
    assert_eq!(link_state(&s, "people/i2"), Some((p, false)), "数えるだけ");

    assert_eq!(s.tidy_duplicate_remote_ids().unwrap(), 2);
    assert_eq!(link_state(&s, "people/i1"), Some((p, false)));
    assert_eq!(link_state(&s, "people/i2"), Some((p, true)));
    assert_eq!(link_state(&s, "people/i3"), Some((p, true)));
    assert_eq!(link_state(&s, "people/k2"), Some((q, false)));
    // 片付いたらもう出ない。
    assert!(s.duplicate_remote_ids().unwrap().is_empty());
    assert_eq!(s.tidy_duplicate_remote_ids().unwrap(), 0);
}
