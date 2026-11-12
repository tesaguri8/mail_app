//! 絞り込みの試験。DB で絞った一覧が、絞り込み無しの一覧に画面側の述語（`mailFilters.ts` /
//! `DateFilter.tsx` / `TagFilter.tsx`）を当てた結果と一致し、日ごとの件数・未読がその一覧と揃うこと。

use crate::models::{ThreadDateBounds, ThreadFilter, ThreadListItem, ThreadToggle};
use crate::services::store::emails::{insert_email, NewEmail};
use crate::services::store::threads::process_pending_conn;
use crate::services::store::Store;

const BASE: i64 = 1_790_000_000; // 2026-09 ごろ

fn test_store() -> Store {
    let store = Store::open_in_memory_for_test();
    store
        .conn
        .lock()
        .unwrap()
        .execute_batch(
            "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'me@example.invalid','i','s');
             INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (2,'me2@example.invalid','i','s');",
        )
        .unwrap();
    store
}

/// 1 通。`parent` があれば、その Message-ID への返信（同じ件名）にする。
fn mail(account_id: i64, n: usize, ts: Option<i64>, from: &str, parent: Option<usize>) -> NewEmail {
    let mid = format!("m{n}@example.invalid");
    let root = parent.unwrap_or(n);
    NewEmail {
        account_id,
        message_id: Some(mid.clone()),
        canonical_key: mid,
        subject: Some(format!("件名 {root}")),
        from_address: Some(from.into()),
        from_name: None,
        to_addresses: Some("me@example.invalid".into()),
        to_name: None,
        reply_to: None,
        cc_addresses: None,
        date: ts.map(|t| {
            chrono::DateTime::from_timestamp(t, 0)
                .expect("試験の時刻は範囲内")
                .to_rfc3339()
        }),
        date_ts: ts,
        body_plain: Some("body".into()),
        clean_body: Some("body".into()),
        body_html: None,
        auth_result: None,
        list_id: None,
        in_reply_to: parent.map(|p| format!("m{p}@example.invalid")),
        references_ids: parent.map(|p| format!("m{p}@example.invalid")),
        thread_index: None,
        raw_headers: None,
        quotes: vec![],
        has_attachments: n % 11 == 0,
        is_read: n % 3 != 0,
        uid: None,
        folder: "inbox".into(),
        verified_self: false,
        attachments: vec![],
    }
}

/// 差出人の顔ぶれ: 住所録の本人・お気に入り・住所録のドメイン（グリーン）・手動グリーン・
/// 警告で外したドメイン・返信歴・見知らぬ人・`@` の無いアドレス・`@` が 2 つあるアドレス。
const SENDERS: &[&str] = &[
    "known@corp.invalid",
    "VIP@corp.invalid",
    "other@corp.invalid",
    "x@green.invalid",
    "y@warn.invalid",
    "replied@far.invalid",
    "stranger@far.invalid",
    "no-at-sign",
    " odd@at@green.invalid. ",
];

/// 何日にもわたるスレッド（返信で 2〜3 通になるものを混ぜる）と、印・添付・タグ・住所録を入れる。
fn seed(store: &Store) {
    let conn = store.conn.lock().unwrap();
    let mut n = 0;
    for i in 0..60_i64 {
        n += 1;
        let root = n;
        // 日時の無いスレッドを少し混ぜる。
        let ts = (i % 17 != 5).then_some(BASE + i * 5 * 3600);
        let from = SENDERS[(i as usize) % SENDERS.len()];
        insert_email(&conn, &mail(1, root, ts, from, None)).unwrap();
        // 3 本に 1 本は返信を足す（代表は新しい返信へ移り、元のメールは代表でなくなる）。
        if i % 3 == 0 {
            n += 1;
            let reply_ts = ts.map(|t| t + 1800);
            let from = SENDERS[(i as usize + 4) % SENDERS.len()];
            insert_email(&conn, &mail(1, n, reply_ts, from, Some(root))).unwrap();
        }
    }
    // 別アカウント（アカウントの絞り込みと合わせて効くこと）。
    for i in 0..9_i64 {
        n += 1;
        let from = SENDERS[(i as usize) % SENDERS.len()];
        insert_email(&conn, &mail(2, n, Some(BASE + i * 7 * 3600), from, None)).unwrap();
    }
    conn.execute_batch(
        "INSERT INTO contacts (id, display_name, is_favorite) VALUES (1, '知り合い', 0), (2, 'お気に入り', 1),
                                                                 (3, '消した人', 1);
         UPDATE contacts SET deleted_at = '2026-01-01' WHERE id = 3;
         INSERT INTO contact_emails (contact_id, value) VALUES (1, 'known@corp.invalid'),
                                                               (2, 'vip@corp.invalid'),
                                                               (3, 'stranger@far.invalid');
         INSERT INTO green_domains (domain) VALUES ('green.invalid'), ('warn.invalid');
         INSERT INTO warning_domains (domain) VALUES ('warn.invalid');
         INSERT INTO sent_addresses (address, sent_count) VALUES ('replied@far.invalid', 1);
         INSERT INTO tags (id, name) VALUES (1, '仕事'), (2, '家族'), (3, '使っていない');
         -- 添付: 本来の添付と、本文に埋め込んだ画像（数えない）。
         INSERT INTO attachments (email_id, filename, kind)
           SELECT id, 'a.pdf', 'attachment' FROM emails WHERE id % 7 = 0;
         INSERT INTO attachments (email_id, filename, kind)
           SELECT id, 'i.png', 'inline' FROM emails WHERE id % 5 = 0;
         -- スター: 代表でない行にだけ付くことがある。
         UPDATE emails SET is_flagged = 1 WHERE id % 4 = 1;
         INSERT INTO email_tags (email_id, tag_id) SELECT id, 1 FROM emails WHERE id % 6 = 2;
         INSERT INTO email_tags (email_id, tag_id) SELECT id, 2 FROM emails WHERE id % 9 = 4;",
    )
    .unwrap();
    for a in [1, 2] {
        process_pending_conn(&conn, a).unwrap();
    }
}

/// 画面側の述語（`matchesFilters` / `matchesNoneOfFilters` / `matchesDate` / `matchesTags`）を写したもの。
fn passes(m: &ThreadListItem, f: &ThreadFilter) -> bool {
    let has = |t: ThreadToggle| match t {
        ThreadToggle::Unread => !m.is_read,
        ThreadToggle::Attachment => m.has_real_attachments,
        ThreadToggle::Star => m.is_starred,
        ThreadToggle::Green => m.is_green,
        ThreadToggle::Vip => m.is_vip,
        ThreadToggle::Known => m.is_known,
        ThreadToggle::Replied => m.is_replied,
    };
    let toggles = if f.invert {
        f.toggles.iter().all(|t| !has(*t))
    } else {
        f.toggles.iter().all(|t| has(*t))
    };
    let date = match f.date {
        None => true,
        Some(b) => m
            .date
            .as_deref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|d| d.timestamp())
            .is_some_and(|ts| {
                !b.since.is_some_and(|s| ts < s) && !b.until.is_some_and(|u| ts >= u)
            }),
    };
    let tags = f.tag_ids.is_empty() || m.tag_ids.iter().any(|t| f.tag_ids.contains(&i64::from(*t)));
    toggles && date && tags
}

fn toggles(ts: &[ThreadToggle], invert: bool) -> ThreadFilter {
    ThreadFilter {
        toggles: ts.to_vec(),
        invert,
        ..Default::default()
    }
}

/// 試す絞り込み: トグル 1 つずつ（と反転）・組み合わせ・期間・タグ・全部入り。
fn cases() -> Vec<ThreadFilter> {
    use ThreadToggle::*;
    let all = [Unread, Attachment, Star, Green, Vip, Known, Replied];
    let mut v: Vec<ThreadFilter> = all
        .iter()
        .flat_map(|t| [toggles(&[*t], false), toggles(&[*t], true)])
        .collect();
    v.push(toggles(&[Unread, Star], false));
    v.push(toggles(&[Known, Unread], true));
    v.push(toggles(&[Green, Attachment, Replied], true));
    let mid = BASE + 100 * 3600;
    for (since, until) in [
        (Some(mid), None),
        (None, Some(mid)),
        (Some(mid), Some(mid + 80 * 3600)),
        (None, None),
    ] {
        v.push(ThreadFilter {
            date: Some(ThreadDateBounds { since, until }),
            ..Default::default()
        });
    }
    v.push(ThreadFilter {
        tag_ids: vec![1, 2],
        ..Default::default()
    });
    v.push(ThreadFilter {
        tag_ids: vec![3],
        ..Default::default()
    });
    v.push(ThreadFilter {
        toggles: vec![Unread],
        invert: true,
        date: Some(ThreadDateBounds {
            since: Some(BASE),
            until: Some(mid + 120 * 3600),
        }),
        tag_ids: vec![1],
        reading_id: None,
    });
    v
}

/// DB で絞った一覧が、絞り込み無しの一覧に画面側の述語を当てたものと同じ（並びも）。
#[test]
fn 絞った一覧は画面側の絞り込みと一致する() {
    let store = test_store();
    seed(&store);
    for account in [Some(1), None] {
        let all = store.list_threads(account, "inbox", 10_000, 0).unwrap();
        for f in cases() {
            let expect: Vec<i32> = all.iter().filter(|m| passes(m, &f)).map(|m| m.id).collect();
            let got: Vec<i32> = store
                .list_threads_filtered(account, "inbox", &f, 10_000, 0)
                .unwrap()
                .iter()
                .map(|m| m.id)
                .collect();
            assert_eq!(got, expect, "{f:?}");
        }
    }
}

/// 試験のデータが偏っていない: どの絞り込みも、空でも全部でもない（タグ 3 だけは誰も持たない）。
#[test]
fn 試験のデータはどの条件でも分かれる() {
    let store = test_store();
    seed(&store);
    let all = store.list_threads(Some(1), "inbox", 10_000, 0).unwrap();
    assert_eq!(all.iter().filter(|m| m.message_count == 2).count(), 20);
    // 代表でない行だけに未読・スターがあるスレッドがある（スレッド全体で判定していること）。
    let only_member = |col: &str| -> i64 {
        store
            .conn
            .lock()
            .unwrap()
            .query_row(
                &format!(
                    "SELECT count(*) FROM emails r WHERE r.is_folder_rep = 1 AND r.{col} = 0
                       AND EXISTS (SELECT 1 FROM emails t WHERE t.logical_thread_id = r.logical_thread_id
                                     AND t.id <> r.id AND t.{col} <> 0)"
                ),
                [],
                |r| r.get(0),
            )
            .unwrap()
    };
    assert!(only_member("is_flagged") > 0);
    assert!(
        store
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM emails r WHERE r.is_folder_rep = 1 AND r.is_read = 1
                   AND EXISTS (SELECT 1 FROM emails t WHERE t.logical_thread_id = r.logical_thread_id
                                 AND t.id <> r.id AND t.is_read = 0)",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            > 0
    );
    for f in cases() {
        let k = all.iter().filter(|m| passes(m, &f)).count();
        if f.tag_ids == [3] {
            assert_eq!(k, 0);
        } else if f.date == Some(ThreadDateBounds::default()) {
            // 期間の端が無くても、日時の無いスレッドは外れる。
            assert!(k < all.len() && k > 0, "{f:?}");
        } else {
            assert!(k > 0 && k < all.len(), "{f:?}: {k}/{}", all.len());
        }
    }
}

/// 絞り込み中も、日ごとの件数の累計が一覧の OFFSET になる（つまみで跳んだ先が合う）。
/// 日ごとの未読も、その一覧の未読と日ごとに一致する。
#[test]
fn 絞り込み中も日ごとの件数と未読が一覧と揃う() {
    let store = test_store();
    seed(&store);
    for account in [Some(1), None] {
        for f in cases() {
            assert_days_match(&store, account, &f);
        }
    }
}

/// 日ごとの件数の累計が一覧の OFFSET になり、日ごとの未読がその一覧の未読と日ごとに一致する。
fn assert_days_match(store: &Store, account: Option<i64>, f: &ThreadFilter) {
    let days = store
        .thread_day_counts_filtered(account, "inbox", f)
        .unwrap();
    let all = store
        .list_threads_filtered(account, "inbox", f, 10_000, 0)
        .unwrap();
    let total: i32 = days.iter().map(|d| d.count).sum();
    assert_eq!(total as usize, all.len(), "{f:?}");
    let mut offset = 0usize;
    let mut unread_expect = Vec::new();
    for d in &days {
        let n = d.count as usize;
        let page = store
            .list_threads_filtered(account, "inbox", f, n as i64, offset as i64)
            .unwrap();
        let ids: Vec<i32> = page.iter().map(|m| m.id).collect();
        let expect: Vec<i32> = all[offset..offset + n].iter().map(|m| m.id).collect();
        assert_eq!(ids, expect, "{f:?} @ {:?}", d.date_key);
        let k = page.iter().filter(|m| m.unread_count > 0).count();
        if k > 0 {
            unread_expect.push((d.date_key.clone(), k as i32));
        }
        offset += n;
    }
    let unread: Vec<_> = store
        .thread_day_unread_filtered(account, "inbox", f)
        .unwrap()
        .into_iter()
        .map(|u| (u.date_key, u.unread))
        .collect();
    assert_eq!(unread, unread_expect, "{f:?}");
}

/// 開いて既読にした読書中の行（`reading_id`）は、未読の絞り込みに同じ位置のまま残り、
/// 件数・OFFSET・未読も揃う。反転（未読でないもの）には出さず、未読以外の絞り込みでは効かない。
#[test]
fn 読書中の行は既読にしても未読の絞り込みに残る() {
    let store = test_store();
    seed(&store);
    let unread = toggles(&[ThreadToggle::Unread], false);
    let ids = |f: &ThreadFilter| -> Vec<i32> {
        store
            .list_threads_filtered(Some(1), "inbox", f, 10_000, 0)
            .unwrap()
            .iter()
            .map(|m| m.id)
            .collect()
    };
    let before = ids(&unread);
    let row = before[before.len() / 2];
    // スレッドを丸ごと既読にする（5 秒経って既読にしたのと同じ）。
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE emails SET is_read = 1
             WHERE id = ?1
                OR logical_thread_id = (SELECT logical_thread_id FROM emails WHERE id = ?1)",
            [row],
        )
        .unwrap();
    let reading = |f: ThreadFilter| ThreadFilter {
        reading_id: Some(i64::from(row)),
        ..f
    };
    // 読書中の印が無ければ消え、あれば同じ位置に残る。
    assert!(!ids(&unread).contains(&row));
    assert_eq!(ids(&reading(unread.clone())), before);
    for account in [Some(1), None] {
        assert_days_match(&store, account, &reading(unread.clone()));
    }
    // 反転（未読でないもの）には出さない（画面側も読書中の行を未読として当てる）。
    assert!(!ids(&reading(toggles(&[ThreadToggle::Unread], true))).contains(&row));
    // 未読以外の絞り込みでは効かない。
    let star = toggles(&[ThreadToggle::Star], false);
    assert_eq!(ids(&reading(star.clone())), ids(&star));
}

/// 絞り込みが無ければ、今までの問い合わせと同じ結果（既定値は何も絞らない）。
#[test]
fn 既定の絞り込みは何も絞らない() {
    let store = test_store();
    seed(&store);
    let none = ThreadFilter::default();
    let a = store.list_threads(Some(1), "inbox", 10_000, 0).unwrap();
    let b = store
        .list_threads_filtered(Some(1), "inbox", &none, 10_000, 0)
        .unwrap();
    assert_eq!(
        a.iter().map(|m| m.id).collect::<Vec<_>>(),
        b.iter().map(|m| m.id).collect::<Vec<_>>()
    );
    assert_eq!(a.len(), 60);
}

/// 代表の行に絞り込みをかけた問い合わせの計画（EXPLAIN QUERY PLAN の detail 列）。
fn plan_of(f: &ThreadFilter) -> Vec<String> {
    let store = test_store();
    let conn = store.read_conn.lock().unwrap();
    // ?1 = フォルダ、?2 = アカウント（指定なしでも番号は空けておく）、?3 から絞り込みの値。
    let fs = super::FilterSql::build(&conn, f, "inbox", "emails.", 3).unwrap();
    let sql = format!(
        "EXPLAIN QUERY PLAN SELECT id FROM emails
         WHERE folder = ?1 AND is_junk = 0 AND is_folder_rep = 1{}",
        fs.clause
    );
    let binds = fs.binds(vec!["inbox".to_owned().into()], None);
    let plan = conn
        .prepare(&sql)
        .unwrap()
        .query_map(rusqlite::params_from_iter(binds), |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    plan
}

/// 期間の絞り込みは、代表の並びの式索引（idx_emails_folder_rep_activity）の範囲で引く。
#[test]
fn 期間の絞り込みは並びの索引で引く() {
    let plan = plan_of(&ThreadFilter {
        date: Some(ThreadDateBounds {
            since: Some(BASE),
            until: Some(BASE + 3600),
        }),
        ..Default::default()
    });
    assert!(
        plan.iter()
            .any(|p| p.contains("idx_emails_folder_rep_activity") && p.contains('>')),
        "{plan:?}"
    );
}

/// 未読の絞り込みは、未読だけの部分索引（idx_emails_unread）から未読のスレッドを集める。
#[test]
fn 未読の絞り込みは未読の索引で引く() {
    let plan = plan_of(&toggles(&[ThreadToggle::Unread], false));
    assert!(
        plan.iter().any(|p| p.contains("idx_emails_unread")),
        "{plan:?}"
    );
}

/// `[計測]` 5 万通（約 3 万スレッド）の受信箱で、絞り込みごとに日ごとの件数・未読・先頭と奥のページを引く時間。
/// `cargo test --release --lib 計測_5_万通で絞り込み -- --ignored --nocapture`。
#[test]
#[ignore = "計測用（release で 1 分ほど）"]
fn 計測_5_万通で絞り込み() {
    use std::time::Instant;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("mail.db")).unwrap();
    {
        let mut conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'me@example.invalid','i','s')",
            [],
        )
        .unwrap();
        let tx = conn.transaction().unwrap();
        let mut n = 0;
        // 3 万スレッド。2 本に 1 本は返信が 1 通、5 本に 1 本はさらにもう 1 通（計 約 5.1 万通）。
        for i in 0..30_000_usize {
            n += 1;
            let root = n;
            let ts = BASE - (i as i64) * 1_200;
            let from = format!("p{}@d{}.invalid", i % 3_000, i % 400);
            let mut m = mail(1, root, Some(ts), &from, None);
            m.is_read = i % 13 != 0;
            m.has_attachments = i % 10 == 0;
            insert_email(&tx, &m).unwrap();
            for k in 0..(usize::from(i % 2 == 0) + usize::from(i % 5 == 0)) {
                n += 1;
                let mut r = mail(1, n, Some(ts + 60 * (k as i64 + 1)), &from, Some(root));
                r.is_read = (i + k) % 17 != 0;
                insert_email(&tx, &r).unwrap();
            }
        }
        tx.execute_batch(
            "WITH RECURSIVE c(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM c WHERE i < 499)
             INSERT INTO contacts (id, display_name, is_favorite) SELECT i + 1, 'c' || i, i % 10 = 0 FROM c;
             INSERT INTO contact_emails (contact_id, value) SELECT id, 'p' || (id * 6) || '@d' || ((id * 6) % 400) || '.invalid' FROM contacts;
             INSERT INTO green_domains (domain) VALUES ('d7.invalid'), ('d8.invalid');
             WITH RECURSIVE c(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM c WHERE i < 299)
             INSERT INTO sent_addresses (address, sent_count) SELECT 'p' || (i * 10) || '@d' || ((i * 10) % 400) || '.invalid', 1 FROM c;
             INSERT INTO tags (id, name) VALUES (1, '仕事'), (2, '家族');
             UPDATE emails SET is_flagged = 1 WHERE id % 31 = 0;
             INSERT INTO email_tags (email_id, tag_id) SELECT id, 1 + id % 2 FROM emails WHERE id % 47 = 0;",
        )
        .unwrap();
        // スレッドは SQL で直接組む（取り込みの割当を 5 万通に通すと、それだけで 10 分ほどかかる）。
        // 根は自分の id、返信は親の id を論理スレッドにし、各スレッドの最新を代表にする。
        tx.execute_batch(
            "INSERT INTO logical_threads (id) SELECT id FROM emails WHERE in_reply_to IS NULL;
             UPDATE emails SET logical_thread_id = CASE WHEN in_reply_to IS NULL THEN id
                 ELSE (SELECT p.id FROM emails p WHERE p.message_id = emails.in_reply_to) END;
             CREATE TEMP TABLE reps AS
               SELECT max(id) AS id FROM emails GROUP BY logical_thread_id;
             UPDATE emails SET is_folder_rep = (id IN (SELECT id FROM reps));
             DROP TABLE reps;",
        )
        .unwrap();
        tx.commit().unwrap();
    }
    use ThreadToggle::*;
    let mut fs = vec![("絞り込み無し", ThreadFilter::default())];
    for (name, t) in [
        ("未読", Unread),
        ("添付", Attachment),
        ("スター", Star),
        ("グリーン", Green),
        ("お気に入り", Vip),
        ("知り合い", Known),
        ("返信歴", Replied),
    ] {
        fs.push((name, toggles(&[t], false)));
    }
    fs.push((
        "反転: 未読でも知り合いでもない",
        toggles(&[Unread, Known], true),
    ));
    fs.push((
        "期間 (10 日)",
        ThreadFilter {
            date: Some(ThreadDateBounds {
                since: Some(BASE - 20 * 86_400),
                until: Some(BASE - 10 * 86_400),
            }),
            ..Default::default()
        },
    ));
    fs.push((
        "タグ",
        ThreadFilter {
            tag_ids: vec![1, 2],
            ..Default::default()
        },
    ));
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
    for (name, f) in &fs {
        let t = Instant::now();
        let days = store
            .thread_day_counts_filtered(Some(1), "inbox", f)
            .unwrap();
        let d_ms = ms(t);
        let total: i32 = days.iter().map(|d| d.count).sum();
        let t = Instant::now();
        store
            .thread_day_unread_filtered(Some(1), "inbox", f)
            .unwrap();
        let u_ms = ms(t);
        let t = Instant::now();
        store
            .list_threads_filtered(Some(1), "inbox", f, 100, 0)
            .unwrap();
        let first_ms = ms(t);
        let t = Instant::now();
        let deep = store
            .list_threads_filtered(Some(1), "inbox", f, 100, i64::from(total / 2))
            .unwrap();
        let deep_ms = ms(t);
        assert!(total == 0 || !deep.is_empty());
        eprintln!(
            "{name}: {total} 件 / 日ごと {d_ms:.0} ms / 未読 {u_ms:.0} ms / 先頭 {first_ms:.0} ms / 中ほど {deep_ms:.0} ms"
        );
    }
}
