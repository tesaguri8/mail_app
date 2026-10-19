use super::*;
use crate::services::store::threads::process_pending_conn;
use crate::services::store::{insert_email, InsertOutcome, NewEmail};

fn test_store() -> Store {
    let store = Store::open_in_memory_for_test();
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO accounts (id, email, imap_host, smtp_host)
             VALUES (1,'me@example.invalid','i','s')",
            [],
        )
        .unwrap();
    store
}

/// 試験用の 1 通。`mid` が None なら Message-ID の無い控え（canonical_key は件名から作る）。
fn mk(
    mid: Option<&str>,
    folder: &str,
    from: &str,
    to: &str,
    day: i64,
    irt: Option<&str>,
    refs: Option<&str>,
) -> NewEmail {
    NewEmail {
        account_id: 1,
        message_id: mid.map(str::to_string),
        canonical_key: mid.map_or_else(|| format!("nomid-{day}"), str::to_string),
        subject: Some(
            if irt.is_some() || day > 1 {
                "Re: 打ち合わせ"
            } else {
                "打ち合わせ"
            }
            .into(),
        ),
        from_address: Some(from.into()),
        from_name: None,
        to_addresses: Some(to.into()),
        to_name: None,
        reply_to: None,
        cc_addresses: None,
        date: Some(format!("2026-10-{day:02}T10:00:00+09:00")),
        date_ts: Some(1_790_000_000 + day * 86400),
        body_plain: Some(format!("本文 {day}")),
        clean_body: Some(format!("本文 {day}")),
        body_html: None,
        auth_result: None,
        list_id: None,
        in_reply_to: irt.map(str::to_string),
        references_ids: refs.map(str::to_string),
        thread_index: None,
        raw_headers: None,
        quotes: vec![],
        has_attachments: false,
        is_read: true,
        uid: None,
        folder: folder.into(),
        verified_self: false,
        attachments: vec![],
    }
}

fn put(store: &Store, e: &NewEmail) -> i64 {
    let conn = store.conn.lock().unwrap();
    let InsertOutcome::Inserted(id) = insert_email(&conn, e).unwrap() else {
        panic!("挿入されるはず");
    };
    process_pending_conn(&conn, 1).unwrap();
    id
}

fn thread_of(store: &Store, id: i64) -> i64 {
    store
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT logical_thread_id FROM emails WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn 親に_message_id_があればそれを返す() {
    let store = test_store();
    let root = mk(
        Some("root@example.invalid"),
        "inbox",
        "you@example.invalid",
        "me@example.invalid",
        1,
        None,
        None,
    );
    put(&store, &root);
    let parent = put(
        &store,
        &mk(
            Some("p@example.invalid"),
            "inbox",
            "you@example.invalid",
            "me@example.invalid",
            2,
            Some("root@example.invalid"),
            Some("root@example.invalid"),
        ),
    );
    let t = store.reply_threading(parent).unwrap().unwrap();
    assert_eq!(t.in_reply_to.as_deref(), Some("p@example.invalid"));
    assert_eq!(
        t.references.as_deref(),
        Some("root@example.invalid p@example.invalid")
    );
    assert_eq!(t.hint_parent_key, None);
}

#[test]
fn 控えに_message_id_が無い親は祖先から_references_を組む() {
    let store = test_store();
    let parent = put(
        &store,
        &mk(
            None,
            "sent",
            "me@example.invalid",
            "you@example.invalid",
            2,
            Some("<p@example.invalid>"),
            Some("root@example.invalid p@example.invalid"),
        ),
    );
    let t = store.reply_threading(parent).unwrap().unwrap();
    assert_eq!(t.in_reply_to, None);
    assert_eq!(
        t.references.as_deref(),
        Some("root@example.invalid p@example.invalid")
    );
    assert!(t.hint_parent_key.is_some());
}

#[test]
fn 控えに_message_id_が無い新規の送信への返信も同じスレッドに載る() {
    let store = test_store();
    // 0.1.0-alpha.34 までの送信: Message-ID も In-Reply-To も無い新規の控え。
    let parent = put(
        &store,
        &mk(
            None,
            "sent",
            "me@example.invalid",
            "you@example.invalid",
            1,
            None,
            None,
        ),
    );
    let t = store.reply_threading(parent).unwrap().unwrap();
    assert_eq!(t.in_reply_to, None);
    assert_eq!(t.references, None);
    let key = t.hint_parent_key.expect("親のスレッドを手がかりにする");
    store
        .put_thread_hint(1, "<r@example.invalid>", &key)
        .unwrap();

    // 返信（自分も Cc に入れた全員返信）の控えと、受信箱に届いた写し。ヘッダでは親へ戻れない。
    let reply = |folder| {
        mk(
            Some("r@example.invalid"),
            folder,
            "me@example.invalid",
            "you@example.invalid",
            2,
            None,
            None,
        )
    };
    let sent = put(&store, &reply("sent"));
    let inbox = put(&store, &reply("inbox"));
    assert_eq!(thread_of(&store, sent), thread_of(&store, parent));
    assert_eq!(thread_of(&store, inbox), thread_of(&store, parent));
    // 会話では、受信箱と送信済みの写しは 1 つのバブル（受信箱の写し）にまとまる。
    let view = store.thread_view(parent).unwrap().unwrap();
    let ids: Vec<i64> = view.messages.iter().map(|m| i64::from(m.id)).collect();
    assert_eq!(ids, vec![parent, inbox]);

    // 相手からの、その返信への返信も同じスレッドに続く。
    let next = put(
        &store,
        &mk(
            Some("n@example.invalid"),
            "inbox",
            "you@example.invalid",
            "me@example.invalid",
            3,
            Some("r@example.invalid"),
            Some("r@example.invalid"),
        ),
    );
    assert_eq!(thread_of(&store, next), thread_of(&store, parent));

    // 再構築しても手がかりで同じスレッドに残る。
    store.rebuild_threads(1).unwrap();
    assert_eq!(thread_of(&store, sent), thread_of(&store, parent));
    assert_eq!(thread_of(&store, next), thread_of(&store, parent));
}

#[test]
fn 親が無ければ_none() {
    let store = test_store();
    assert_eq!(store.reply_threading(999).unwrap(), None);
}
