use super::*;
use crate::models::ContactLinkState;
use crate::services::store::test_support::person;

fn store() -> Store {
    Store::open_in_memory_for_test()
}

/// 連絡先の同期をしている Google アカウント。
fn account(s: &Store, email: &str) -> i64 {
    let id = s.upsert_google_account(email, None, None).unwrap();
    s.set_google_account_service(id, crate::services::store::GoogleService::Contacts, true)
        .unwrap();
    id
}

fn contact(s: &Store, name: &str) -> i64 {
    i64::from(s.upsert_contact(&person(name, &[])).unwrap().id)
}

fn requests(s: &Store) -> i64 {
    s.conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM contact_create_requests", [], |r| {
            r.get(0)
        })
        .unwrap()
}

#[test]
fn unlinked_contacts_are_never_created_on_their_own() {
    let s = store();
    let acct = account(&s, "a@gmail.com");
    contact(&s, "メールの相手");
    // どこにもつながっていない連絡先を勝手に作ることはしない（作るのは作成待ちを置いた人だけ）。
    assert!(s.list_contacts_to_push(acct).unwrap().is_empty());
}

#[test]
fn a_create_request_is_pushed_once_and_filled_in_after_creation() {
    let s = store();
    let acct = account(&s, "a@gmail.com");
    let c = contact(&s, "山田太郎");
    s.request_contact_create(c, acct).unwrap();
    s.request_contact_create(c, acct).unwrap();
    assert_eq!(requests(&s), 1, "同じ人・同じアカウントの作成待ちは 1 つ");
    let links = s.get_contact(c).unwrap().links;
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].state, ContactLinkState::PendingCreate);

    let push = s.list_contacts_to_push(acct).unwrap();
    assert_eq!(push.len(), 1);
    assert_eq!((push[0].contact_id, push[0].external_id.clone()), (c, None));

    // 作れたら ID が埋まり、作成待ちは消える。
    s.mark_contact_pushed(acct, c, "people/c9", Some("etag1"), None)
        .unwrap();
    assert_eq!(requests(&s), 0);
    let links = s.get_contact(c).unwrap().links;
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].state, ContactLinkState::Synced);
    assert!(s.list_contacts_to_push(acct).unwrap().is_empty());
    // つながったあとは、作成待ちを置いても何もしない（二重に作らない）。
    s.request_contact_create(c, acct).unwrap();
    assert_eq!(requests(&s), 0);
}

#[test]
fn deleted_contacts_are_not_created() {
    let s = store();
    let acct = account(&s, "a@gmail.com");
    let c = contact(&s, "消した人");
    s.request_contact_create(c, acct).unwrap();
    s.delete_contact(c).unwrap();
    assert!(s.list_contacts_to_push(acct).unwrap().is_empty());
}

#[test]
fn stopping_keeps_or_deletes_the_remote_contact() {
    let s = store();
    let acct = account(&s, "a@gmail.com");
    let keep = contact(&s, "残す人");
    let gone = contact(&s, "消す人");
    s.mark_contact_pushed(acct, keep, "people/k", None, None)
        .unwrap();
    s.mark_contact_pushed(acct, gone, "people/g", None, None)
        .unwrap();

    // 向こうは残す: つながりをすぐ外す。Rondine の連絡先は残る。
    s.stop_contact_sync(keep, acct, false).unwrap();
    assert!(s.get_contact(keep).unwrap().links.is_empty());
    assert!(s.contact_identity(acct, "people/k").unwrap().is_none());

    // 向こうも消す: 削除待ちにして、次の同期で削除を送る。
    s.stop_contact_sync(gone, acct, true).unwrap();
    assert_eq!(
        s.get_contact(gone).unwrap().links[0].state,
        ContactLinkState::PendingDelete
    );
    let push = s.list_contacts_to_push(acct).unwrap();
    assert_eq!(push.len(), 1);
    assert_eq!(push[0].external_id.as_deref(), Some("people/g"));
    assert!(push[0].deleted);
    // 送れたら行を消す（Pusher::push_delete と同じ）。連絡先は残る。
    s.forget_contact_identity(acct, "people/g").unwrap();
    assert!(s.get_contact(gone).unwrap().links.is_empty());
    assert!(s.get_contact(gone).unwrap().deleted_at.is_none());
}

#[test]
fn stopping_a_pending_create_just_cancels_it() {
    let s = store();
    let acct = account(&s, "a@gmail.com");
    let c = contact(&s, "やめた人");
    s.request_contact_create(c, acct).unwrap();
    s.stop_contact_sync(c, acct, true).unwrap();
    assert_eq!(requests(&s), 0);
    assert!(s.list_contacts_to_push(acct).unwrap().is_empty());
}

#[test]
fn merging_carries_requests_and_drops_redundant_ones() {
    let s = store();
    let a = account(&s, "a@gmail.com");
    let b = account(&s, "b@gmail.com");
    let keep = contact(&s, "末松");
    let drop = contact(&s, "末松");
    // 残す側は a につながっている。消える側は a と b に作成待ち。
    s.mark_contact_pushed(a, keep, "people/k", None, None)
        .unwrap();
    s.request_contact_create(drop, a).unwrap();
    s.request_contact_create(drop, b).unwrap();
    s.merge_contacts(keep, &[drop], &[]).unwrap();
    // a は既につながっているので作らない。b の作成待ちは残す側へ寄る。
    let links = s.get_contact(keep).unwrap().links;
    let states: Vec<(i32, ContactLinkState)> =
        links.iter().map(|l| (l.account_id, l.state)).collect();
    assert_eq!(
        states,
        vec![
            (a as i32, ContactLinkState::Synced),
            (b as i32, ContactLinkState::PendingCreate)
        ]
    );
    assert!(s
        .list_contacts_to_push(a)
        .unwrap()
        .iter()
        .all(|p| p.external_id.is_some()));
}
