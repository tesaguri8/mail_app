use super::*;
use crate::services::store::test_support::{labeled, organization};
use crate::services::vcard;

fn store() -> Store {
    Store::open_in_memory_for_test()
}

/// vCard で取り込む（取り込みは組織を作らないので、会社名だけの連絡先がそのまま残る）。
fn import(s: &Store, vcf: &str) {
    s.import_contacts(&vcard::parse(vcf)).unwrap();
}

fn id_of(s: &Store, name: &str, kind: ContactKind) -> i64 {
    let all = s.list_contacts(None, &[], false).unwrap();
    let c = all
        .iter()
        .find(|c| c.fields.display_name == name && c.fields.kind == kind)
        .unwrap_or_else(|| panic!("{name} が無い"));
    i64::from(c.id)
}

/// Google につながっている印を付ける（Google へは送らない）。
fn link_google(s: &Store, id: i64, external: &str) {
    let conn = s.conn.lock().unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO google_accounts (id, email) VALUES (1, 'a@example.invalid')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO contact_identities (provider, account_id, external_id, contact_id) \
         VALUES ('google', 1, ?1, ?2)",
        params![external, id],
    )
    .unwrap();
}

fn kind_of(s: &Store, id: i64) -> ContactKind {
    s.get_contact(id).unwrap().fields.kind
}

#[test]
fn startup_merge_keeps_the_company_only_contact_and_takes_the_card_values() {
    let s = store();
    // 組織カードから作った組織（代表電話・メモ）と、同じ名前（表記ゆれ）の会社名だけの連絡先。
    let mut card = organization("株式会社見本", Some("+81311112222"), None);
    card.fields.note = Some("代表の窓口".into());
    let card = i64::from(s.upsert_contact(&card).unwrap().id);
    import(
        &s,
        "BEGIN:VCARD\nVERSION:3.0\nFN:(株)見本\nORG:(株)見本\nEND:VCARD\n\
         BEGIN:VCARD\nVERSION:3.0\nFN:試験 太郎\nN:試験;太郎;;;\nORG:株式会社見本\nEND:VCARD\n",
    );
    let only = id_of(&s, "(株)見本", ContactKind::Person);
    link_google(&s, only, "people/c1");
    let uid = s.get_contact(only).unwrap().uid;
    let taro = id_of(&s, "試験 太郎", ContactKind::Person);
    // 取り込みで組織へつながった人。
    assert_eq!(
        s.get_contact(taro).unwrap().fields.organizations[0].org_id,
        Some(card as i32)
    );

    let merged = merge_cards_with_company_only(&s.conn.lock().unwrap()).unwrap();
    assert_eq!(merged, 1);

    // 残るのは会社名だけの連絡先の側（uid と Google のつながりはそのまま）で、組織になる。
    let kept = s.get_contact(only).unwrap();
    assert_eq!(kept.fields.kind, ContactKind::Organization);
    assert_eq!(kept.uid, uid);
    assert_eq!(kept.links.len(), 1);
    assert!(kept.fields.phones.iter().any(|p| p.value == "+81311112222"));
    assert_eq!(kept.fields.note.as_deref(), Some("代表の窓口"));
    assert_eq!(kept.fields.organizations[0].org_id, None);
    assert!(s.get_contact(card).is_err(), "カードから作った側は消える");
    // 人のつながりは残る側へ付け替わる。
    assert_eq!(
        s.get_contact(taro).unwrap().fields.organizations[0].org_id,
        Some(only as i32)
    );
    // 2 回目は何もしない。
    assert_eq!(
        merge_cards_with_company_only(&s.conn.lock().unwrap()).unwrap(),
        0
    );
}

#[test]
fn startup_merge_takes_one_and_leaves_the_rest_as_candidates() {
    let s = store();
    let card = i64::from(
        s.upsert_contact(&organization("見本商店", None, None))
            .unwrap()
            .id,
    );
    import(
        &s,
        "BEGIN:VCARD\nVERSION:3.0\nFN:見本商店\nORG:見本商店\nTEL:0311112222\nEND:VCARD\n\
         BEGIN:VCARD\nVERSION:3.0\nFN:見本商店\nORG:見本商店\nTEL:0399998888\nEND:VCARD\n",
    );
    let ids: Vec<i64> = s
        .list_contacts(None, &[], false)
        .unwrap()
        .iter()
        .filter(|c| c.fields.kind == ContactKind::Person)
        .map(|c| i64::from(c.id))
        .collect();
    assert_eq!(ids.len(), 2);
    // つながりのある方が残る側になる。
    link_google(&s, ids[1], "people/c2");
    merge_cards_with_company_only(&s.conn.lock().unwrap()).unwrap();
    assert_eq!(kind_of(&s, ids[1]), ContactKind::Organization);
    assert_eq!(kind_of(&s, ids[0]), ContactKind::Person);
    assert!(s.get_contact(card).is_err());
    // 残りは整理の候補に出て、同じ名前の組織があることを示す。
    let cands = s.company_only_candidates().unwrap();
    assert_eq!(cands.len(), 1);
    assert_eq!(i64::from(cands[0].contact.id), ids[0]);
    assert_eq!(cands[0].existing_org, Some(ids[1] as i32));
}

#[test]
fn candidates_list_company_only_contacts_but_not_people_or_orgs() {
    let s = store();
    import(
        &s,
        "BEGIN:VCARD\nVERSION:3.0\nFN:sngDESIGN名護\nORG:sngDESIGN名護\nEND:VCARD\n\
         BEGIN:VCARD\nVERSION:3.0\nFN:sngDESIGN名護\nORG:sngDESIGN名護;設計部\nTITLE:所長\nEND:VCARD\n\
         BEGIN:VCARD\nVERSION:3.0\nFN:試験 太郎\nN:試験;太郎;;;\nORG:sngDESIGN名護\nEND:VCARD\n\
         BEGIN:VCARD\nVERSION:3.0\nFN:見本 花子\nORG:別の会社\nEND:VCARD\n",
    );
    s.upsert_contact(&organization("既にある組織", None, None))
        .unwrap();
    let cands = s.company_only_candidates().unwrap();
    // 会社名だけの 2 件（役職つきは印を付ける）。名前のある人・表示名が会社名でない人・組織は出ない。
    assert_eq!(cands.len(), 2, "{cands:?}");
    assert!(cands.iter().all(|c| c.company == "sngDESIGN名護"));
    assert_eq!(cands.iter().filter(|c| c.has_title).count(), 1);
    assert!(cands.iter().all(|c| c.same_company_people >= 1));
}

#[test]
fn convert_switches_in_place_and_links_people_with_the_same_company() {
    let s = store();
    import(
        &s,
        "BEGIN:VCARD\nVERSION:3.0\nFN:sngDESIGN名護\nORG:sngDESIGN名護\nTEL:0980000000\nEND:VCARD\n\
         BEGIN:VCARD\nVERSION:3.0\nFN:試験 太郎\nN:試験;太郎;;;\nORG:sngDESIGN 名護\nEND:VCARD\n",
    );
    let only = id_of(&s, "sngDESIGN名護", ContactKind::Person);
    link_google(&s, only, "people/c3");
    let uid = s.get_contact(only).unwrap().uid;
    let taro = id_of(&s, "試験 太郎", ContactKind::Person);
    {
        let conn = s.conn.lock().unwrap();
        conn.execute("UPDATE contacts SET dirty = 0", []).unwrap();
    }

    assert_eq!(s.convert_to_organizations(&[only, taro]).unwrap(), 1);

    let org = s.get_contact(only).unwrap();
    assert_eq!(org.fields.kind, ContactKind::Organization);
    assert_eq!(org.uid, uid, "消して作り直さない");
    assert_eq!(org.links.len(), 1, "Google のつながりは残る");
    assert_eq!(org.fields.phones[0].value, "0980000000");
    // 同じ会社名の人はつながり、会社名は組織の名前にそろう。
    let t = s.get_contact(taro).unwrap();
    assert_eq!(t.fields.organizations[0].org_id, Some(only as i32));
    assert_eq!(
        t.fields.organizations[0].name.as_deref(),
        Some("sngDESIGN名護")
    );
    // 人（姓名のある連絡先）は切り替えない。
    assert_eq!(t.fields.kind, ContactKind::Person);
    assert!(s.company_only_candidates().unwrap().is_empty());
}

#[test]
fn convert_merges_into_an_existing_org_with_the_same_name() {
    let s = store();
    let mut existing = organization("株式会社見本", None, None);
    existing.fields.phones = vec![labeled("代表", "+81311112222")];
    let existing = i64::from(s.upsert_contact(&existing).unwrap().id);
    import(
        &s,
        "BEGIN:VCARD\nVERSION:3.0\nFN:見本\nORG:見本\nEMAIL:info@example.invalid\nEND:VCARD\n",
    );
    let only = id_of(&s, "見本", ContactKind::Person);
    link_google(&s, only, "people/c4");

    assert_eq!(s.convert_to_organizations(&[only]).unwrap(), 1);
    // つながりのある会社名だけの連絡先が残り、既存の組織の値を受け取る。
    let org = s.get_contact(only).unwrap();
    assert_eq!(org.fields.kind, ContactKind::Organization);
    assert!(org.fields.phones.iter().any(|p| p.value == "+81311112222"));
    assert!(s.get_contact(existing).is_err());
}
