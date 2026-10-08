use super::*;
use crate::models::OrganizationInput;
use crate::services::store::test_support::{employee, person};
use crate::services::vcard;

fn store() -> Store {
    Store::open_in_memory_for_test()
}

/// 取り込み（カードを作らない道）で会社名だけを持つ連絡先を入れる。
fn import(s: &Store, cards: &[(&str, &str, &str)]) {
    let text: String = cards
        .iter()
        .map(|(name, org, email)| {
            format!("BEGIN:VCARD\nVERSION:3.0\nFN:{name}\nORG:{org}\nEMAIL:{email}\nEND:VCARD\n")
        })
        .collect();
    s.import_contacts(&vcard::parse(&text)).unwrap();
}

fn id_of(s: &Store, name: &str) -> i64 {
    s.list_contacts(None, &[], false)
        .unwrap()
        .into_iter()
        .find(|c| c.fields.display_name == name)
        .unwrap()
        .id as i64
}

#[test]
fn unlinked_names_are_grouped_by_normalized_name_and_counted() {
    let s = store();
    import(
        &s,
        &[
            ("A", "株式会社テスト", "a@test.co.jp"),
            ("B", "(株)テスト", "b@test.co.jp"),
            ("C", "株式会社テスト", "c@test.co.jp"),
            ("D", "別の会社", "d@other.co.jp"),
        ],
    );
    let names = s.list_unlinked_org_names().unwrap();
    assert_eq!(names.len(), 2);
    assert_eq!(names[0].name, "株式会社テスト", "多い表記が代表");
    assert_eq!(names[0].contact_count, 3);
    assert_eq!(
        names[0].variants,
        vec!["株式会社テスト".to_string(), "(株)テスト".to_string()]
    );
    assert_eq!(names[1].contact_count, 1);
}

#[test]
fn creating_a_card_from_a_name_links_everyone_with_that_name() {
    let s = store();
    import(
        &s,
        &[
            ("A", "株式会社テスト", "a@x.jp"),
            ("B", "(株)テスト", "b@y.jp"),
        ],
    );
    let card = s.create_org_from_name("株式会社テスト").unwrap();
    assert_eq!(card.member_count, 2);
    let b = s.get_contact(id_of(&s, "B")).unwrap();
    assert_eq!(b.fields.organizations[0].org_id, Some(card.id));
    assert_eq!(
        b.fields.organizations[0].name.as_deref(),
        Some("株式会社テスト")
    );
    // カードになった会社名は「カードになっていない会社名」から消える。
    assert!(s.list_unlinked_org_names().unwrap().is_empty());
}

#[test]
fn suggestions_find_people_by_company_name_and_email_domain() {
    let s = store();
    let card = s
        .upsert_organization(&OrganizationInput {
            name: "株式会社テスト".into(),
            email: Some("info@test.co.jp".into()),
            ..Default::default()
        })
        .unwrap();
    // 取り込みは正規化名が同じなら自動でつなぐので、名前の違う会社で入れる。
    import(
        &s,
        &[
            ("同じ名前", "テスト", "x@unrelated.jp"),
            ("同じドメイン", "テスト事業部", "y@test.co.jp"),
            ("フリーメール", "", "z@gmail.com"),
        ],
    );
    // 「同じ名前」は取り込みでつながってしまうので、つながりを外して候補にする。
    {
        let conn = s.conn.lock().unwrap();
        conn.execute("UPDATE contact_organizations SET org_id = NULL", [])
            .unwrap();
    }
    // 既につながっている人（gmail のメンバー）は、ドメインの手掛かりにならない。
    let mut member = employee("メンバー", "株式会社テスト");
    member.fields.emails = vec![crate::services::store::test_support::value("m@gmail.com")];
    s.upsert_contact(&member).unwrap();

    let sugg = s.org_link_suggestions().unwrap();
    assert_eq!(sugg.len(), 1);
    assert_eq!(sugg[0].org.id, card.id);
    let by_name = sugg[0]
        .candidates
        .iter()
        .find(|c| c.contact.fields.display_name == "同じ名前")
        .unwrap();
    // 取り込みでいったんつながったので、会社名はカードの名前にそろっている。
    assert_eq!(by_name.matched_name.as_deref(), Some("株式会社テスト"));
    let by_domain = sugg[0]
        .candidates
        .iter()
        .find(|c| c.contact.fields.display_name == "同じドメイン")
        .unwrap();
    assert_eq!(by_domain.matched_domain.as_deref(), Some("test.co.jp"));
    assert!(
        !sugg[0]
            .candidates
            .iter()
            .any(|c| c.contact.fields.display_name == "フリーメール"),
        "フリーメールのドメインでは候補にしない"
    );
}

#[test]
fn linking_people_reuses_a_matching_company_or_adds_one() {
    let s = store();
    let card = s
        .upsert_organization(&OrganizationInput {
            name: "株式会社テスト".into(),
            ..Default::default()
        })
        .unwrap();
    import(&s, &[("名前違い", "テスト事業部", "a@test.co.jp")]);
    let no_org = s
        .upsert_contact(&person("会社なし", &["b@test.co.jp"]))
        .unwrap()
        .id as i64;
    let other = id_of(&s, "名前違い");

    let linked = s
        .link_contacts_to_org(card.id as i64, &[other, no_org])
        .unwrap();
    assert_eq!(linked.member_count, 2);
    let o = s.get_contact(other).unwrap();
    assert_eq!(
        o.fields.organizations.len(),
        2,
        "別の会社名は残し、カードの会社を足す"
    );
    assert_eq!(o.fields.organizations[1].org_id, Some(card.id));
    let n = s.get_contact(no_org).unwrap();
    assert_eq!(n.fields.organizations[0].org_id, Some(card.id));
    assert_eq!(n.primary_organization.as_deref(), Some("株式会社テスト"));
}

/// 連絡先を Google につながっていることにする（送り直しの数え方の試験用）。
fn link_to_google(s: &Store, contact_id: i64) {
    let conn = s.conn.lock().unwrap();
    conn.execute(
        "INSERT INTO contact_identities (provider, account_id, external_id, contact_id) \
         VALUES ('google', 1, ?1, ?2)",
        rusqlite::params![format!("people/c{contact_id}"), contact_id],
    )
    .unwrap();
}

#[test]
fn provider_and_public_body_domains_are_not_a_hint() {
    let s = store();
    import(
        &s,
        &[("メンバー", "北部土木事務所", "a@pref.okinawa.lg.jp")],
    );
    import(&s, &[("会員", "株式会社ニライ", "b@nirai.ne.jp")]);
    s.create_org_from_name("北部土木事務所").unwrap();
    s.create_org_from_name("株式会社ニライ").unwrap();
    import(
        &s,
        &[
            ("県の別部署", "環境部", "c@pref.okinawa.lg.jp"),
            ("同じプロバイダ", "別の会社", "d@nirai.ne.jp"),
        ],
    );
    let sugg = s.org_link_suggestions().unwrap();
    assert!(
        sugg.is_empty(),
        "官公庁・プロバイダのドメインだけでは候補にしない: {:?}",
        sugg.iter().map(|g| &g.org.name).collect::<Vec<_>>()
    );
}

#[test]
fn impact_counts_only_synced_people_whose_company_changes() {
    let s = store();
    import(
        &s,
        &[
            ("そのまま", "株式会社テスト", "a@x.jp"),
            ("表記ゆれ", "(株)テスト", "b@y.jp"),
            ("つながりなし", "(株)テスト", "c@z.jp"),
        ],
    );
    link_to_google(&s, id_of(&s, "そのまま"));
    link_to_google(&s, id_of(&s, "表記ゆれ"));

    // カード名は「株式会社テスト」: 名前が変わるのは表記ゆれの 2 人、うち同期しているのは 1 人。
    let impact = s.create_org_from_name_impact("株式会社テスト").unwrap();
    assert_eq!(impact.resent, 1);
    // 下見は何も書き換えない。
    assert!(s.list_organizations(None, false).unwrap().is_empty());

    let card = s.create_org_from_name("株式会社テスト").unwrap();
    // つなぐ: 会社が足される人（同期あり）は送り直し、同期の無い人は数えない。
    import(&s, &[("足される人", "別の会社", "d@test.co.jp")]);
    let added = id_of(&s, "足される人");
    link_to_google(&s, added);
    let loose = s
        .upsert_contact(&person("会社なし", &["e@test.co.jp"]))
        .unwrap()
        .id as i64;
    let impact = s
        .link_contacts_to_org_impact(card.id as i64, &[added, loose])
        .unwrap();
    assert_eq!(impact.resent, 1);
    // 既につながっている人は数えない。
    let already = id_of(&s, "そのまま");
    let impact = s
        .link_contacts_to_org_impact(card.id as i64, &[already])
        .unwrap();
    assert_eq!(impact.resent, 0);
}
