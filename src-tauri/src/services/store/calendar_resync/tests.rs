use std::collections::HashSet;

use rusqlite::params;

use super::super::calendar_sync::{RecurringInstance, RemoteApplied, RemoteEvent};
use super::super::Store;
use super::FullPullReconciled;

fn remote(id: &str, title: &str, start: &str) -> RemoteEvent {
    RemoteEvent {
        external_id: id.into(),
        etag: Some(format!("\"{id}-1\"")),
        title: title.into(),
        start_at: start.into(),
        availability: "busy".into(),
        visibility: "default".into(),
        ..Default::default()
    }
}

fn calendar(s: &Store, ext: &str) -> i64 {
    let acct = s
        .upsert_google_account("owner@example.invalid", None, None)
        .unwrap();
    s.upsert_google_calendar(acct, ext, "予定表", None, "owner", true)
        .unwrap()
}

/// 予定（Google の ID）が手元で削除扱いか。
fn deleted(s: &Store, ext: &str) -> bool {
    let conn = s.conn.lock().unwrap();
    conn.query_row(
        "SELECT deleted_at IS NOT NULL FROM events WHERE external_id = ?1",
        params![ext],
        |r| r.get(0),
    )
    .unwrap()
}

fn seen(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

#[test]
fn 増分で消えた予定も全件で返ってくれば戻す() {
    let s = Store::open_in_memory_for_test();
    let cal = calendar(&s, "cal5");
    let ev = remote("e1", "打ち合わせ", "2026-11-12T10:00");
    assert_eq!(
        s.apply_remote_event_detail(cal, &ev).unwrap(),
        RemoteApplied::Inserted
    );
    // 増分で cancelled が届いた（vaio 2026-11-10 の状態）。
    let cancelled = RemoteEvent {
        cancelled: true,
        ..ev.clone()
    };
    assert_eq!(
        s.apply_remote_event_detail(cal, &cancelled).unwrap(),
        RemoteApplied::Deleted
    );
    assert!(deleted(&s, "e1"));
    // 全件の取り直しで、同じ版がふつうの予定として返ってきた → 戻す（同じ etag でも落とさない）。
    assert_eq!(
        s.apply_remote_event_detail(cal, &ev).unwrap(),
        RemoteApplied::Restored
    );
    assert!(!deleted(&s, "e1"));
    // もう一度来ても、同じ版なら何もしない。
    assert_eq!(
        s.apply_remote_event_detail(cal, &ev).unwrap(),
        RemoteApplied::Unchanged
    );
}

#[test]
fn 返ってこなかった予定は期間の中で未送信の変更が無いものだけ消す() {
    let s = Store::open_in_memory_for_test();
    let cal = calendar(&s, "cal5");
    let other = calendar(&s, "cal6");
    for (id, start) in [
        ("kept", "2026-11-12T10:00"),
        ("gone", "2026-10-29T09:00"),
        ("old", "2025-10-01T09:00"),
        ("edited", "2026-11-05T09:00"),
    ] {
        s.apply_remote_event(cal, &remote(id, id, start)).unwrap();
    }
    s.apply_remote_event(
        other,
        &remote("elsewhere", "別のカレンダー", "2026-11-01T09:00"),
    )
    .unwrap();
    // 手元で編集してまだ送っていない（dirty=1）。
    {
        let conn = s.conn.lock().unwrap();
        conn.execute(
            "UPDATE events SET dirty = 1 WHERE external_id = 'edited'",
            [],
        )
        .unwrap();
    }
    let r = s
        .reconcile_full_pull(cal, &seen(&["kept"]), "2025-11-14")
        .unwrap();
    assert_eq!(
        r,
        FullPullReconciled {
            removed: 1,
            uncancelled: 0
        }
    );
    assert!(!deleted(&s, "kept"));
    assert!(deleted(&s, "gone"), "期間の中で返ってこなかった");
    assert!(!deleted(&s, "old"), "期間より前には触らない");
    assert!(!deleted(&s, "edited"), "未送信の変更は消さない");
    assert!(!deleted(&s, "elsewhere"), "別のカレンダーには触らない");
    // 消したものは送信対象にしない（Google 側には既に無い）。
    assert!(s
        .list_local_changes(cal)
        .unwrap()
        .iter()
        .all(|c| !c.deleted));
}

#[test]
fn 本体が返ってこなければ1回だけ変更された回も消す() {
    let s = Store::open_in_memory_for_test();
    let cal = calendar(&s, "cal5");
    let master = RemoteEvent {
        recurrence: Some("FREQ=WEEKLY".into()),
        ..remote("m1", "定例", "2026-09-04T10:30")
    };
    s.apply_remote_event(cal, &master).unwrap();
    let moved = RemoteEvent {
        instance: Some(RecurringInstance {
            recurring_external_id: "m1".into(),
            original_start_at: "2026-10-09T10:30".into(),
        }),
        ..remote("m1_20261009", "定例", "2026-10-08T15:00")
    };
    s.apply_remote_event(cal, &moved).unwrap();
    let r = s
        .reconcile_full_pull(cal, &seen(&[]), "2025-11-14")
        .unwrap();
    assert_eq!(r.removed, 2);
    assert!(deleted(&s, "m1") && deleted(&s, "m1_20261009"));
}

#[test]
fn 返ってこなかった1回だけの削除の記録は期間の中だけ外す() {
    let s = Store::open_in_memory_for_test();
    let cal = calendar(&s, "cal5");
    let cancel = |id: &str, original: &str| RemoteEvent {
        cancelled: true,
        instance: Some(RecurringInstance {
            recurring_external_id: "m1".into(),
            original_start_at: original.into(),
        }),
        ..remote(id, "", original)
    };
    for (id, original) in [
        ("still", "2026-10-16T10:30"),
        ("restored", "2026-10-23T10:30"),
        ("old", "2025-06-06T10:30"),
    ] {
        assert_eq!(
            s.apply_remote_event_detail(cal, &cancel(id, original))
                .unwrap(),
            RemoteApplied::Deleted
        );
    }
    // 全件では「still」の削除だけが返ってきた（「restored」は Google 側で戻された）。
    let r = s
        .reconcile_full_pull(cal, &seen(&["still"]), "2025-11-14")
        .unwrap();
    assert_eq!(r.uncancelled, 1);
    let conn = s.conn.lock().unwrap();
    let mut left: Vec<String> = conn
        .prepare("SELECT external_id FROM event_cancelled_instances ORDER BY external_id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    left.sort();
    assert_eq!(left, vec!["old".to_string(), "still".to_string()]);
}

#[test]
fn 全件の取り直しは24時間ごと_手動の同期し直しで次に全件() {
    let s = Store::open_in_memory_for_test();
    let cal = calendar(&s, "cal5");
    let acct = s
        .upsert_google_account("owner@example.invalid", None, None)
        .unwrap();
    let due = |s: &Store| s.list_synced_google_calendars(acct).unwrap()[0].full_due;
    assert!(due(&s), "一度も取り直していなければ全件");
    s.mark_calendar_full_synced(cal).unwrap();
    s.set_calendar_sync_token(cal, Some("tok")).unwrap();
    assert!(!due(&s), "取り直した直後は増分");
    {
        let conn = s.conn.lock().unwrap();
        conn.execute(
            "UPDATE calendars SET last_full_sync_at = datetime('now', '-25 hours') WHERE id = ?1",
            params![cal],
        )
        .unwrap();
    }
    assert!(due(&s), "24 時間を超えたら全件");
    s.mark_calendar_full_synced(cal).unwrap();
    s.request_calendar_full_resync(cal).unwrap();
    let c = &s.list_synced_google_calendars(acct).unwrap()[0];
    assert!(
        c.full_due && c.sync_token.is_none(),
        "手動で同期し直すと、次は最初から"
    );
}

/// カレンダーの行と予定の件数（そのカレンダーに属するもの）。無ければ None。
fn calendar_events(s: &Store, id: i64) -> Option<i64> {
    let conn = s.conn.lock().unwrap();
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM calendars WHERE id = ?1)",
            params![id],
            |r| r.get(0),
        )
        .unwrap();
    exists.then(|| {
        conn.query_row(
            "SELECT COUNT(*) FROM events WHERE calendar_id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    })
}

#[test]
fn 一覧に無くなったカレンダーだけを予定ごと外す() {
    let s = Store::open_in_memory_for_test();
    let a = s
        .upsert_google_account("a@example.invalid", None, None)
        .unwrap();
    let b = s
        .upsert_google_account("b@example.invalid", None, None)
        .unwrap();
    let mine = s
        .upsert_google_calendar(a, "mine", "マイ", None, "owner", true)
        .unwrap();
    let gone = s
        .upsert_google_calendar(a, "gone", "当銘", None, "owner", false)
        .unwrap();
    let other = s
        .upsert_google_calendar(b, "gone", "別アカウント", None, "owner", true)
        .unwrap();
    let local = s
        .upsert_calendar(&crate::models::CalendarInput {
            id: None,
            name: "手元".into(),
            color: None,
            kind: None,
        })
        .unwrap()
        .id as i64;
    for (cal, id) in [(mine, "e-mine"), (gone, "e-gone"), (other, "e-other")] {
        s.apply_remote_event(cal, &remote(id, id, "2026-11-12T10:00"))
            .unwrap();
    }
    // 1 回だけの削除の記録も一緒に消える（CASCADE）。
    let cancelled = RemoteEvent {
        cancelled: true,
        instance: Some(RecurringInstance {
            recurring_external_id: "m".into(),
            original_start_at: "2026-11-20T10:00".into(),
        }),
        ..remote("m_1120", "", "2026-11-20T10:00")
    };
    s.apply_remote_event(gone, &cancelled).unwrap();

    let removed = s
        .prune_missing_google_calendars(a, &seen(&["mine"]))
        .unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(
        (removed[0].id, removed[0].name.as_str(), removed[0].events),
        (gone, "当銘", 1)
    );
    assert_eq!(calendar_events(&s, gone), None, "消えたカレンダーは外す");
    assert_eq!(calendar_events(&s, mine), Some(1), "一覧にあるものは残す");
    assert_eq!(
        calendar_events(&s, other),
        Some(1),
        "ほかのアカウントには触らない"
    );
    assert!(
        calendar_events(&s, local).is_some(),
        "Google 以外には触らない"
    );
    let conn = s.conn.lock().unwrap();
    let left: i64 = conn
        .query_row("SELECT COUNT(*) FROM event_cancelled_instances", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(left, 0);
    drop(conn);
    // 同期対象にも出ない。
    assert!(s
        .list_synced_google_calendars(a)
        .unwrap()
        .iter()
        .all(|c| c.local_id != gone));
}

#[test]
fn 一覧が空なら何も外さない() {
    let s = Store::open_in_memory_for_test();
    let cal = calendar(&s, "mine");
    let acct = s
        .upsert_google_account("owner@example.invalid", None, None)
        .unwrap();
    assert!(s
        .prune_missing_google_calendars(acct, &HashSet::new())
        .unwrap()
        .is_empty());
    assert!(calendar_events(&s, cal).is_some());
}

#[test]
fn 予定の取得が404なら外す_グーグル以外のカレンダーは外さない() {
    let s = Store::open_in_memory_for_test();
    let cal = calendar(&s, "gone");
    s.apply_remote_event(cal, &remote("e1", "e1", "2026-11-12T10:00"))
        .unwrap();
    let local = s
        .upsert_calendar(&crate::models::CalendarInput {
            id: None,
            name: "手元".into(),
            color: None,
            kind: None,
        })
        .unwrap()
        .id as i64;
    assert_eq!(s.remove_google_calendar(local).unwrap(), None);
    assert!(calendar_events(&s, local).is_some());
    let gone = s.remove_google_calendar(cal).unwrap().unwrap();
    assert_eq!((gone.external_id.as_str(), gone.events), ("gone", 1));
    assert_eq!(calendar_events(&s, cal), None);
    // 2 回目は何もしない。
    assert_eq!(s.remove_google_calendar(cal).unwrap(), None);
}
