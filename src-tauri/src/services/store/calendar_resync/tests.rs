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
