use super::*;
use crate::models::{ContactInput, ContactOrganization, ContactValue, OrganizationInput};
use crate::services::store::test_support::{person, value};

fn mem_store() -> Store {
    Store::open_in_memory_for_test()
}

fn account(s: &Store) -> i64 {
    s.upsert_google_account("a@gmail.com", None, None).unwrap()
}

fn fields(name: &str, emails: &[&str]) -> ContactFields {
    ContactFields {
        display_name: name.into(),
        emails: emails.iter().map(|e| value(e)).collect(),
        ..Default::default()
    }
}

fn remote(id: &str, f: ContactFields) -> RemoteContact {
    RemoteContact {
        external_id: id.into(),
        etag: Some("e1".into()),
        deleted: false,
        contact: Some(f),
    }
}

fn deleted(id: &str) -> RemoteContact {
    RemoteContact {
        external_id: id.into(),
        etag: None,
        deleted: true,
        contact: None,
    }
}

/// 台帳に取り込み、照合して紐付けたローカル連絡先の ID を返す。
fn linked_contact(s: &Store, acct: i64, rid: &str, f: ContactFields) -> i64 {
    s.apply_remote_contact(acct, &remote(rid, f)).unwrap();
    s.apply_contact_matches(acct).unwrap();
    s.contact_identity(acct, rid)
        .unwrap()
        .unwrap()
        .contact_id
        .expect("照合で紐付いているはず")
}

fn edit(s: &Store, id: i64, change: impl FnOnce(&mut ContactFields)) {
    let mut f = s.get_contact(id).unwrap().fields;
    change(&mut f);
    s.upsert_contact(&ContactInput {
        id: Some(id as i32),
        fields: f,
    })
    .unwrap();
}

/// 本体の送信待ちの印。
fn contact_dirty(s: &Store, id: i64) -> bool {
    s.conn
        .lock()
        .unwrap()
        .query_row("SELECT dirty FROM contacts WHERE id = ?1", [id], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
        != 0
}

#[test]
fn upsert_keeps_the_row_unique_and_refreshes_etag() {
    let s = mem_store();
    let acct = account(&s);
    assert_eq!(
        s.apply_remote_contact(acct, &remote("people/c1", fields("山田太郎", &[])))
            .unwrap(),
        ApplyOutcome::Upserted
    );
    let mut again = remote("people/c1", fields("山田 太郎", &[]));
    again.etag = Some("e2".into());
    s.apply_remote_contact(acct, &again).unwrap();
    let got = s.contact_identity(acct, "people/c1").unwrap().unwrap();
    assert_eq!(got.contact_id, None);
    assert_eq!(got.etag.as_deref(), Some("e2"));
    assert_eq!(got.snapshot.unwrap().display_name, "山田 太郎");
    assert_eq!(s.count_unlinked_identities(acct).unwrap(), 1);
}

#[test]
fn identities_are_scoped_per_account_and_tokens_round_trip() {
    let s = mem_store();
    let a = account(&s);
    let b = s.upsert_google_account("b@gmail.com", None, None).unwrap();
    s.apply_remote_contact(a, &remote("people/c1", fields("A の連絡先", &[])))
        .unwrap();
    s.apply_remote_contact(b, &remote("people/c1", fields("B の連絡先", &[])))
        .unwrap();
    assert_eq!(s.count_unlinked_identities(a).unwrap(), 1);
    assert_eq!(s.count_unlinked_identities(b).unwrap(), 1);
    assert_eq!(s.contacts_sync_token(a).unwrap(), None);
    s.set_contacts_sync_token(a, Some("tok1")).unwrap();
    assert_eq!(s.contacts_sync_token(a).unwrap().as_deref(), Some("tok1"));
    s.set_contacts_sync_token(a, None).unwrap();
    assert_eq!(s.contacts_sync_token(a).unwrap(), None);
}

#[test]
fn matching_links_the_known_one_and_creates_the_rest() {
    let s = mem_store();
    let acct = account(&s);
    // Rondine にしか無い項目（旧姓・取引先）を持つ既存の連絡先。
    let mut mine = person("末松 信吾", &["s@x.jp"]);
    mine.fields.maiden_name = Some("旧姓".into());
    mine.fields.is_business = true;
    let known = s.upsert_contact(&mine).unwrap().id as i64;
    // 送信待ちを落としておく（Rondine で作った人は未連携の新規として送信待ちになっている）。
    s.conn
        .lock()
        .unwrap()
        .execute("UPDATE contacts SET dirty = 0 WHERE id = ?1", [known])
        .unwrap();
    let mut theirs = fields("末松信吾", &["s@x.jp"]);
    theirs.nickname = Some("しんご".into());
    s.apply_remote_contact(acct, &remote("people/c1", theirs))
        .unwrap();
    s.apply_remote_contact(acct, &remote("people/c2", fields("山田太郎", &["t@y.jp"])))
        .unwrap();

    let applied = s.apply_contact_matches(acct).unwrap();
    assert_eq!(
        (applied.linked, applied.created, applied.ambiguous),
        (1, 1, 0)
    );
    assert_eq!(
        s.contact_identity(acct, "people/c1")
            .unwrap()
            .unwrap()
            .contact_id,
        Some(known)
    );
    let created = s
        .contact_identity(acct, "people/c2")
        .unwrap()
        .unwrap()
        .contact_id
        .unwrap();
    // 同期のたびに自動で反映するので、反映が Google へ送る変更を増やさないこと:
    // 取り込みから起こした人は送信待ちにしない（Google から来たままなので送るものが無い）。
    assert!(
        !s.contact_identity(acct, "people/c2")
            .unwrap()
            .unwrap()
            .dirty
    );
    assert!(!contact_dirty(&s, created));
    // 既存へつないだ人も送信待ちにしない（確認なしに Google を書き換えない）。Google の内容は
    // 取り込み、Rondine にしか無い項目は残す（通常の取り込みと同じ規則）。
    assert!(
        !s.contact_identity(acct, "people/c1")
            .unwrap()
            .unwrap()
            .dirty
    );
    assert!(!contact_dirty(&s, known));
    let k = s.get_contact(known).unwrap().fields;
    assert_eq!(
        k.nickname.as_deref(),
        Some("しんご"),
        "Google の値を取り込む"
    );
    assert_eq!(
        k.maiden_name.as_deref(),
        Some("旧姓"),
        "Rondine にしか無い項目は残す"
    );
    assert!(k.is_business);
    assert_eq!(
        s.get_contact(created).unwrap().fields.display_name,
        "山田太郎"
    );
    assert_eq!(s.count_unlinked_identities(acct).unwrap(), 0);
    // 一覧でつながりが見える（アイコン用）。
    let c = s.get_contact(created).unwrap();
    assert_eq!(c.links.len(), 1);
    assert_eq!(c.links[0].account_email.as_deref(), Some("a@gmail.com"));
    // 照合の反映だけでは、Google へ送るものは何も増えない。
    assert!(s.list_contacts_to_push(acct).unwrap().is_empty());
}

#[test]
fn linking_does_not_overwrite_unsent_local_edits() {
    let s = mem_store();
    let acct = account(&s);
    let mut mine = person("末松 信吾", &["s@x.jp"]);
    mine.fields.nickname = Some("手元で直した".into());
    // Rondine で作った（または編集した）まま送っていない連絡先は送信待ちのまま。
    let known = s.upsert_contact(&mine).unwrap().id as i64;
    assert!(contact_dirty(&s, known));
    let mut theirs = fields("末松信吾", &["s@x.jp"]);
    theirs.nickname = Some("Google の値".into());
    s.apply_remote_contact(acct, &remote("people/c1", theirs))
        .unwrap();

    let applied = s.apply_contact_matches(acct).unwrap();
    assert_eq!(applied.linked, 1);
    // つなぐだけで、手元の未送信の変更は上書きしない（通常の取り込みと同じ）。
    assert_eq!(
        s.get_contact(known).unwrap().fields.nickname.as_deref(),
        Some("手元で直した")
    );
    assert!(
        !s.contact_identity(acct, "people/c1")
            .unwrap()
            .unwrap()
            .dirty,
        "つながりは送信待ちにしない"
    );
}

#[test]
fn applying_again_has_nothing_left_to_do() {
    let s = mem_store();
    let acct = account(&s);
    s.apply_remote_contact(acct, &remote("people/c1", fields("山田太郎", &["t@y.jp"])))
        .unwrap();
    s.apply_contact_matches(acct).unwrap();
    let again = s.apply_contact_matches(acct).unwrap();
    assert_eq!((again.linked, again.created, again.ambiguous), (0, 0, 0));
    assert_eq!(s.list_contacts(None, &[], false).unwrap().len(), 1);
}

#[test]
fn an_ambiguous_one_is_created_and_counted_for_review() {
    let s = mem_store();
    let acct = account(&s);
    s.upsert_contact(&person("山田太郎", &["a@x.jp"])).unwrap();
    s.apply_remote_contact(acct, &remote("people/c1", fields("山田太郎", &["b@y.jp"])))
        .unwrap();
    let r = s.apply_contact_matches(acct).unwrap();
    assert_eq!((r.linked, r.created, r.ambiguous), (0, 1, 1));
    let groups = s.find_duplicate_groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].contacts.len(), 2);
}

#[test]
fn editing_a_linked_contact_queues_it_for_push_and_pushing_clears_it() {
    let s = mem_store();
    let acct = account(&s);
    let id = linked_contact(&s, acct, "people/c1", fields("山田太郎", &["t@y.jp"]));
    assert!(s.list_contacts_to_push(acct).unwrap().is_empty());

    edit(&s, id, |f| f.display_name = "山田 太郎".into());
    let push = s.list_contacts_to_push(acct).unwrap();
    assert_eq!(push.len(), 1);
    assert_eq!(push[0].external_id.as_deref(), Some("people/c1"));
    assert!(!push[0].deleted);

    s.mark_contact_pushed(acct, id, "people/c1", Some("etag-new"), None)
        .unwrap();
    assert!(s.list_contacts_to_push(acct).unwrap().is_empty());
    let got = s.contact_identity(acct, "people/c1").unwrap().unwrap();
    assert_eq!(got.etag.as_deref(), Some("etag-new"));
    assert!(!got.dirty);
}

#[test]
fn an_edit_is_sent_to_every_linked_account() {
    let s = mem_store();
    let a = account(&s);
    let b = s.upsert_google_account("b@gmail.com", None, None).unwrap();
    let id = linked_contact(&s, a, "people/c1", fields("山田太郎", &["t@y.jp"]));
    // 同じ人を別のアカウントにもつなぐ。
    {
        let conn = s.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO contact_identities (provider, account_id, external_id, contact_id) \
             VALUES ('google', ?1, 'people/b1', ?2)",
            params![b, id],
        )
        .unwrap();
    }
    edit(&s, id, |f| f.note = Some("メモ".into()));
    assert_eq!(s.list_contacts_to_push(a).unwrap().len(), 1);
    assert_eq!(s.list_contacts_to_push(b).unwrap().len(), 1);

    // 片方へ送れても、もう片方へはまだ送る（未送信の印はつながりごと）。
    s.mark_contact_pushed(a, id, "people/c1", None, None)
        .unwrap();
    assert!(s.list_contacts_to_push(a).unwrap().is_empty());
    assert_eq!(s.list_contacts_to_push(b).unwrap().len(), 1);
    // 未送信が残っている間は、取り込みで上書きしない。
    let mut google = fields("Google 側の名前", &["t@y.jp"]);
    google.note = None;
    s.apply_remote_contact(a, &remote("people/c1", google))
        .unwrap();
    assert_eq!(
        s.get_contact(id).unwrap().fields.note.as_deref(),
        Some("メモ")
    );

    s.mark_contact_pushed(b, id, "people/b1", None, None)
        .unwrap();
    let dirty: i64 = {
        let conn = s.conn.lock().unwrap();
        conn.query_row(
            "SELECT dirty FROM contacts WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(dirty, 0, "全部送れたら本体の印も落ちる");
}

#[test]
fn deleting_a_linked_contact_queues_a_remote_delete_and_forgets_the_link() {
    let s = mem_store();
    let acct = account(&s);
    let id = linked_contact(&s, acct, "people/c1", fields("山田太郎", &["t@y.jp"]));
    s.delete_contact(id).unwrap();
    let push = s.list_contacts_to_push(acct).unwrap();
    assert_eq!(push.len(), 1);
    assert!(push[0].deleted);
    s.forget_contact_identity(acct, "people/c1").unwrap();
    assert!(s.contact_identity(acct, "people/c1").unwrap().is_none());
    assert!(s.list_contacts_to_push(acct).unwrap().is_empty());
}

#[test]
fn locally_born_contacts_are_pushed_only_when_enabled() {
    let s = mem_store();
    let acct = account(&s);
    s.upsert_contact(&person("手元で作った人", &["local@x.jp"]))
        .unwrap();
    assert!(!s.push_new_contacts(acct).unwrap());
    assert!(s.list_contacts_to_push(acct).unwrap().is_empty());
    s.set_push_new_contacts(acct, true).unwrap();
    let push = s.list_contacts_to_push(acct).unwrap();
    assert_eq!(push.len(), 1);
    assert_eq!(push[0].external_id, None);
    assert_eq!(push[0].contact.display_name, "手元で作った人");
}

#[test]
fn pulling_refreshes_google_fields_and_keeps_local_marks() {
    let s = mem_store();
    let acct = account(&s);
    let card = s
        .upsert_organization(&OrganizationInput {
            name: "株式会社ヤマダ".into(),
            ..Default::default()
        })
        .unwrap();
    let id = linked_contact(
        &s,
        acct,
        "people/c1",
        fields("山田太郎", &["t@y.jp", "info@y.jp"]),
    );
    // Rondine 固有の印（共有の印・取引先）を付けて送り終えた状態にする。
    edit(&s, id, |f| {
        f.emails[1].is_shared = true;
        f.is_business = true;
        f.maiden_name = Some("佐藤".into());
    });
    s.mark_contact_pushed(acct, id, "people/c1", None, None)
        .unwrap();

    let mut updated = fields("山田太郎", &["t@y.jp", "info@y.jp"]);
    updated.organizations = vec![ContactOrganization {
        name: Some("(株)ヤマダ".into()),
        ..Default::default()
    }];
    updated.is_favorite = true;
    s.apply_remote_contact(acct, &remote("people/c1", updated))
        .unwrap();

    let c = s.get_contact(id).unwrap();
    assert_eq!(
        c.fields.organizations[0].org_id,
        Some(card.id),
        "既存のカードにだけつなぐ"
    );
    assert!(c.fields.is_favorite, "スターはお気に入り");
    assert!(c.fields.is_business);
    assert_eq!(c.fields.maiden_name.as_deref(), Some("佐藤"));
    assert_eq!(
        c.fields.emails[1],
        ContactValue {
            label: None,
            value: "info@y.jp".into(),
            is_shared: true
        }
    );
    assert!(
        s.list_contacts_to_push(acct).unwrap().is_empty(),
        "取り込みは送り返さない"
    );
}

#[test]
fn a_remote_delete_only_unlinks() {
    let s = mem_store();
    let acct = account(&s);
    let id = linked_contact(&s, acct, "people/c1", fields("山田太郎", &["t@y.jp"]));
    assert_eq!(
        s.apply_remote_contact(acct, &deleted("people/c1")).unwrap(),
        ApplyOutcome::Deleted
    );
    let c = s.get_contact(id).unwrap();
    assert!(c.deleted_at.is_none(), "連絡先は消さない");
    assert!(c.links.is_empty(), "つながりだけ外す");
    assert_eq!(
        s.apply_remote_contact(acct, &deleted("people/unknown"))
            .unwrap(),
        ApplyOutcome::Skipped
    );
}

#[test]
fn labels_follow_google_but_local_only_tags_survive() {
    let s = mem_store();
    let acct = account(&s);
    s.replace_contact_groups(acct, &[("g1".into(), "取引先".into())])
        .unwrap();
    assert_eq!(
        s.contact_group_id(acct, "取引先").unwrap().as_deref(),
        Some("g1")
    );
    let mut f = fields("山田太郎", &["t@y.jp"]);
    f.tags = vec!["取引先".into()];
    let id = linked_contact(&s, acct, "people/c1", f);
    assert_eq!(
        s.get_contact(id).unwrap().fields.tags,
        vec!["取引先".to_string()]
    );
    {
        let conn = s.conn.lock().unwrap();
        conn.execute("INSERT INTO tags (name) VALUES ('自分用')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO contact_tags (contact_id, tag_id) SELECT ?1, id FROM tags WHERE name = '自分用'",
            params![id],
        )
        .unwrap();
    }
    // Google 側でラベルを外した → 取引先は外れ、Google が知らないタグは残る。
    s.apply_remote_contact(acct, &remote("people/c1", fields("山田太郎", &["t@y.jp"])))
        .unwrap();
    assert_eq!(
        s.get_contact(id).unwrap().fields.tags,
        vec!["自分用".to_string()]
    );

    // 洗い替え: 消えたラベルの行は残さない。送信で作ったラベルは覚える。
    s.replace_contact_groups(acct, &[("g2".into(), "友人".into())])
        .unwrap();
    assert_eq!(s.contact_group_id(acct, "取引先").unwrap(), None);
    s.remember_contact_group(acct, "g3", "新しいラベル")
        .unwrap();
    assert_eq!(
        s.contact_group_id(acct, "新しいラベル").unwrap().as_deref(),
        Some("g3")
    );
}
