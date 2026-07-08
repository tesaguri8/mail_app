//! Google の予定（GEvent）⇄ ローカル表現（RemoteEvent / LocalChange）の相互変換。
//!
//! ローカルの日時は端末ローカルの素の文字列（終日='YYYY-MM-DD' / 時間指定='YYYY-MM-DDTHH:MM'）。
//! Google はオフセット付き RFC3339 なので、取り込み時は端末ローカルへ、送信時はローカルの
//! 素の時刻へ端末オフセットを付けて RFC3339 にする。終日の終了日は Google が排他日（翌日）を
//! 使うため、取り込みで -1 日、送信で +1 日する。

use super::api::GEvent;
use crate::services::store::{LocalChange, RemoteEvent};
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

/// Google の予定 → ローカル表現。取り込めない/対象外なら None。
pub fn remote_from_gevent(ev: &GEvent) -> Option<RemoteEvent> {
    let external_id = ev.id.clone()?;
    let cancelled = ev.status.as_deref() == Some("cancelled");
    if cancelled {
        // 削除は id だけで十分（他フィールドは無い場合がある）。
        return Some(RemoteEvent {
            external_id,
            cancelled: true,
            availability: "busy".into(),
            visibility: "default".into(),
            ..Default::default()
        });
    }
    // v1: 繰り返しの個別インスタンス上書きは取り込まない（マスターのみ扱う）。
    if ev.recurring_event_id.is_some() {
        return None;
    }
    let start = ev.start.as_ref()?;
    let (start_at, all_day) = if let Some(d) = &start.date {
        (d.clone(), true)
    } else if let Some(dt) = &start.date_time {
        (rfc3339_to_local(dt)?, false)
    } else {
        return None;
    };
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
    let reminder_minutes = ev
        .reminders
        .as_ref()
        .and_then(|r| r.overrides.as_ref())
        .and_then(|o| o.first())
        .and_then(|o| o.minutes)
        .map(|m| m as i32);

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
        availability,
        visibility,
        color: None,
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

    // リマインダー（分指定があればポップアップで上書き）
    if let Some(min) = c.reminder_minutes {
        m.insert(
            "reminders".into(),
            json!({ "useDefault": false, "overrides": [{ "method": "popup", "minutes": min }] }),
        );
    }

    Value::Object(m)
}
