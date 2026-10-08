//! Google の予定（GEvent）⇄ ローカル表現（RemoteEvent / LocalChange）の相互変換。
//!
//! ローカルの日時は端末ローカルの素の文字列（終日='YYYY-MM-DD' / 時間指定='YYYY-MM-DDTHH:MM'）。
//! Google はオフセット付き RFC3339 なので、取り込み時は端末ローカルへ、送信時はローカルの
//! 素の時刻へ端末オフセットを付けて RFC3339 にする。終日の終了日は Google が排他日（翌日）を
//! 使うため、取り込みで -1 日、送信で +1 日する。

use super::api::{GEvent, GTime};
use crate::services::store::{LocalChange, RecurringInstance, RemoteEvent};
use chrono::{Duration, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde_json::{json, Map, Value};

/// 'YYYY-MM-DDTHH:MM'（端末ローカル）→ RFC3339（端末オフセット付き）。
fn local_to_rfc3339(s: &str) -> Option<String> {
    let naive = NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%dT%H:%M").ok()?;
    Local
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.to_rfc3339())
}

/// RFC3339（オフセット付き）→ 'YYYY-MM-DDTHH:MM'（端末ローカル）。
fn rfc3339_to_local(s: &str) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(s.trim()).ok()?;
    Some(dt.with_timezone(&Local).format("%Y-%m-%dT%H:%M").to_string())
}

fn date_plus_days(date: &str, days: i64) -> Option<String> {
    let d = NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d").ok()?;
    Some((d + Duration::days(days)).format("%Y-%m-%d").to_string())
}

/// recurrence の配列から最初の RRULE 本体（"RRULE:" を除いた部分）を取り出す。
fn first_rrule(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|l| {
        let up = l.trim();
        up.strip_prefix("RRULE:")
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty())
    })
}

/// Google の日時（終日 date / 時間指定 dateTime）→ (ローカル表現, 終日か)。
fn gtime_to_local(t: &GTime) -> Option<(String, bool)> {
    match (&t.date, &t.date_time) {
        (Some(d), _) => Some((d.clone(), true)),
        (None, Some(dt)) => rfc3339_to_local(dt).map(|s| (s, false)),
        (None, None) => None,
    }
}

/// 例外インスタンス（recurringEventId ＋ originalStartTime）なら、本体との対応を返す。
fn instance_of(ev: &GEvent) -> Option<RecurringInstance> {
    let recurring_external_id = ev.recurring_event_id.clone()?;
    let (original_start_at, _) = gtime_to_local(ev.original_start_time.as_ref()?)?;
    Some(RecurringInstance {
        recurring_external_id,
        original_start_at,
    })
}

/// Google の予定 → ローカル表現。取り込めない/対象外なら None。
///
/// 繰り返しの例外インスタンス（1 回だけ変更・削除された回）は `instance` に本体との対応を
/// 持たせて返す。本体側の展開では、その回を出さずに例外の方を出す（削除なら何も出さない）。
pub fn remote_from_gevent(ev: &GEvent) -> Option<RemoteEvent> {
    let external_id = ev.id.clone()?;
    let instance = instance_of(ev);
    let cancelled = ev.status.as_deref() == Some("cancelled");
    if cancelled {
        // 削除は id（と例外なら本体との対応）だけで十分（他フィールドは無い場合がある）。
        return Some(RemoteEvent {
            external_id,
            cancelled: true,
            instance,
            availability: "busy".into(),
            visibility: "default".into(),
            ..Default::default()
        });
    }
    // 元の回が分からない例外は、本体のどの回を置き換えるか決められないので取り込まない
    // （単独の予定として出すと、本体の展開と二重に出る）。
    if ev.recurring_event_id.is_some() && instance.is_none() {
        return None;
    }
    let (start_at, all_day) = gtime_to_local(ev.start.as_ref()?)?;
    let end_at = ev.end.as_ref().and_then(|t| {
        if all_day {
            // 排他日（翌日）→ 含む最終日へ。単日なら start と同じ日になり、None 相当。
            t.date.as_ref().and_then(|d| date_plus_days(d, -1))
        } else {
            t.date_time.as_ref().and_then(|dt| rfc3339_to_local(dt))
        }
    });
    // 単日終日で end == start のときは end_at を落として単日表示にする。
    let end_at = match &end_at {
        Some(e) if all_day && *e == start_at => None,
        _ => end_at,
    };

    let recurrence = ev.recurrence.as_ref().and_then(|l| first_rrule(l));
    let availability = match ev.transparency.as_deref() {
        Some("transparent") => "free",
        _ => "busy",
    }
    .to_string();
    let visibility = match ev.visibility.as_deref() {
        Some("public") => "public",
        Some("private") | Some("confidential") => "private",
        _ => "default",
    }
    .to_string();
    // Google の override をすべて取り込む（分単位・昇順・重複除去）。
    // 代表値 reminder_minutes は最小（最も早い通知）。useDefault のみ（override 無し）は空。
    let reminders: Vec<i32> = ev
        .reminders
        .as_ref()
        .and_then(|r| r.overrides.as_ref())
        .map(|o| {
            let mut v: Vec<i32> = o.iter().filter_map(|x| x.minutes).map(|m| m as i32).collect();
            v.sort_unstable();
            v.dedup();
            v
        })
        .unwrap_or_default();
    let reminder_minutes = reminders.first().copied();

    Some(RemoteEvent {
        external_id,
        etag: ev.etag.clone(),
        cancelled: false,
        title: ev.summary.clone().unwrap_or_default(),
        description: ev.description.clone(),
        location: ev.location.clone(),
        start_at,
        end_at,
        all_day,
        recurrence,
        reminder_minutes,
        reminders,
        availability,
        visibility,
        color: None,
        instance,
    })
}

fn opt(map: &mut Map<String, Value>, key: &str, val: &Option<String>) {
    if let Some(v) = val {
        let t = v.trim();
        if !t.is_empty() {
            map.insert(key.into(), json!(t));
        }
    }
}

/// ローカル変更 → Google へ送る予定 JSON（insert/patch 共通ボディ）。
pub fn gevent_write_from_local(c: &LocalChange) -> Value {
    let mut m = Map::new();
    m.insert("summary".into(), json!(c.title.trim()));
    opt(&mut m, "description", &c.description);
    opt(&mut m, "location", &c.location);

    // start / end
    if c.all_day {
        m.insert("start".into(), json!({ "date": c.start_at.trim() }));
        // 終了は排他日（含む最終日 + 1 日）。end_at 未指定なら start の翌日。
        let last = c
            .end_at
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| c.start_at.trim());
        let end_excl = date_plus_days(last, 1).unwrap_or_else(|| last.to_string());
        m.insert("end".into(), json!({ "date": end_excl }));
    } else {
        let start_rfc = local_to_rfc3339(&c.start_at).unwrap_or_else(|| c.start_at.clone());
        let end_rfc = c
            .end_at
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .and_then(local_to_rfc3339)
            .or_else(|| {
                // 終了未指定は開始 + 1 時間（Google は end 必須）。
                let naive =
                    NaiveDateTime::parse_from_str(c.start_at.trim(), "%Y-%m-%dT%H:%M").ok()?;
                Local
                    .from_local_datetime(&(naive + Duration::hours(1)))
                    .single()
                    .map(|dt| dt.to_rfc3339())
            })
            .unwrap_or_else(|| start_rfc.clone());
        m.insert("start".into(), json!({ "dateTime": start_rfc }));
        m.insert("end".into(), json!({ "dateTime": end_rfc }));
    }

    // 繰り返し（RRULE）
    if let Some(r) = c.recurrence.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        m.insert("recurrence".into(), json!([format!("RRULE:{r}")]));
    }

    // 予定あり/なし（Busy/Free）
    m.insert(
        "transparency".into(),
        json!(if c.availability == "free" {
            "transparent"
        } else {
            "opaque"
        }),
    );

    // 公開設定
    let vis = match c.visibility.as_str() {
        "public" => "public",
        "private" => "private",
        _ => "default",
    };
    m.insert("visibility".into(), json!(vis));

    // リマインダー（全通知をポップアップの override として送る）。空のときは reminders を
    // 付けない＝Google 側の設定（既定通知など）に触れない（従来の単一通知時と同じ方針）。
    if !c.reminders.is_empty() {
        let overrides: Vec<Value> = c
            .reminders
            .iter()
            .map(|min| json!({ "method": "popup", "minutes": min }))
            .collect();
        m.insert(
            "reminders".into(),
            json!({ "useDefault": false, "overrides": overrides }),
        );
    }

    Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(d: &str) -> Option<GTime> {
        Some(GTime {
            date: Some(d.into()),
            ..Default::default()
        })
    }

    #[test]
    fn modified_instance_keeps_link_to_master() {
        let ev = GEvent {
            id: Some("abc_20261009".into()),
            status: Some("confirmed".into()),
            summary: Some("定例".into()),
            start: date("2026-10-08"),
            end: date("2026-10-09"),
            recurring_event_id: Some("abc".into()),
            original_start_time: date("2026-10-09"),
            ..Default::default()
        };
        let re = remote_from_gevent(&ev).expect("例外は取り込む");
        assert_eq!(re.start_at, "2026-10-08");
        assert!(re.recurrence.is_none());
        let inst = re.instance.expect("本体との対応を持つ");
        assert_eq!(inst.recurring_external_id, "abc");
        assert_eq!(inst.original_start_at, "2026-10-09");
    }

    #[test]
    fn cancelled_instance_carries_original_start() {
        // 1 回だけ削除された回は id・recurringEventId・originalStartTime しか来ない。
        let ev = GEvent {
            id: Some("abc_20261016".into()),
            status: Some("cancelled".into()),
            recurring_event_id: Some("abc".into()),
            original_start_time: date("2026-10-16"),
            ..Default::default()
        };
        let re = remote_from_gevent(&ev).expect("削除も取り込む");
        assert!(re.cancelled);
        assert_eq!(
            re.instance.map(|i| i.original_start_at).as_deref(),
            Some("2026-10-16")
        );
    }

    #[test]
    fn instance_without_original_start_is_skipped() {
        let ev = GEvent {
            id: Some("abc_x".into()),
            start: date("2026-10-08"),
            recurring_event_id: Some("abc".into()),
            ..Default::default()
        };
        assert!(remote_from_gevent(&ev).is_none());
    }

    #[test]
    fn master_has_no_instance() {
        let ev = GEvent {
            id: Some("abc".into()),
            start: date("2026-10-02"),
            recurrence: Some(vec!["RRULE:FREQ=WEEKLY;WKST=MO".into()]),
            ..Default::default()
        };
        let re = remote_from_gevent(&ev).expect("本体は取り込む");
        assert!(re.instance.is_none());
        assert_eq!(re.recurrence.as_deref(), Some("FREQ=WEEKLY;WKST=MO"));
    }
}
