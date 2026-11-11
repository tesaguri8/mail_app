use super::*;
use crate::services::store::Store;

/// 試験用の店（アカウント 1 = 自分 taro@example.invalid）。名前・宛先は作り物。
fn store() -> Store {
    let store = Store::open_in_memory_for_test();
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO accounts (id, email, imap_host, smtp_host)
             VALUES (1, 'taro@example.invalid', 'imap.example.invalid', 'smtp.example.invalid')",
            [],
        )
        .unwrap();
    store
}

/// メール 1 通を入れて id を返す（folder・Message-ID・In-Reply-To・References・差出人・日時）。
fn put(
    store: &Store,
    folder: &str,
    mid: Option<&str>,
    irt: Option<&str>,
    refs: Option<&str>,
    from: &str,
    date: &str,
) -> i64 {
    let conn = store.conn.lock().unwrap();
    let ts = chrono::DateTime::parse_from_rfc3339(date)
        .unwrap()
        .timestamp();
    conn.execute(
        "INSERT INTO emails (account_id, canonical_key, folder, message_id, in_reply_to, references_ids,
                             from_address, subject, date, date_ts, body_plain, clean_body)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, '工程の件', ?7, ?8, '本文', '本文')",
        params![
            format!("{folder}:{date}"),
            folder,
            mid,
            irt,
            refs,
            from,
            date,
            ts
        ],
    )
    .unwrap();
    conn.last_insert_rowid()
}

const ME: &str = "taro@example.invalid";
const THEM: &str = "suzuki@example.invalid";

fn view(store: &Store, id: i64) -> crate::models::ThreadView {
    store.rebuild_threads(1).unwrap();
    store.thread_view(id).unwrap().unwrap()
}

fn refs_of(store: &Store, id: i64) -> ThreadRefs {
    let v = view(store, id);
    let conn = store.conn.lock().unwrap();
    collect(&conn, &v.messages).unwrap()
}

#[test]
fn missing_ids_are_collected_in_reference_order_with_the_root() {
    let s = store();
    // 根 r0（手元に無い）→ r1（手元）→ r2（手元に無い）→ r3（手元）。
    put(
        &s,
        "inbox",
        Some("r1@x"),
        Some("r0@x"),
        Some("<r0@x>"),
        THEM,
        "2026-09-24T13:22:00+09:00",
    );
    let last = put(
        &s,
        "inbox",
        Some("r3@x"),
        Some("r2@x"),
        Some("<r0@x> <r1@x> <r2@x>"),
        THEM,
        "2026-10-06T15:10:00+09:00",
    );
    let refs = refs_of(&s, last);
    assert_eq!(refs.missing, ["r0@x", "r2@x"]);
    assert_eq!(refs.root.as_deref(), Some("r0@x"));
    assert!(refs.root_is_missing());
    let v = view(&s, last);
    let ids: Vec<_> = v
        .missing_refs
        .iter()
        .map(|m| m.message_id.as_str())
        .collect();
    assert_eq!(ids, ["r0@x", "r2@x"]);
    assert!(v
        .missing_refs
        .iter()
        .all(|m| m.state == MissingRefState::Due));
}

#[test]
fn ids_present_elsewhere_in_the_account_are_not_missing() {
    let s = store();
    // r0 は別の会話（件名違い・参照なし）にあるが、同じアカウントの手元にある。
    put(
        &s,
        "archive",
        Some("r0@x"),
        None,
        None,
        ME,
        "2026-01-01T09:00:00+09:00",
    );
    let last = put(
        &s,
        "inbox",
        Some("r1@x"),
        Some("r0@x"),
        Some("r0@x"),
        THEM,
        "2026-09-24T13:22:00+09:00",
    );
    assert!(refs_of(&s, last).missing.is_empty());
}

#[test]
fn an_inferred_id_on_an_own_copy_is_neither_missing_nor_subtracted() {
    // 自分の控え 2 通は ID が無かった。うち 1 通には、相手の返信から推定した ID が入っている（0072）。
    let s = store();
    put(
        &s,
        "sent",
        Some("mine-1@x"),
        None,
        None,
        ME,
        "2026-09-25T09:00:00+09:00",
    );
    put(
        &s,
        "sent",
        None,
        None,
        None,
        ME,
        "2026-10-01T09:00:00+09:00",
    );
    let last = put(
        &s,
        "inbox",
        Some("r9@x"),
        Some("mine-2@x"),
        Some("<mine-1@x> <mine-2@x>"),
        THEM,
        "2026-10-02T09:00:00+09:00",
    );
    // 2 通とも同じ会話に入れる（ID の無い控えは References では束ねられないので、手で寄せる）。
    view(&s, last);
    {
        let conn = s.conn.lock().unwrap();
        conn.execute(
            "UPDATE emails SET logical_thread_id = (SELECT logical_thread_id FROM emails WHERE message_id = 'r9@x')",
            [],
        )
        .unwrap();
    }
    let v = s.thread_view(last).unwrap().unwrap();
    assert_eq!(v.messages.len(), 3);
    let conn = s.conn.lock().unwrap();
    let refs = collect(&conn, &v.messages).unwrap();
    // 推定の ID が入った控え（mine-1）は手元にあるので欠けていない。差し引くのは ID が空のままの 1 通だけ。
    assert_eq!(refs.missing, ["mine-2@x"]);
    assert_eq!(refs.own_without_id, 1);
}

#[test]
fn lookups_are_remembered_and_retried_once_after_thirty_days() {
    let s = store();
    let conn = s.conn.lock().unwrap();
    let ids = vec!["a@x".to_string(), "b@x".to_string(), "c@x".to_string()];
    let t0 = 1_800_000_000;
    record_absent(&conn, 1, "a@x", t0).unwrap();
    record_found(&conn, 1, "b@x", "sent", t0).unwrap();
    let st = |now| {
        states(&conn, 1, &ids, now)
            .unwrap()
            .into_iter()
            .map(|m| m.state)
            .collect::<Vec<_>>()
    };
    use MissingRefState::{Absent, Due};
    // 探した直後: 見つからなかった a・見つけた b は探さない。まだ探していない c は探す。
    assert_eq!(st(t0 + 60), [Absent, Absent, Due]);
    // 30 日たつと a だけ探し直す。
    assert_eq!(st(t0 + RETRY_AFTER_SECS), [Due, Absent, Due]);
    // 探し直しても無ければ、それ以後は何日たっても探さない。
    let t1 = t0 + RETRY_AFTER_SECS;
    record_absent(&conn, 1, "a@x", t1).unwrap();
    assert_eq!(st(t1 + 10 * RETRY_AFTER_SECS)[0], Absent);
    assert!(was_fetched(&conn, 1, "b@x").unwrap());
    assert!(!was_fetched(&conn, 1, "a@x").unwrap());
}

/// 根（9/24 10:33 の自分のメール）を引用した相手の返信の本文。
const QUOTING_ROOT: &str = "末松様\n見積りを拝見しました。\n\n\
-----Original Message-----\n\
From: 末松 太郎 <taro@example.invalid>\n\
Sent: Thursday, September 24, 2026 10:33 AM\n\
To: 鈴木 次郎 <suzuki@example.invalid>\n\
Subject: スラブ案の見積り\n\
\n\
鈴木様\nスラブ案の見積りをお送りします。合計は別紙のとおりです。";

fn set_body(s: &Store, id: i64, body: &str) {
    let clean = crate::services::quotes::split_reply(body).clean;
    s.conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE emails SET body_plain = ?1, clean_body = ?2 WHERE id = ?3",
            params![body, clean, id],
        )
        .unwrap();
}

#[test]
fn the_oldest_frame_is_bound_to_a_missing_root() {
    let s = store();
    let reply = put(
        &s,
        "inbox",
        Some("r1@x"),
        Some("r0@x"),
        Some("<r0@x>"),
        THEM,
        "2026-09-24T13:22:00+09:00",
    );
    set_body(&s, reply, QUOTING_ROOT);
    let v = view(&s, reply);
    let frames = &v.messages[0].unseen_quotes;
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0].root_of_ref.as_deref(), Some("r0@x"));
}

#[test]
fn a_fetched_root_replaces_its_frame_even_when_the_quoted_time_is_off() {
    let s = store();
    // 取り寄せた根。引用の見出しの時刻（10:33・時差なし）と 5 時間ずれている（時差の書き違いなど）。
    put(
        &s,
        "sent",
        Some("r0@x"),
        None,
        None,
        ME,
        "2026-09-24T15:33:00+09:00",
    );
    let reply = put(
        &s,
        "inbox",
        Some("r1@x"),
        Some("r0@x"),
        Some("<r0@x>"),
        THEM,
        "2026-09-24T16:22:00+09:00",
    );
    set_body(&s, reply, QUOTING_ROOT);
    // 探して取り込む前（ただ手元にあるだけ）は、照らし合わせで外れた枠がそのまま残る。
    let before = view(&s, reply);
    assert_eq!(
        before
            .messages
            .iter()
            .map(|m| m.unseen_quotes.len())
            .sum::<usize>(),
        1
    );
    {
        let conn = s.conn.lock().unwrap();
        record_found(&conn, 1, "r0@x", "sent", 1_800_000_000).unwrap();
    }
    let after = view(&s, reply);
    assert!(after.missing_refs.is_empty());
    assert_eq!(
        after
            .messages
            .iter()
            .map(|m| m.unseen_quotes.len())
            .sum::<usize>(),
        0
    );
}

#[test]
fn a_fetched_root_does_not_remove_a_frame_from_someone_else() {
    let s = store();
    put(
        &s,
        "sent",
        Some("r0@x"),
        None,
        None,
        ME,
        "2026-09-24T15:33:00+09:00",
    );
    let reply = put(
        &s,
        "inbox",
        Some("r1@x"),
        Some("r0@x"),
        Some("<r0@x>"),
        THEM,
        "2026-09-24T16:22:00+09:00",
    );
    // 引用されているのは別の人（高橋）のメール。根と差出人が違うので、根の枠とはみなさない。
    set_body(
        &s,
        reply,
        &QUOTING_ROOT.replace(
            "末松 太郎 <taro@example.invalid>",
            "高橋 花子 <hanako@example.invalid>",
        ),
    );
    {
        let conn = s.conn.lock().unwrap();
        record_found(&conn, 1, "r0@x", "sent", 1_800_000_000).unwrap();
    }
    let v = view(&s, reply);
    assert_eq!(
        v.messages
            .iter()
            .map(|m| m.unseen_quotes.len())
            .sum::<usize>(),
        1
    );
}
