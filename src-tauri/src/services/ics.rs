//! iCalendar (.ics) の最小パーサ/ジェネレータ（Google カレンダー相互運用）。
//! 日時は端末ローカルの素の ISO（終日='YYYY-MM-DD' / 時間指定='YYYY-MM-DDTHH:MM'）へ正規化する。
//! docs/IMPORT_EXPORT.md。

use crate::models::EventSummary;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};

/// 取り込んだ 1 件の予定（挿入前の中間表現）。
pub struct ParsedEvent {
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_at: String,
    pub end_at: Option<String>,
    pub all_day: bool,
    pub recurrence: Option<String>,
}

/// ICS テキストを VEVENT 単位で解析する。
pub fn parse(text: &str) -> Vec<ParsedEvent> {
    let mut out = Vec::new();
    let mut cur: Option<Draft> = None;
    for line in unfold(text) {
        let l = line.trim_end();
        if l.eq_ignore_ascii_case("BEGIN:VEVENT") {
            cur = Some(Draft::default());
        } else if l.eq_ignore_ascii_case("END:VEVENT") {
            if let Some(d) = cur.take() {
                if let Some(ev) = d.finish() {
                    out.push(ev);
                }
            }
        } else if let Some(d) = cur.as_mut() {
            d.feed(l);
        }
    }
    out
}

/// 予定群を VCALENDAR テキストへ書き出す。
pub fn generate(events: &[EventSummary]) -> String {
    let mut s = String::new();
    s.push_str("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Tesaguri//Rondine//EN\r\nCALSCALE:GREGORIAN\r\n");
    let stamp = Local::now().format("%Y%m%dT%H%M%S").to_string();
    for e in events {
        s.push_str("BEGIN:VEVENT\r\n");
        s.push_str(&format!("UID:rondine-{}@tesaguri.rondine\r\n", e.id));
        s.push_str(&format!("DTSTAMP:{stamp}\r\n"));
        push_dt(&mut s, "DTSTART", &e.start_at, e.all_day, false);
        if let Some(end) = &e.end_at {
            push_dt(&mut s, "DTEND", end, e.all_day, true);
        }
        s.push_str(&format!("SUMMARY:{}\r\n", escape(&e.title)));
        if let Some(loc) = e.location.as_deref().filter(|v| !v.is_empty()) {
            s.push_str(&format!("LOCATION:{}\r\n", escape(loc)));
        }
        if let Some(d) = e.description.as_deref().filter(|v| !v.is_empty()) {
            s.push_str(&format!("DESCRIPTION:{}\r\n", escape(d)));
        }
        if let Some(r) = e.recurrence.as_deref().filter(|v| !v.is_empty()) {
            s.push_str(&format!("RRULE:{r}\r\n"));
        }
        s.push_str("END:VEVENT\r\n");
    }
    s.push_str("END:VCALENDAR\r\n");
    s
}

#[derive(Default)]
struct Draft {
    title: Option<String>,
    description: Option<String>,
    location: Option<String>,
    start_at: Option<String>,
    end_at: Option<String>,
    all_day: bool,
    recurrence: Option<String>,
}

impl Draft {
    fn feed(&mut self, line: &str) {
        let (head, value) = match line.split_once(':') {
            Some(x) => x,
            None => return,
        };
        let mut parts = head.split(';');
        let name = parts.next().unwrap_or("").to_ascii_uppercase();
        let params: Vec<&str> = parts.collect();
        match name.as_str() {
            "SUMMARY" => self.title = Some(unescape(value)),
            "DESCRIPTION" => self.description = Some(unescape(value)),
            "LOCATION" => self.location = Some(unescape(value)),
            "RRULE" => self.recurrence = Some(value.trim().to_string()),
            "DTSTART" => {
                let (iso, all_day) = to_iso(value, &params);
                self.start_at = iso;
                self.all_day = all_day;
            }
            "DTEND" => {
                let (iso, all_day) = to_iso(value, &params);
                // 終日の DTEND は排他（翌日）なので 1 日戻して包含終端に。
                self.end_at = iso.map(|s| if all_day { minus_one_day(&s) } else { s });
            }
            _ => {}
        }
    }

    fn finish(self) -> Option<ParsedEvent> {
        let start_at = self.start_at?;
        let title = self
            .title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Untitled".to_string());
        Some(ParsedEvent {
            title,
            description: self.description,
            location: self.location,
            start_at,
            end_at: self.end_at,
            all_day: self.all_day,
            recurrence: self.recurrence,
        })
    }
}

/// 折り返し（継続行が空白/タブ始まり）を結合して 1 行ずつに。
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r');
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&line[1..]);
        } else {
            out.push(line.to_string());
        }
    }
    out
}

/// ICS の日付/日時値を、端末ローカルの素の ISO（＋終日フラグ）へ。
fn to_iso(value: &str, params: &[&str]) -> (Option<String>, bool) {
    let v = value.trim();
    let is_date = params.iter().any(|p| p.eq_ignore_ascii_case("VALUE=DATE"))
        || (v.len() == 8 && v.chars().all(|c| c.is_ascii_digit()));
    if is_date {
        if v.len() >= 8 {
            return (Some(format!("{}-{}-{}", &v[0..4], &v[4..6], &v[6..8])), true);
        }
        return (None, true);
    }
    if let Some(ti) = v.find('T') {
        let d = &v[..ti];
        let rest = &v[ti + 1..];
        if d.len() >= 8 && rest.len() >= 4 {
            let (y, mo, da) = (&d[0..4], &d[4..6], &d[6..8]);
            let (hh, mi) = (&rest[0..2], &rest[2..4]);
            if rest.ends_with('Z') {
                // UTC → ローカル
                if let Ok(naive) =
                    NaiveDateTime::parse_from_str(&format!("{y}{mo}{da}T{hh}{mi}00Z"), "%Y%m%dT%H%M%SZ")
                {
                    let local = Utc.from_utc_datetime(&naive).with_timezone(&Local);
                    return (
                        Some(format!(
                            "{:04}-{:02}-{:02}T{:02}:{:02}",
                            local.year(),
                            local.month(),
                            local.day(),
                            local.hour(),
                            local.minute()
                        )),
                        false,
                    );
                }
            }
            // TZID 付き/floating はローカル壁時計として扱う。
            return (Some(format!("{y}-{mo}-{da}T{hh}:{mi}")), false);
        }
    }
    (None, false)
}

fn minus_one_day(s: &str) -> String {
    if let Ok(d) = NaiveDate::parse_from_str(&s[..10.min(s.len())], "%Y-%m-%d") {
        let p = d - Duration::days(1);
        return format!("{:04}-{:02}-{:02}", p.year(), p.month(), p.day());
    }
    s.to_string()
}

/// DTSTART/DTEND を書き出す（終日は VALUE=DATE、終端は排他へ+1日）。
fn push_dt(s: &mut String, name: &str, iso: &str, all_day: bool, is_end: bool) {
    if all_day {
        let d = &iso[..10.min(iso.len())];
        let day = if is_end {
            plus_one_compact(d)
        } else {
            d.replace('-', "")
        };
        s.push_str(&format!("{name};VALUE=DATE:{day}\r\n"));
    } else {
        let digits: String = iso.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() >= 12 {
            s.push_str(&format!("{name}:{}T{}00\r\n", &digits[0..8], &digits[8..12]));
        } else if digits.len() >= 8 {
            s.push_str(&format!("{name}:{}T000000\r\n", &digits[0..8]));
        }
    }
}

fn plus_one_compact(d: &str) -> String {
    if let Ok(nd) = NaiveDate::parse_from_str(d, "%Y-%m-%d") {
        let p = nd + Duration::days(1);
        return format!("{:04}{:02}{:02}", p.year(), p.month(), p.day());
    }
    d.replace('-', "")
}

fn escape(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\r', "")
        .replace('\n', "\\n")
}

fn unescape(v: &str) -> String {
    let mut s = String::new();
    let mut chars = v.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => s.push('\n'),
                Some(',') => s.push(','),
                Some(';') => s.push(';'),
                Some('\\') => s.push('\\'),
                Some(other) => s.push(other),
                None => {}
            }
        } else {
            s.push(c);
        }
    }
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timed_all_day_and_rrule() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:会議\r\nDTSTART:20260706T100000\r\nDTEND:20260706T110000\r\nRRULE:FREQ=WEEKLY\r\nLOCATION:東京\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:旅行\r\nDTSTART;VALUE=DATE:20260720\r\nDTEND;VALUE=DATE:20260723\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let evs = parse(ics);
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].start_at, "2026-07-06T10:00");
        assert_eq!(evs[0].end_at.as_deref(), Some("2026-07-06T11:00"));
        assert_eq!(evs[0].recurrence.as_deref(), Some("FREQ=WEEKLY"));
        assert!(!evs[0].all_day);
        // 終日は DTEND 排他(23日)→包含(22日)へ
        assert!(evs[1].all_day);
        assert_eq!(evs[1].start_at, "2026-07-20");
        assert_eq!(evs[1].end_at.as_deref(), Some("2026-07-22"));
    }

    #[test]
    fn roundtrip_generate_then_parse() {
        let e = EventSummary {
            id: 1,
            title: "打ち合わせ; 重要".into(),
            description: Some("メモ\n2行目".into()),
            location: Some("会議室A".into()),
            start_at: "2026-07-06T09:30".into(),
            end_at: Some("2026-07-06T10:30".into()),
            all_day: false,
            color: None,
            recurrence: None,
            reminder_minutes: None,
            related_email_id: None,
            deleted_at: None,
            calendar_id: None,
            availability: "busy".into(),
            visibility: "default".into(),
        };
        let ics = generate(std::slice::from_ref(&e));
        let back = parse(&ics);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].title, "打ち合わせ; 重要");
        assert_eq!(back[0].start_at, "2026-07-06T09:30");
        assert_eq!(back[0].end_at.as_deref(), Some("2026-07-06T10:30"));
        assert_eq!(back[0].location.as_deref(), Some("会議室A"));
        assert_eq!(back[0].description.as_deref(), Some("メモ\n2行目"));
    }
}
