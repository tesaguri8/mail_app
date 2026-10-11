//! 引用の見出しにある日時（"Sent: Friday, October 2, 2026 5:55 PM" / "送信日時: 2026年10月2日 17:55" 等）を読む。
//!
//! 書式はクライアントと言語ごとにばらばらなので、決まった書式を当てるのではなく、
//! 字句（年・月・日・時刻・午前午後・時差）を拾って組み立てる。読めなければ None。

use chrono::{NaiveDate, NaiveDateTime};

/// 引用の見出しから読んだ日時。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotedTime {
    /// 書かれていたままの時刻（時差は当てていない）。
    pub local: NaiveDateTime,
    /// 時差（秒。"+0900" なら 32400）。書かれていなければ None（Outlook の "Sent:" は書かない）。
    pub offset_secs: Option<i32>,
}

/// 英語の月名（先頭 3 文字で引く）。
const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// 時刻の字句（"17:55" / "5:55:12"）を (時, 分, 秒) に読む。
fn parse_clock(tok: &str) -> Option<(u32, u32, u32)> {
    let mut parts = tok.split(':');
    let h = parts.next()?.parse().ok()?;
    let m = parts.next()?.parse().ok()?;
    let s = match parts.next() {
        Some(s) => s.parse().ok()?,
        None => 0,
    };
    (parts.next().is_none() && h < 24 && m < 60 && s < 61).then_some((h, m, s))
}

/// 時差の字句（"+0900" / "-0700" / "+09:00"）を秒に読む。
fn parse_offset(tok: &str) -> Option<i32> {
    let (sign, rest) = match tok.as_bytes().first()? {
        b'+' => (1, &tok[1..]),
        b'-' => (-1, &tok[1..]),
        _ => return None,
    };
    let digits: String = rest.chars().filter(|c| *c != ':').collect();
    if digits.len() != 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let h: i32 = digits[..2].parse().ok()?;
    let m: i32 = digits[2..].parse().ok()?;
    (h <= 14 && m < 60).then_some(sign * (h * 3600 + m * 60))
}

/// 引用の見出しの日時を読む。年・月・日・時刻がそろわなければ None。
///
/// 読める例: "Friday, October 2, 2026 5:55 PM" / "Fri, 2 Oct 2026 17:55:00 +0900" /
/// "Fri, Oct 2, 2026 at 5:55 PM" / "2026年10月2日(金) 17:55" / "2026/10/02 17:55" / "2026年10月2日 午後5:55"。
/// 数字だけの日付は「年が先」（年/月/日）のときだけ読む（"10/02/2026" は月日の順が決まらない）。
pub fn parse_quoted_time(text: &str) -> Option<QuotedTime> {
    // 区切りを空白にそろえる。'-' は数字の直後（"2026-10-02"）だけ区切りとみなし、
    // 時差の符号（" -0700"）は残す。
    let mut prev = ' ';
    let spaced: String = text
        .chars()
        .map(|c| {
            let out = match c {
                ',' | '(' | ')' | '（' | '）' | '年' | '月' | '日' | '/' => ' ',
                '-' if prev.is_ascii_digit() => ' ',
                '：' => ':',
                _ => c,
            };
            prev = c;
            out
        })
        .collect();

    let mut year: Option<i32> = None;
    let mut month_name: Option<u32> = None;
    // 1〜2 桁の数字を、年より前に出たものと後に出たものに分けて取る。
    let mut before_year: Vec<u32> = Vec::new();
    let mut after_year: Vec<u32> = Vec::new();
    let mut clock: Option<(u32, u32, u32)> = None;
    let mut pm: Option<bool> = None;
    let mut offset: Option<i32> = None;

    for raw in spaced.split_whitespace() {
        // 「午後5:55」のように午前午後が時刻に付いている書き方をほどく。
        let (tok, ampm_prefix) = if let Some(rest) = raw.strip_prefix("午後") {
            (rest, Some(true))
        } else if let Some(rest) = raw.strip_prefix("午前") {
            (rest, Some(false))
        } else {
            (raw, None)
        };
        if ampm_prefix.is_some() {
            pm = ampm_prefix;
        }
        if tok.is_empty() {
            continue;
        }
        let lower = tok.to_ascii_lowercase();
        if lower == "pm" || lower == "p.m." {
            pm = Some(true);
        } else if lower == "am" || lower == "a.m." {
            pm = Some(false);
        } else if tok.contains(':') {
            // 時刻（"17:55"）か、"+09:00" の時差。
            if let Some(o) = parse_offset(tok) {
                offset = Some(o);
            } else if clock.is_none() {
                clock = parse_clock(tok);
            }
        } else if let Some(o) = parse_offset(tok) {
            offset = Some(o);
        } else if let Ok(n) = tok.parse::<u32>() {
            if tok.len() == 4 && year.is_none() {
                year = Some(n as i32);
            } else if tok.len() <= 2 {
                if year.is_some() {
                    after_year.push(n);
                } else {
                    before_year.push(n);
                }
            }
        } else if lower.len() >= 3 && lower.is_char_boundary(3) {
            if let Some(i) = MONTHS.iter().position(|m| lower.starts_with(m)) {
                month_name.get_or_insert(i as u32 + 1);
            }
        }
    }

    let year = year?;
    let (month, day) = match month_name {
        // 月名があれば、日はその近くの数字（年より前でも後でもよい）。
        Some(m) => (m, *before_year.first().or(after_year.first())?),
        // 数字だけなら、年の後に「月 日」の順で並んでいるときだけ読む。
        None => match after_year[..] {
            [m, d, ..] => (m, d),
            _ => return None,
        },
    };
    let (mut h, mi, s) = clock?;
    match pm {
        Some(true) if h < 12 => h += 12,
        Some(false) if h == 12 => h = 0,
        _ => {}
    }
    let local = NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(h, mi, s)?;
    Some(QuotedTime {
        local,
        offset_secs: offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    }

    #[test]
    fn reads_outlook_english_sent_line() {
        let t = parse_quoted_time("Friday, October 2, 2026 5:55 PM").unwrap();
        assert_eq!(t.local, at(2026, 10, 2, 17, 55));
        assert_eq!(t.offset_secs, None);
        let t = parse_quoted_time("Friday, October 2, 2026 12:05 AM").unwrap();
        assert_eq!(t.local, at(2026, 10, 2, 0, 5));
    }

    #[test]
    fn reads_japanese_forms() {
        for s in [
            "2026年10月2日 17:55",
            "2026年10月2日(金) 17:55",
            "2026年10月2日 金曜日 17:55",
            "2026/10/02 17:55",
            "2026年10月2日 午後5:55",
            "2026-10-02 17:55",
        ] {
            assert_eq!(
                parse_quoted_time(s).map(|t| t.local),
                Some(at(2026, 10, 2, 17, 55)),
                "{s}"
            );
        }
    }

    #[test]
    fn reads_rfc2822_and_gmail_forms_with_offset() {
        let t = parse_quoted_time("Fri, 2 Oct 2026 17:55:00 +0900").unwrap();
        assert_eq!(t.local, at(2026, 10, 2, 17, 55));
        assert_eq!(t.offset_secs, Some(9 * 3600));
        let t = parse_quoted_time("Fri, Oct 2, 2026 at 5:55 PM").unwrap();
        assert_eq!(t.local, at(2026, 10, 2, 17, 55));
        let t = parse_quoted_time("Thu, 1 Oct 2026 08:00:00 -0700").unwrap();
        assert_eq!(t.offset_secs, Some(-7 * 3600));
    }

    #[test]
    fn gives_up_without_year_time_or_with_ambiguous_numbers() {
        assert_eq!(parse_quoted_time("October 2 5:55 PM"), None);
        assert_eq!(parse_quoted_time("2026年10月2日"), None);
        assert_eq!(parse_quoted_time("10/02/2026 17:55"), None);
        assert_eq!(parse_quoted_time(""), None);
    }
}
