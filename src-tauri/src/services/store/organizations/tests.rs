use super::*;
use crate::models::ContactValue;
use crate::models::{ContactInput, ContactOrganization};
use crate::services::store::test_support::{employee, labeled, organization, person};

fn store() -> Store {
    Store::open_in_memory_for_test()
}

fn card(s: &Store, name: &str) -> OrganizationSummary {
    let id = s
        .upsert_contact(&organization(name, None, None))
        .unwrap()
        .id;
    s.get_organization(i64::from(id)).unwrap()
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
    // 組織は連絡先のゴミ箱へ。
    s.delete_contact(oid as i64).unwrap();
    assert!(s.list_organizations(None, false).unwrap().is_empty());
    assert_eq!(s.list_organizations(None, true).unwrap().len(), 1);
    // 同名で連絡先を作ると、削除済みのカードが復活して再利用される。
    let c = s.upsert_contact(&employee("田中", "テスト社")).unwrap();
    assert_eq!(c.fields.organizations[0].org_id, Some(oid));
    assert_eq!(s.list_organizations(None, false).unwrap().len(), 1);
}

#[test]
fn org_values_survive_merge() {
    let s = store();
    let keep = s
        .upsert_contact(&organization("テスト社", Some("+81311112222"), None))
        .unwrap();
    let mut dropped = organization("(株)テスト", Some("+81399999999"), Some("info@example.com"));
    dropped.fields.phones.push(labeled("FAX", "+81311113333"));
    dropped.fields.addresses = vec![crate::models::ContactAddress {
        region: Some("沖縄県".into()),
        ..Default::default()
    }];
    let dropped = s.upsert_contact(&dropped).unwrap();
    let merged = s
        .merge_organizations(keep.id as i64, &[dropped.id as i64], "テスト社")
        .unwrap();
    assert_eq!(merged.name, "テスト社");
    assert_eq!(merged.phone.as_deref(), Some("+81311112222"));
    assert_eq!(merged.fax.as_deref(), Some("+81311113333"));
    assert_eq!(merged.email.as_deref(), Some("info@example.com"));
    assert_eq!(merged.address.region.as_deref(), Some("沖縄県"));
    // 消えた側の電話も残す側に入る（和集合）。
    let all = s.get_contact(keep.id as i64).unwrap();
    assert!(all.fields.phones.iter().any(|p| p.value == "+81399999999"));
    assert!(s.get_organization(dropped.id as i64).is_err());
}

#[test]
fn renaming_an_org_renames_its_members_and_queues_them() {
    let s = store();
    let c = s.upsert_contact(&employee("田中", "テスト社")).unwrap();
    let oid = c.fields.organizations[0].org_id.unwrap();
    {
        let conn = s.conn.lock().unwrap();
        conn.execute("UPDATE contacts SET dirty = 0", []).unwrap();
    }
    let dirty = |s: &Store| -> i64 {
        let conn = s.conn.lock().unwrap();
        conn.query_row(
            "SELECT dirty FROM contacts WHERE id = ?1",
            params![c.id],
            |r| r.get(0),
        )
        .unwrap()
    };
    // 代表電話だけ変えても、人は送り直さない。
    let mut org = s.get_contact(i64::from(oid)).unwrap();
    org.fields.phones = vec![labeled("代表", "+81311112222")];
    let input = |f: &crate::models::ContactFields| ContactInput {
        id: Some(oid),
        fields: f.clone(),
    };
    s.upsert_contact(&input(&org.fields)).unwrap();
    assert_eq!(dirty(&s), 0);
    // 名前を変えると、つながっている人の会社名も変わり、送り直しの印が立つ。
    org.fields.organizations[0].name = Some("テスト株式会社".into());
    let renamed = s.upsert_contact(&input(&org.fields)).unwrap();
    assert_eq!(renamed.fields.display_name, "テスト株式会社");
    let after = s.get_contact(c.id as i64).unwrap();
    assert_eq!(
        after.primary_organization.as_deref(),
        Some("テスト株式会社")
    );
    assert_eq!(dirty(&s), 1);
}

#[test]
fn deleting_an_org_keeps_members_linked_until_purged() {
    let s = store();
    let a = s.upsert_contact(&employee("田中", "テスト社")).unwrap();
    let oid = a.fields.organizations[0].org_id.unwrap() as i64;
    // メンバーがいても消せる（ゴミ箱へ。要判断 F）。つながりは残る。
    s.delete_contact(oid).unwrap();
    let aa = s.get_contact(a.id as i64).unwrap();
    assert_eq!(aa.fields.organizations[0].org_id, Some(oid as i32));
    // 完全に消えると、会社名は文字として残る。
    {
        let conn = s.conn.lock().unwrap();
        conn.execute("DELETE FROM contacts WHERE id = ?1", params![oid])
            .unwrap();
    }
    let aa = s.get_contact(a.id as i64).unwrap();
    assert_eq!(aa.fields.organizations[0].org_id, None);
    assert_eq!(aa.fields.organizations[0].name.as_deref(), Some("テスト社"));
}

#[test]
fn org_self_row_never_links_and_people_cannot_link_to_people() {
    let s = store();
    // 同じ名前の組織がもう 1 件あっても、組織の自分の会社の行はどこにもつながない。
    let first = card(&s, "株式会社テスト");
    let second = s
        .upsert_contact(&organization("株式会社テスト", None, None))
        .unwrap();
    assert_eq!(second.fields.organizations[0].org_id, None);
    assert_ne!(second.id, first.id);
    // 人の id を org_id に入れても、人にはつながない（名前で組織を引く）。
    let p = s.upsert_contact(&person("佐藤", &[])).unwrap();
    let mut c = employee("鈴木", "株式会社テスト");
    c.fields.organizations[0].org_id = Some(p.id);
    let c = s.upsert_contact(&c).unwrap();
    assert_eq!(c.fields.organizations[0].org_id, Some(first.id));
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
    a.fields.emails = vec![ContactValue {
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
