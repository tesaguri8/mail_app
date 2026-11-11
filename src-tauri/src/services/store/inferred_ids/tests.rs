use super::*;
use crate::services::store::threads::process_pending_conn;
use crate::services::store::{insert_email, InsertOutcome, NewEmail, NewQuote};

/// 試験の口座は me@example.invalid、相手は you@example.invalid（どちらも合成）。
const ME: &str = "me@example.invalid";
const YOU: &str = "you@example.invalid";

fn test_store() -> Store {
    let store = Store::open_in_memory_for_test();
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1, ?1, 'i', 's')",
            params![ME],
        )
        .unwrap();
    store
}

/// 10 月 `day` 日 10:00（+09:00）の 1 通。`mid` が None なら Message-ID の無い控え。
#[allow(clippy::too_many_arguments)]
fn mk(
    key: &str,
    mid: Option<&str>,
    folder: &str,
    from: &str,
    to: &str,
    subject: &str,
    day: u32,
    irt: Option<&str>,
    refs: Option<&str>,
) -> NewEmail {
    let date = format!("2026-10-{day:02}T10:00:00+09:00");
    let date_ts = chrono::DateTime::parse_from_rfc3339(&date)
        .unwrap()
        .timestamp();
    NewEmail {
        account_id: 1,
        message_id: mid.map(str::to_string),
        canonical_key: mid.map_or_else(|| key.to_string(), str::to_string),
        subject: Some(subject.into()),
        from_address: Some(from.into()),
        from_name: None,
        to_addresses: Some(to.into()),
        to_name: None,
        reply_to: None,
        cc_addresses: None,
        date: Some(date),
        date_ts: Some(date_ts),
        body_plain: Some(format!("本文 {key}")),
        clean_body: Some(format!("本文 {key}")),
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

/// 新規の控え C（10/1、ID 無し）。
fn copy_new() -> NewEmail {
    mk("c-new", None, "sent", ME, YOU, "打ち合わせ", 1, None, None)
}

/// C への相手の返信 R（10/3）。X は中継が C に付けた ID。
fn reply(key: &str, x: &str, refs: &str, day: u32) -> NewEmail {
    mk(
        key,
        Some(key),
        "inbox",
        YOU,
        ME,
        "Re: 打ち合わせ",
        day,
        Some(x),
        Some(refs),
    )
}

/// 挿入して、取り込みと同じ後処理（推定・会話の割当）を走らせる。
fn put(store: &Store, e: &NewEmail) -> i64 {
    let conn = store.conn.lock().unwrap();
    let InsertOutcome::Inserted(id) = insert_email(&conn, e).unwrap() else {
        panic!("挿入されるはず");
    };
    process_pending_conn(&conn, 1).unwrap();
    id
}

/// 推定より前の版で取り込んだ状態を作る（会話の割当だけ。推定は走らせない）。
fn put_without_inference(store: &Store, e: &NewEmail) -> i64 {
    let conn = store.conn.lock().unwrap();
    let InsertOutcome::Inserted(id) = insert_email(&conn, e).unwrap() else {
        panic!("挿入されるはず");
    };
    assign_thread(&conn, id).unwrap();
    id
}

fn mid_of(store: &Store, id: i64) -> Option<String> {
    store
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT message_id FROM emails WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
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

/// 記録（state, message_id）を canonical_key の順に返す。
fn records(store: &Store) -> Vec<(String, String, Option<String>)> {
    let conn = store.conn.lock().unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT canonical_key, state, message_id FROM inferred_message_ids
             ORDER BY canonical_key",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    rows
}

fn startup(store: &Store) -> usize {
    link_all_conn(&store.conn.lock().unwrap(), std::time::Duration::ZERO).unwrap()
}

#[test]
fn 新規の控えに_1_通だけ合えば_id_を入れ_割れた会話を_1_つにする() {
    let store = test_store();
    let c = put_without_inference(&store, &copy_new());
    let r = put_without_inference(
        &store,
        &reply(
            "r1@example.invalid",
            "x1@relay.invalid",
            "x1@relay.invalid",
            3,
        ),
    );
    assert_ne!(
        thread_of(&store, c),
        thread_of(&store, r),
        "推定の前は割れている"
    );

    assert_eq!(startup(&store), 1);
    assert_eq!(mid_of(&store, c).as_deref(), Some("x1@relay.invalid"));
    assert_eq!(thread_of(&store, c), thread_of(&store, r));
    assert_eq!(
        records(&store),
        vec![(
            "sent:c-new".to_string(),
            "linked".to_string(),
            Some("x1@relay.invalid".to_string())
        )]
    );
    // 2 回目の起動では何もしない。
    assert_eq!(startup(&store), 0);
}

#[test]
fn 返信の控えは_references_の並びが合えば結ぶ() {
    let store = test_store();
    put(
        &store,
        &mk(
            "p0@example.invalid",
            Some("p0@example.invalid"),
            "inbox",
            YOU,
            ME,
            "打ち合わせ",
            1,
            None,
            None,
        ),
    );
    let c = put(
        &store,
        &mk(
            "c-re",
            None,
            "sent",
            ME,
            YOU,
            "Re: 打ち合わせ",
            2,
            Some("p0@example.invalid"),
            Some("p0@example.invalid"),
        ),
    );
    let r = put(
        &store,
        &reply(
            "r1@example.invalid",
            "x1@relay.invalid",
            "p0@example.invalid x1@relay.invalid",
            3,
        ),
    );
    assert_eq!(mid_of(&store, c).as_deref(), Some("x1@relay.invalid"));
    assert_eq!(thread_of(&store, c), thread_of(&store, r));
}

#[test]
fn 並びが合わない返信には結ばない() {
    let store = test_store();
    let c = put(
        &store,
        &mk(
            "c-re",
            None,
            "sent",
            ME,
            YOU,
            "Re: 打ち合わせ",
            2,
            Some("p0@example.invalid"),
            Some("p0@example.invalid"),
        ),
    );
    // X の 1 つ前が C の In-Reply-To ではない。
    put(
        &store,
        &reply(
            "r1@example.invalid",
            "x1@relay.invalid",
            "zz@example.invalid x1@relay.invalid",
            3,
        ),
    );
    // X が References に無く、References も空ではない。
    put(
        &store,
        &reply(
            "r2@example.invalid",
            "x2@relay.invalid",
            "p0@example.invalid",
            4,
        ),
    );
    assert_eq!(mid_of(&store, c), None);
    assert!(records(&store).is_empty());
}

#[test]
fn 差出人_日時_件名が合わなければ結ばない() {
    let store = test_store();
    let c = put(&store, &copy_new());
    // 宛先にいない人からの返信。
    let mut other = reply(
        "r1@example.invalid",
        "x1@relay.invalid",
        "x1@relay.invalid",
        3,
    );
    other.from_address = Some("other@example.invalid".into());
    put(&store, &other);
    // 件名が違う。
    let mut subj = reply(
        "r2@example.invalid",
        "x2@relay.invalid",
        "x2@relay.invalid",
        3,
    );
    subj.subject = Some("Re: 別の件".into());
    put(&store, &subj);
    assert_eq!(mid_of(&store, c), None);
    // 控えより前の日時の返信。
    let store = test_store();
    let mut late_copy = copy_new();
    late_copy.date = Some("2026-10-05T10:00:00+09:00".into());
    late_copy.date_ts = Some(late_copy.date_ts.unwrap() + 4 * 86400);
    let c = put(&store, &late_copy);
    put(
        &store,
        &reply(
            "r3@example.invalid",
            "x3@relay.invalid",
            "x3@relay.invalid",
            3,
        ),
    );
    assert_eq!(mid_of(&store, c), None);
}

#[test]
fn 合う控えが_2_通あれば曖昧なので結ばない() {
    let store = test_store();
    let a = put(&store, &copy_new());
    let b = put(
        &store,
        &mk(
            "c-new-2",
            None,
            "sent",
            ME,
            YOU,
            "打ち合わせ",
            2,
            None,
            None,
        ),
    );
    put(
        &store,
        &reply(
            "r1@example.invalid",
            "x1@relay.invalid",
            "x1@relay.invalid",
            3,
        ),
    );
    assert_eq!(mid_of(&store, a), None);
    assert_eq!(mid_of(&store, b), None);
    assert!(
        records(&store).is_empty(),
        "曖昧なものは記録もしない（後で 1 通に絞れたら結ぶ）"
    );
}

#[test]
fn 同じ控えに別の_x_が_2_つ来たら結ばない() {
    let store = test_store();
    let c = put_without_inference(&store, &copy_new());
    put_without_inference(
        &store,
        &reply(
            "r1@example.invalid",
            "x1@relay.invalid",
            "x1@relay.invalid",
            3,
        ),
    );
    put_without_inference(
        &store,
        &reply(
            "r2@example.invalid",
            "x2@relay.invalid",
            "x2@relay.invalid",
            4,
        ),
    );
    assert_eq!(startup(&store), 0);
    assert_eq!(mid_of(&store, c), None);
    assert_eq!(
        records(&store),
        vec![("sent:c-new".to_string(), "conflict".to_string(), None)]
    );
}

#[test]
fn 結んだあとで別の_x_が来たら取り消して以後は結ばない() {
    let store = test_store();
    let c = put(&store, &copy_new());
    let r1 = put(
        &store,
        &reply(
            "r1@example.invalid",
            "x1@relay.invalid",
            "x1@relay.invalid",
            3,
        ),
    );
    assert_eq!(mid_of(&store, c).as_deref(), Some("x1@relay.invalid"));
    assert_eq!(thread_of(&store, c), thread_of(&store, r1));

    put(
        &store,
        &reply(
            "r2@example.invalid",
            "x2@relay.invalid",
            "x2@relay.invalid",
            4,
        ),
    );
    assert_eq!(mid_of(&store, c), None);
    assert_eq!(
        records(&store),
        vec![("sent:c-new".to_string(), "conflict".to_string(), None)]
    );
    assert_ne!(
        thread_of(&store, c),
        thread_of(&store, r1),
        "会話も元に戻る"
    );
    assert_eq!(startup(&store), 0);
    assert_eq!(mid_of(&store, c), None);
}

#[test]
fn 再同期で控えを取り込み直しても_id_は消えない() {
    let store = test_store();
    let c = put(&store, &copy_new());
    let r = put(
        &store,
        &reply(
            "r1@example.invalid",
            "x1@relay.invalid",
            "x1@relay.invalid",
            3,
        ),
    );
    // 同じ控え（ヘッダに Message-ID は無いまま）がもう一度来る。
    {
        let conn = store.conn.lock().unwrap();
        let out = insert_email(&conn, &copy_new()).unwrap();
        assert!(!matches!(out, InsertOutcome::Inserted(_)), "同じ行に入る");
        process_pending_conn(&conn, 1).unwrap();
    }
    assert_eq!(mid_of(&store, c).as_deref(), Some("x1@relay.invalid"));
    store.rebuild_threads(1).unwrap();
    assert_eq!(
        thread_of(&store, c),
        thread_of(&store, r),
        "再構築でも同じ会話"
    );
}

#[test]
fn 控えがあとから入っても結ぶ() {
    // 初回の同期は受信箱が先に入ることがある。
    let store = test_store();
    let r = put(
        &store,
        &reply(
            "r1@example.invalid",
            "x1@relay.invalid",
            "x1@relay.invalid",
            3,
        ),
    );
    let c = put(&store, &copy_new());
    assert_eq!(mid_of(&store, c).as_deref(), Some("x1@relay.invalid"));
    assert_eq!(thread_of(&store, c), thread_of(&store, r));
}

/// 引用の見出しが 1 つある返信（差出人は自分、日時は `at`）。
fn reply_quoting(at: &str) -> NewEmail {
    let mut r = reply(
        "r1@example.invalid",
        "x1@relay.invalid",
        "x1@relay.invalid",
        3,
    );
    r.quotes = vec![NewQuote {
        order: 0,
        quoted_from: Some(format!("自分 <{ME}>")),
        quoted_at: Some(at.into()),
        fingerprint: "fp".into(),
        bare_gt: false,
        gt_depth: 1,
        body: "本文 c-new".into(),
    }];
    r
}

#[test]
fn 引用の見出しの日時が控えと違えば結ばない() {
    let store = test_store();
    let c = put(&store, &copy_new());
    put(&store, &reply_quoting("2026年10月1日 15:00"));
    assert_eq!(mid_of(&store, c), None);
}

#[test]
fn 引用の見出しの日時が控えと合えば結ぶ() {
    let store = test_store();
    let c = put(&store, &copy_new());
    put(&store, &reply_quoting("Thu, 1 Oct 2026 10:00:00 +0900"));
    assert_eq!(mid_of(&store, c).as_deref(), Some("x1@relay.invalid"));
}

#[test]
fn 取り消すと_id_を外して会話を戻し_以後は推定しない() {
    let store = test_store();
    let c = put_without_inference(&store, &copy_new());
    let r = put_without_inference(
        &store,
        &reply(
            "r1@example.invalid",
            "x1@relay.invalid",
            "x1@relay.invalid",
            3,
        ),
    );
    assert_eq!(startup(&store), 1);

    assert_eq!(store.revert_inferred_message_ids(1).unwrap(), 1);
    assert_eq!(mid_of(&store, c), None);
    assert_ne!(thread_of(&store, c), thread_of(&store, r));
    assert_eq!(
        records(&store),
        vec![(
            "sent:c-new".to_string(),
            "reverted".to_string(),
            Some("x1@relay.invalid".to_string())
        )]
    );
    assert_eq!(startup(&store), 0);
    assert_eq!(mid_of(&store, c), None);
}

#[test]
fn 迷惑メールの_id_無しは対象にしない() {
    let store = test_store();
    let mut spam = copy_new();
    spam.folder = "spam".into();
    let s = put(&store, &spam);
    put(
        &store,
        &reply(
            "r1@example.invalid",
            "x1@relay.invalid",
            "x1@relay.invalid",
            3,
        ),
    );
    assert_eq!(mid_of(&store, s), None);
}

#[test]
fn references_の並びの判定() {
    assert!(chain_fits(Some("x"), "x", None));
    assert!(chain_fits(None, "x", None));
    assert!(chain_fits(Some("p0 p1 x"), "x", Some("<p1>")));
    assert!(!chain_fits(Some("p0 p1 x"), "x", Some("p0")));
    assert!(!chain_fits(Some("x"), "x", Some("p0")));
    assert!(!chain_fits(None, "x", Some("p0")));
    assert!(!chain_fits(Some("p0"), "x", Some("p0")));
}

/// 計測用の 1 通（日時は秒で与える）。
#[allow(clippy::too_many_arguments)]
fn mk_at(
    key: &str,
    mid: Option<&str>,
    folder: &str,
    from: &str,
    to: &str,
    subject: &str,
    ts: i64,
    irt: Option<&str>,
    refs: Option<&str>,
) -> NewEmail {
    let mut e = mk(key, mid, folder, from, to, subject, 1, irt, refs);
    e.date_ts = Some(ts);
    e.date = chrono::DateTime::from_timestamp(ts, 0).map(|d| d.to_rfc3339());
    e
}

/// vaio に近い件数（emails 約 5 万行、ID の無い控え 200 通、結べるもの 150 通、親の無い返信 2,400 通）
/// で、起動時の推定と、取り込み 1 回ぶんの推定にかかる時間を測る。
/// `cargo test --lib inferred_ids::tests::計測 -- --ignored --nocapture`
#[test]
#[ignore = "計測用（5 万行を作るので数十秒かかる）"]
fn 計測_5_万行で起動時と取り込みの推定() {
    use std::time::Instant;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mail.db");
    let store = Store::open(&path).unwrap();
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1, ?1, 'i', 's')",
            params![ME],
        )
        .unwrap();
    let base = 1_780_000_000_i64;
    let mut rows: Vec<NewEmail> = Vec::new();
    // ふつうの会話: 5 通ずつ、References を積む（9,450 会話 = 47,250 通）。
    for i in 0..9_450 {
        let mut chain: Vec<String> = Vec::new();
        for k in 0..5 {
            let mid = format!("f{i}.{k}@example.invalid");
            let (from, to, folder) = if k % 2 == 0 {
                (format!("p{i}@example.invalid"), ME.to_string(), "inbox")
            } else {
                (ME.to_string(), format!("p{i}@example.invalid"), "sent")
            };
            let refs = (!chain.is_empty()).then(|| chain.join(" "));
            rows.push(mk_at(
                &mid,
                Some(&mid),
                folder,
                &from,
                &to,
                &format!("Re: 用件 {i}"),
                base + i64::from(i) * 600 + k * 60,
                chain.last().map(String::as_str),
                refs.as_deref(),
            ));
            chain.push(mid);
        }
    }
    // 親が手元に無い返信（メーリングリスト等）: 2,400 通。
    for i in 0..2_400 {
        let mid = format!("lost{i}@example.invalid");
        let gone = format!("gone{i}@example.invalid");
        rows.push(mk_at(
            &mid,
            Some(&mid),
            "inbox",
            &format!("l{i}@example.invalid"),
            ME,
            &format!("Re: 迷子 {i}"),
            base + 1_000 + i64::from(i) * 7,
            Some(&gone),
            Some(&gone),
        ));
    }
    // ID の無い控え 200 通。うち 150 通に返信（R）と、その返信への返信が 1 通ずつ届いている。
    for i in 0..200 {
        let to = format!("c{i}@example.invalid");
        let t0 = base + 2_000 + i64::from(i) * 3_600;
        rows.push(mk_at(
            &format!("bare{i}"),
            None,
            "sent",
            ME,
            &to,
            &format!("相談 {i}"),
            t0,
            None,
            None,
        ));
        if i < 150 {
            let x = format!("x{i}@relay.invalid");
            let r = format!("r{i}@example.invalid");
            rows.push(mk_at(
                &r,
                Some(&r),
                "inbox",
                &to,
                ME,
                &format!("Re: 相談 {i}"),
                t0 + 600,
                Some(&x),
                Some(&x),
            ));
            let r2 = format!("rr{i}@example.invalid");
            let refs2 = format!("{x} {r}");
            rows.push(mk_at(
                &r2,
                Some(&r2),
                "inbox",
                &to,
                ME,
                &format!("Re: 相談 {i}"),
                t0 + 1_200,
                Some(&r),
                Some(&refs2),
            ));
        }
    }
    let n_rows = rows.len();
    {
        let t = Instant::now();
        let mut conn = store.conn.lock().unwrap();
        let tx = conn.transaction().unwrap();
        for e in &rows {
            let InsertOutcome::Inserted(id) = insert_email(&tx, e).unwrap() else {
                panic!("挿入されるはず");
            };
            assign_thread(&tx, id).unwrap();
        }
        tx.commit().unwrap();
        eprintln!("用意: {n_rows} 通 {:.1} 秒", t.elapsed().as_secs_f64());
    }

    let t = Instant::now();
    let linked = link_sent_copies_at(&path).unwrap();
    let startup_secs = t.elapsed().as_secs_f64();
    eprintln!("起動時の推定: 結んだ控え {linked} 通 {startup_secs:.2} 秒（チャンク間の待ち 15 ms を含む）");
    assert_eq!(linked, 150);
    {
        let conn = store.conn.lock().unwrap();
        let split: i64 = conn
            .query_row(
                "SELECT count(*) FROM emails c JOIN emails r
                   ON r.account_id = c.account_id AND r.in_reply_to = c.message_id
                 WHERE c.folder = 'sent' AND c.canonical_key LIKE 'sent:bare%'
                   AND c.logical_thread_id IS NOT r.logical_thread_id",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(split, 0, "控えと返信が同じ会話");
        let rr_split: i64 = conn
            .query_row(
                "SELECT count(*) FROM emails r JOIN emails rr
                   ON rr.account_id = r.account_id AND rr.in_reply_to = r.message_id
                 WHERE r.message_id LIKE 'r%@example.invalid' AND rr.message_id LIKE 'rr%'
                   AND r.logical_thread_id IS NOT rr.logical_thread_id",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rr_split, 0, "返信への返信も同じ会話");
    }

    // 取り込み 1 回ぶん: 残りの控えの 1 通へ返信が届く（ふつうのメール 20 通と一緒に）。
    let t = Instant::now();
    {
        let conn = store.conn.lock().unwrap();
        let to = "c150@example.invalid";
        let x = "x150@relay.invalid";
        insert_email(
            &conn,
            &mk_at(
                "r150@example.invalid",
                Some("r150@example.invalid"),
                "inbox",
                to,
                ME,
                "Re: 相談 150",
                base + 9_000_000,
                Some(x),
                Some(x),
            ),
        )
        .unwrap();
        for k in 0..20 {
            let mid = format!("new{k}@example.invalid");
            insert_email(
                &conn,
                &mk_at(
                    &mid,
                    Some(&mid),
                    "inbox",
                    "n@example.invalid",
                    ME,
                    "お知らせ",
                    base + 9_000_100 + k,
                    None,
                    None,
                ),
            )
            .unwrap();
        }
        process_pending_conn(&conn, 1).unwrap();
    }
    eprintln!(
        "取り込み 1 回（21 通、うち結べる返信 1 通）: {:.2} 秒",
        t.elapsed().as_secs_f64()
    );
    let t = Instant::now();
    {
        let conn = store.conn.lock().unwrap();
        for k in 0..20 {
            let mid = format!("plain{k}@example.invalid");
            insert_email(
                &conn,
                &mk_at(
                    &mid,
                    Some(&mid),
                    "inbox",
                    "n@example.invalid",
                    ME,
                    "お知らせ",
                    base + 9_100_000 + k,
                    None,
                    None,
                ),
            )
            .unwrap();
        }
        // 推定だけにかかる分を分けて測る（残りは従来の会話の割当）。
        let pending: Vec<i64> = conn
            .prepare("SELECT id FROM emails WHERE logical_thread_id IS NULL")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let ti = Instant::now();
        assert_eq!(link_for_pending(&conn, 1, &pending).unwrap(), 0);
        eprintln!("  うち推定だけ: {:.3} 秒", ti.elapsed().as_secs_f64());
        process_pending_conn(&conn, 1).unwrap();
    }
    eprintln!(
        "取り込み 1 回（20 通、結べるもの無し）: {:.2} 秒",
        t.elapsed().as_secs_f64()
    );
    let c150: Option<String> = store
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT message_id FROM emails WHERE canonical_key = 'sent:bare150'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(c150.as_deref(), Some("x150@relay.invalid"));
}
