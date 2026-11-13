//! Google の予定 ID はカレンダーをまたいで一意ではない（招待された予定は主催者と招待された側の
//! 両方に同じ ID で載る・カレンダーを移しても ID は変わらず、移動元には同じ ID が cancelled で載る）。
//! 手元の予定は (calendar_id, external_id) で引き、カレンダーごとに独立して保つこと。

use std::collections::HashSet;

use rusqlite::params;

use super::super::calendar_sync::{RecurringInstance, RemoteApplied, RemoteEvent};
use super::super::Store;

fn remote(id: &str, start: &str) -> RemoteEvent {
    RemoteEvent {
        external_id: id.into(),
        etag: Some(format!("\"{id}-1\"")),
        title: "隔週の定例".into(),
        start_at: start.into(),
        availability: "busy".into(),
        visibility: "default".into(),
        ..Default::default()
    }
}

/// 隔週木曜の本体（vaio の events 719 と同じ形）。
fn master() -> RemoteEvent {
    RemoteEvent {
        recurrence: Some("FREQ=WEEKLY;INTERVAL=2;BYDAY=TH".into()),
        ..remote("m719", "2026-10-01T10:00")
    }
}

/// 本体の 1 回だけ変更された回（10/15 の回を 10/16 へ）。
fn modified() -> RemoteEvent {
    RemoteEvent {
        instance: Some(RecurringInstance {
            recurring_external_id: "m719".into(),
            original_start_at: "2026-10-15T10:00".into(),
        }),
        ..remote("m719_20261015", "2026-10-16T10:00")
    }
}

/// 本体の 1 回だけ削除された回（10/29）。
fn skipped() -> RemoteEvent {
    RemoteEvent {
        cancelled: true,
        instance: Some(RecurringInstance {
            recurring_external_id: "m719".into(),
            original_start_at: "2026-10-29T10:00".into(),
        }),
        ..remote("m719_20261029", "")
    }
}

/// 同じ ID の cancelled。
fn cancelled(ev: RemoteEvent) -> RemoteEvent {
    RemoteEvent {
        cancelled: true,
        ..ev
    }
}

fn calendars(s: &Store) -> (i64, i64) {
    let acct = s
        .upsert_google_account("owner@example.invalid", None, None)
        .unwrap();
    let a = s
        .upsert_google_calendar(acct, "cal_a", "移動元", None, "owner", true)
        .unwrap();
    let b = s
        .upsert_google_calendar(acct, "cal_b", "移動先", None, "owner", true)
        .unwrap();
    (a, b)
}

/// その ID の、削除されていない行があるカレンダー（行の数だけ並ぶ）。
fn alive_in(s: &Store, ext: &str) -> Vec<i64> {
    let conn = s.conn.lock().unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT calendar_id FROM events \
             WHERE external_id = ?1 AND deleted_at IS NULL ORDER BY calendar_id",
        )
        .unwrap();
    stmt.query_map(params![ext], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// そのカレンダーの、その ID の行の題名。
fn title_in(s: &Store, cal: i64, ext: &str) -> String {
    let conn = s.conn.lock().unwrap();
    conn.query_row(
        "SELECT title FROM events WHERE calendar_id = ?1 AND external_id = ?2",
        params![cal, ext],
        |r| r.get(0),
    )
    .unwrap()
}

/// そのカレンダーの本体の、展開から除かれる回（変更・削除された回の元の開始）。
fn exdates(s: &Store, cal: i64) -> Vec<String> {
    let list = s.list_events("2026-10-01", "2026-11-30", false).unwrap();
    let m = list
        .iter()
        .find(|e| e.recurrence.is_some() && e.calendar_id.map(i64::from) == Some(cal))
        .expect("そのカレンダーに本体が出る");
    let mut v = m.exdates.clone();
    v.sort();
    v
}

/// 全件の取り直し 1 回分（反映して、返ってこなかったものを片付ける）。
fn full_pull(s: &Store, cal: i64, evs: &[RemoteEvent]) -> Vec<RemoteApplied> {
    let out = evs
        .iter()
        .map(|e| s.apply_remote_event_detail(cal, e).unwrap())
        .collect();
    let seen: HashSet<String> = evs.iter().map(|e| e.external_id.clone()).collect();
    s.reconcile_full_pull(cal, &seen, "2025-11-14").unwrap();
    out
}

/// 移動元 A の全件（本体・変更された回が cancelled で載る。削除された回もそのまま）。
fn source_after_move() -> Vec<RemoteEvent> {
    vec![cancelled(master()), cancelled(modified()), skipped()]
}

/// 移動先 B の全件。
fn dest_after_move() -> Vec<RemoteEvent> {
    vec![master(), modified(), skipped()]
}

/// 移す前: A に本体・変更された回・削除された回がある。
fn seed(s: &Store, cal: i64) {
    full_pull(s, cal, &dest_after_move());
}

fn assert_only_in(s: &Store, b: i64) {
    assert_eq!(alive_in(s, "m719"), vec![b], "本体は移動先に 1 行だけ");
    assert_eq!(
        alive_in(s, "m719_20261015"),
        vec![b],
        "変更された回も移動先に 1 行だけ"
    );
    assert_eq!(
        exdates(s, b),
        vec![
            "2026-10-15T10:00".to_string(),
            "2026-10-29T10:00".to_string()
        ],
        "変更・削除された回は、移動先でも展開から除かれる"
    );
}

#[test]
fn 移動は_移動元を先に取り込んでも後に取り込んでも_移動先に1行だけ残る() {
    for source_first in [true, false] {
        let s = Store::open_in_memory_for_test();
        let (a, b) = calendars(&s);
        seed(&s, a);
        if source_first {
            full_pull(&s, a, &source_after_move());
            assert!(alive_in(&s, "m719").is_empty(), "A の行は消える");
            full_pull(&s, b, &dest_after_move());
        } else {
            // vaio 2026-11-13 04:44:40（cal 5 = B）→ 04:44:41（cal 6 = A）の順。
            let out = full_pull(&s, b, &dest_after_move());
            assert_eq!(
                out[0],
                RemoteApplied::Inserted,
                "B には新しい行ができる（付け替えない）"
            );
            assert_eq!(alive_in(&s, "m719"), vec![a, b]);
            full_pull(&s, a, &source_after_move());
        }
        assert_only_in(&s, b);
        // 翌日の取り直しでも変わらない（A の cancelled が B に及ばない）。
        full_pull(&s, a, &source_after_move());
        assert_eq!(
            full_pull(&s, b, &dest_after_move())[0],
            RemoteApplied::Unchanged
        );
        assert_only_in(&s, b);
    }
}

#[test]
fn 同じidが2つのカレンダーにあれば_2行が独立して保たれる() {
    let s = Store::open_in_memory_for_test();
    let (a, b) = calendars(&s);
    let ev = remote("inv1", "2026-11-20T15:00");
    assert_eq!(
        s.apply_remote_event_detail(a, &ev).unwrap(),
        RemoteApplied::Inserted
    );
    assert_eq!(
        s.apply_remote_event_detail(b, &ev).unwrap(),
        RemoteApplied::Inserted
    );
    assert_eq!(alive_in(&s, "inv1"), vec![a, b]);

    // 行き来しない: もう一度同じ版が来ても、どちらも「変化なし」。
    assert_eq!(
        s.apply_remote_event_detail(a, &ev).unwrap(),
        RemoteApplied::Unchanged
    );
    assert_eq!(
        s.apply_remote_event_detail(b, &ev).unwrap(),
        RemoteApplied::Unchanged
    );

    // 片方の版が変わっても、もう片方は書き換えない。
    let changed = RemoteEvent {
        etag: Some("\"inv1-2\"".into()),
        title: "招待（変更）".into(),
        ..ev.clone()
    };
    assert_eq!(
        s.apply_remote_event_detail(b, &changed).unwrap(),
        RemoteApplied::Updated
    );
    assert_eq!(title_in(&s, a, "inv1"), "隔週の定例");
    assert_eq!(title_in(&s, b, "inv1"), "招待（変更）");

    // 全件の取り直しで、片方に返ってこなくても、もう片方は残る。
    full_pull(&s, a, &[]);
    assert_eq!(alive_in(&s, "inv1"), vec![b]);
}

#[test]
fn 一方の取り消しは_もう一方に及ばない() {
    let s = Store::open_in_memory_for_test();
    let (a, b) = calendars(&s);
    seed(&s, a);
    seed(&s, b);

    // A で本体が取り消された → A の本体と変更された回だけが消える。
    assert_eq!(
        s.apply_remote_event_detail(a, &cancelled(master()))
            .unwrap(),
        RemoteApplied::Deleted
    );
    assert_eq!(alive_in(&s, "m719"), vec![b]);
    assert_eq!(alive_in(&s, "m719_20261015"), vec![b]);

    // A で 1 回だけ削除された回は、B の展開に効かない。
    let mut once = skipped();
    once.external_id = "m719_20261112".into();
    once.instance = Some(RecurringInstance {
        recurring_external_id: "m719".into(),
        original_start_at: "2026-11-12T10:00".into(),
    });
    s.apply_remote_event_detail(a, &once).unwrap();
    assert_eq!(
        exdates(&s, b),
        vec![
            "2026-10-15T10:00".to_string(),
            "2026-10-29T10:00".to_string()
        ]
    );
}

#[test]
fn 移動元の通知で消えた予定は_移動先の全件の取り直しで戻る() {
    // vaio の 719 の今の状態: 直す前に、cal 5（B）の行が cal 6（A）の cancelled で消えた。
    let s = Store::open_in_memory_for_test();
    let (a, b) = calendars(&s);
    s.apply_remote_event_detail(b, &master()).unwrap();
    {
        let conn = s.conn.lock().unwrap();
        conn.execute(
            "UPDATE events SET deleted_at = CURRENT_TIMESTAMP WHERE external_id = 'm719'",
            [],
        )
        .unwrap();
    }
    // A の cancelled は、A に行が無いので何もしない（B の行に触らない）。
    assert_eq!(
        s.apply_remote_event_detail(a, &cancelled(master()))
            .unwrap(),
        RemoteApplied::Unchanged
    );
    assert!(alive_in(&s, "m719").is_empty());
    // B の全件の取り直しで、Google から返ってきたので戻す（同じ etag でも落とさない）。
    assert_eq!(full_pull(&s, b, &[master()])[0], RemoteApplied::Restored);
    assert_eq!(alive_in(&s, "m719"), vec![b]);
}

#[test]
fn 移動元に未送信の変更があっても_取り消しを優先し_中身はゴミ箱に残す() {
    let s = Store::open_in_memory_for_test();
    let (a, b) = calendars(&s);
    seed(&s, a);
    {
        let conn = s.conn.lock().unwrap();
        conn.execute(
            "UPDATE events SET title = '手元で直した題', dirty = 1 \
             WHERE calendar_id = ?1 AND external_id = 'm719'",
            params![a],
        )
        .unwrap();
    }
    full_pull(&s, a, &source_after_move());
    full_pull(&s, b, &dest_after_move());
    assert_only_in(&s, b);
    // A の行は送信対象から外れてゴミ箱にあり、手元で直した題はそこに残る。
    assert!(s.list_local_changes(a).unwrap().is_empty());
    assert_eq!(title_in(&s, a, "m719"), "手元で直した題");
    assert!(s
        .list_trashed_events()
        .unwrap()
        .iter()
        .any(|e| e.title == "手元で直した題"));
}
