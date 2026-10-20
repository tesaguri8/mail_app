//! Rondine が書く返信の引用見出し（`compose.quoteHeader`）と転送の見出しを、Rondine の解析が読めることの試験（往復）。
//!
//! 文言は画面の翻訳（locales の JSON）から読む。文言を変えてこの試験が落ちたら、解析（attribution.rs /
//! when.rs）を足すか、文言を読める形に戻す。日時の文字列は画面の組み立て（utils/datetime.ts の
//! `formatQuoteHeaderDate`）の出力と同じもので、vitest（datetime.test.ts）でも同じ文字列を固定している。
//! 名前・アドレスは作り物。

use super::super::attribution::parse_attribution;
use super::super::sender::split_sender;
use super::super::split_reply;
use super::super::when::{parse_quoted_time, QuotedTime};
use chrono::NaiveDate;
use serde_json::Value;

const JA: &str = include_str!("../../../../../src/renderer/locales/ja/common.json");
const EN: &str = include_str!("../../../../../src/renderer/locales/en/common.json");

/// 翻訳の JSON から "<節>.<鍵>" の文言を読む。
fn text(json: &str, section: &str, key: &str) -> String {
    let v: Value = serde_json::from_str(json).expect("locales の JSON が読める");
    v[section][key]
        .as_str()
        .unwrap_or_else(|| panic!("{section}.{key} が無い"))
        .to_string()
}

/// `compose.quoteHeader` に差出人と日時を入れた見出し（i18next の {{…}} の差し込みと同じ）。
fn quote_header(json: &str, from: &str, date: &str) -> String {
    let tpl = text(json, "compose", "quoteHeader");
    assert!(
        tpl.contains("{{from}}") && tpl.contains("{{date}}"),
        "{tpl}"
    );
    tpl.replace("{{from}}", from).replace("{{date}}", date)
}

fn time(y: i32, mo: u32, d: u32, h: u32, mi: u32, offset_hours: i32) -> QuotedTime {
    QuotedTime {
        local: NaiveDate::from_ymd_opt(y, mo, d)
            .and_then(|d| d.and_hms_opt(h, mi, 0))
            .expect("試験の日時は正しい"),
        offset_secs: Some(offset_hours * 3600),
    }
}

/// 見出しの 1 行を読み、(アドレス, 名前, 日時) を返す。
fn read(line: &str) -> (Option<String>, Option<String>, Option<QuotedTime>) {
    let (from, at) = parse_attribution(line).unwrap_or_else(|| panic!("属性行と読めない: {line}"));
    let from = from.unwrap_or_else(|| panic!("差出人が取れない: {line}"));
    let (addr, name) = split_sender(&from);
    (addr, name, at.as_deref().and_then(parse_quoted_time))
}

/// (日本語の日時, 英語の日時, 読めるはずの日時)。`formatQuoteHeaderDate` の出力の形。
fn dates() -> Vec<(&'static str, &'static str, QuotedTime)> {
    vec![
        (
            "2026/10/14(水) 12:50 +0900",
            "Wed, Oct 14, 2026 at 12:50 PM +0900",
            time(2026, 10, 14, 12, 50, 9),
        ),
        (
            "2026/10/14(水) 00:05 +0900",
            "Wed, Oct 14, 2026 at 12:05 AM +0900",
            time(2026, 10, 14, 0, 5, 9),
        ),
        (
            "2026/10/13(火) 23:50 -0400",
            "Tue, Oct 13, 2026 at 11:50 PM -0400",
            time(2026, 10, 13, 23, 50, -4),
        ),
        (
            "2026/11/30(月) 09:07 -0500",
            "Mon, Nov 30, 2026 at 9:07 AM -0500",
            time(2026, 11, 30, 9, 7, -5),
        ),
    ]
}

/// 差出人の書き方（`formatSender` の出力の形）と、読めるはずの (アドレス, 名前)。
fn senders() -> Vec<(&'static str, Option<&'static str>)> {
    vec![
        ("金城 彩 <aya@kinjo.invalid>", Some("金城 彩")),
        ("Aya Kinjo <aya@kinjo.invalid>", Some("Aya Kinjo")),
        ("Kinjo, Aya <aya@kinjo.invalid>", Some("Kinjo, Aya")),
        ("aya@kinjo.invalid", None),
    ]
}

#[test]
fn reads_own_reply_header_in_both_languages() {
    for (ja_date, en_date, want) in dates() {
        for (from, name) in senders() {
            for line in [
                quote_header(JA, from, ja_date),
                quote_header(EN, from, en_date),
            ] {
                assert_eq!(
                    read(&line),
                    (
                        Some("aya@kinjo.invalid".to_string()),
                        name.map(str::to_string),
                        Some(want)
                    ),
                    "{line}"
                );
            }
        }
    }
}

/// 返信の本文全体でも、見出しから下が引用の枠になり、本文には新しく書いた部分だけが残る。
#[test]
fn own_reply_becomes_a_quote_frame() {
    for (json, date) in [
        (JA, "2026/10/14(水) 12:50 +0900"),
        (EN, "Wed, Oct 14, 2026 at 12:50 PM +0900"),
    ] {
        let header = quote_header(json, "金城 彩 <aya@kinjo.invalid>", date);
        let body =
            format!("承知しました。\n\n{header}\n> 図面を送ります。\n> よろしくお願いします。");
        let s = split_reply(&body);
        assert_eq!(s.clean, "承知しました。", "{header}");
        assert_eq!(s.quotes.len(), 1, "{header}");
        let q = &s.quotes[0];
        assert_eq!(
            q.quoted_from.as_deref().map(split_sender),
            Some((
                Some("aya@kinjo.invalid".to_string()),
                Some("金城 彩".to_string())
            ))
        );
        assert_eq!(
            q.quoted_at.as_deref().and_then(parse_quoted_time),
            Some(time(2026, 10, 14, 12, 50, 9))
        );
    }
}

/// 転送の見出し（`compose.forwardSep` の後の 差出人 / 日時 / 宛先 / 件名）も、差出人・日時・件名が読める。
#[test]
fn reads_own_forward_header() {
    for (json, date) in [
        (JA, "2026/10/14(水) 12:50 +0900"),
        (EN, "Wed, Oct 14, 2026 at 12:50 PM +0900"),
    ] {
        let label = |section: &str, key: &str| text(json, section, key);
        let body = format!(
            "ご確認ください。\n\n{}\n{}: 金城 彩 <aya@kinjo.invalid>\n{}: {date}\n{}: taro@maeda.invalid\n{}: 図面の件\n\n図面を送ります。",
            label("compose", "forwardSep"),
            label("mailbox", "from"),
            label("compose", "date"),
            label("mailbox", "to"),
            label("compose", "subject"),
        );
        let s = split_reply(&body);
        assert_eq!(s.clean, "ご確認ください。", "{body}");
        assert_eq!(s.quotes.len(), 1, "{body}");
        let q = &s.quotes[0];
        assert_eq!(
            q.quoted_from.as_deref().map(split_sender),
            Some((
                Some("aya@kinjo.invalid".to_string()),
                Some("金城 彩".to_string())
            )),
            "{body}"
        );
        assert_eq!(
            q.quoted_at.as_deref().and_then(parse_quoted_time),
            Some(time(2026, 10, 14, 12, 50, 9)),
            "{body}"
        );
        assert_eq!(q.subject.as_deref(), Some("図面の件"), "{body}");
        assert_eq!(q.body, "図面を送ります。", "{body}");
    }
}
