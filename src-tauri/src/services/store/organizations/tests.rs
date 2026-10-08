use super::*;
use crate::models::{ContactInput, ContactOrganization};
use crate::services::store::test_support::{employee, person};

fn store() -> Store {
    Store::open_in_memory_for_test()
}

fn card(s: &Store, name: &str) -> OrganizationSummary {
    s.upsert_organization(&OrganizationInput {
        name: name.into(),
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn upsert_links_organization_and_lists_with_counts() {
    let s = store();
    let a = s
        .upsert_contact(&employee("田中", "株式会社テスト"))
        .unwrap();
    let oid = a.fields.organizations[0].org_id;
    assert!(oid.is_some());
    // 同名の会社は同じカード（重複作成しない）。
    let b = s
        .upsert_contact(&employee("鈴木", "株式会社テスト"))
        .unwrap();
    assert_eq!(b.fields.organizations[0].org_id, oid);

    let orgs = s.list_organizations(None, false).unwrap();
    assert_eq!(orgs.len(), 1);
    assert_eq!(orgs[0].member_count, 2);

    // org_id 指定でもつながり、会社名はカードの名前にそろう。
    let mut c = person("佐藤", &[]);
    c.fields.organizations = vec![ContactOrganization {
        org_id: oid,
        ..Default::default()
    }];
    // 名前の無い会社は落ちるので、役職を入れておく。
    c.fields.organizations[0].title = Some("課長".into());
    let c = s.upsert_contact(&c).unwrap();
    assert_eq!(c.fields.organizations[0].org_id, oid);
    assert_eq!(
        c.fields.organizations[0].name.as_deref(),
        Some("株式会社テスト")
    );
}

#[test]
fn list_organizations_matches_similar_names() {
    let s = store();
    for n in ["sngDESIGN Inc.", "sngDESIGN浦添アトリエ", "全然別の会社"] {
        card(&s, n);
    }
    let r = s
        .list_organizations(Some("sngDESIGN浦添アトリエ"), false)
        .unwrap();
    let names: Vec<&str> = r.iter().map(|o| o.name.as_str()).collect();
    assert!(names.contains(&"sngDESIGN Inc."));
    assert!(!names.contains(&"全然別の会社"));
    assert_eq!(r[0].name, "sngDESIGN浦添アトリエ");
    assert_eq!(s.list_organizations(Some("sng"), false).unwrap().len(), 2);
    assert!(s.list_organizations(Some("xyz"), false).unwrap().is_empty());
}

#[test]
fn org_soft_delete_hides_and_revives_on_reuse() {
    let s = store();
    let oid = card(&s, "テスト社").id;
    assert!(s.delete_organization(oid as i64).unwrap());
    assert!(s.list_organizations(None, false).unwrap().is_empty());
    assert_eq!(s.list_organizations(None, true).unwrap().len(), 1);
    // 同名で連絡先を作ると、削除済みのカードが復活して再利用される。
    let c = s.upsert_contact(&employee("田中", "テスト社")).unwrap();
    assert_eq!(c.fields.organizations[0].org_id, Some(oid));
    assert_eq!(s.list_organizations(None, false).unwrap().len(), 1);
}

#[test]
fn org_card_fields_round_trip_and_survive_merge() {
    let s = store();
    let keep = s
        .upsert_organization(&OrganizationInput {
            name: "テスト社".into(),
            phone: Some("+81311112222".into()),
            ..Default::default()
        })
        .unwrap();
    let dropped = s
        .upsert_organization(&OrganizationInput {
            name: "(株)テスト".into(),
            phone: Some("+81399999999".into()),
            fax: Some("+81311113333".into()),
            email: Some("info@example.com".into()),
            address: OrgAddress {
                region: Some("沖縄県".into()),
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    let merged = s
        .merge_organizations(keep.id as i64, &[dropped.id as i64], "テスト社")
        .unwrap();
    assert_eq!(merged.phone.as_deref(), Some("+81311112222"));
    assert_eq!(merged.fax.as_deref(), Some("+81311113333"));
    assert_eq!(merged.email.as_deref(), Some("info@example.com"));
    assert_eq!(merged.address.region.as_deref(), Some("沖縄県"));
}

#[test]
fn renaming_a_card_renames_its_members_and_queues_them() {
    let s = store();
    let c = s.upsert_contact(&employee("田中", "テスト社")).unwrap();
    let oid = c.fields.organizations[0].org_id.unwrap();
    {
        let conn = s.conn.lock().unwrap();
        conn.execute("UPDATE contacts SET dirty = 0", []).unwrap();
    }
    // 代表電話だけ変えても、人は送り直さない。
    s.upsert_organization(&OrganizationInput {
        id: Some(oid),
        name: "テスト社".into(),
        phone: Some("+81311112222".into()),
        ..Default::default()
    })
    .unwrap();
    let dirty = |s: &Store| -> i64 {
        let conn = s.conn.lock().unwrap();
        conn.query_row("SELECT dirty FROM contacts", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(dirty(&s), 0);
    // 名前を変えると、つながっている人の会社名も変わり、送り直しの印が立つ。
    s.upsert_organization(&OrganizationInput {
        id: Some(oid),
        name: "テスト株式会社".into(),
        ..Default::default()
    })
    .unwrap();
    let after = s.get_contact(c.id as i64).unwrap();
    assert_eq!(
        after.primary_organization.as_deref(),
        Some("テスト株式会社")
    );
    assert_eq!(dirty(&s), 1);
}

#[test]
fn delete_organization_only_when_no_members() {
    let s = store();
    let a = s.upsert_contact(&employee("田中", "テスト社")).unwrap();
    let oid = a.fields.organizations[0].org_id.unwrap() as i64;
    assert!(!s.delete_organization(oid).unwrap());
    s.delete_contact(a.id as i64).unwrap();
    assert!(s.delete_organization(oid).unwrap());
}

#[test]
fn org_duplicates_grouped_by_normalized_name_and_merge_repoints() {
    let s = store();
    let keep = card(&s, "株式会社テスト");
    let drop_card = card(&s, "(株)テスト");
    let mut b: ContactInput = person("鈴木", &[]);
    b.fields.organizations = vec![ContactOrganization {
        org_id: Some(drop_card.id),
        ..Default::default()
    }];
    b.fields.organizations[0].title = Some("係長".into());
    let b = s.upsert_contact(&b).unwrap();
    assert_eq!(b.fields.organizations[0].org_id, Some(drop_card.id));

    let groups = s.find_organization_duplicates().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].organizations.len(), 2);

    let merged = s
        .merge_organizations(keep.id as i64, &[drop_card.id as i64], "株式会社テスト")
        .unwrap();
    assert_eq!(merged.member_count, 1);
    assert_eq!(s.list_organizations(None, false).unwrap().len(), 1);
    let bb = s.get_contact(b.id as i64).unwrap();
    assert_eq!(bb.fields.organizations[0].org_id, Some(keep.id));
    assert_eq!(
        bb.fields.organizations[0].name.as_deref(),
        Some("株式会社テスト")
    );
}

#[test]
fn detail_lists_members_and_shared_values() {
    let s = store();
    let mut a = employee("田中", "テスト社");
    a.fields.emails = vec![crate::models::ContactValue {
        label: Some("代表".into()),
        value: "info@test.co.jp".into(),
        is_shared: true,
    }];
    let a = s.upsert_contact(&a).unwrap();
    let oid = a.fields.organizations[0].org_id.unwrap() as i64;
    let d = s.organization_detail(oid).unwrap();
    assert_eq!(d.members.len(), 1);
    assert_eq!(d.shared_values.len(), 1);
    assert_eq!(d.shared_values[0].value, "info@test.co.jp");
}
