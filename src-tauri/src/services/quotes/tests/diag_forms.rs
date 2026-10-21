//! 診断（quotes::diag）で見つかった、読めなかった枠の形の試験（2026-10-21 vaio）。名前・アドレスは作り物。

use super::super::split_reply;

/// 本文を分けて、引用ブロックごとの (差出人, 日時) を返す。
fn frames(body: &str) -> Vec<(Option<String>, Option<String>)> {
    split_reply(body)
        .quotes
        .into_iter()
        .map(|q| (q.quoted_from, q.quoted_at))
        .collect()
}

fn some(s: &str) -> Option<String> {
    Some(s.to_string())
}

/// 各行の間に空行があり、値の頭に NBSP が入り、途中に知らない項目（"Reply:"）がある見出し。
/// 知らない項目で読み取りを止めず、その先の Date まで読む（止まると 1 通が 2 つに割れていた）。
#[test]
fn reads_blank_separated_header_with_unknown_field_and_nbsp() {
    let body = "本文です。\n山田\n\n=========================\r\n\r\n\
From:\u{a0}山田（Taro Yamada） <taro@example.jp>\r\n\r\n\
Reply:\u{a0}山田（Taro Yamada） <taro@example.jp>\r\n\r\n\
Date:\u{a0}August 9, 2026 at 12:34:56\r\n\r\n\
To:\u{a0}Foo Bar <foo@example.jp>\r\n\r\n\
Subject:\u{a0} Re: Re: 件名\r\n\r\n引用の本文";
    let s = split_reply(body);
    assert_eq!(s.quotes.len(), 1);
    assert_eq!(s.quotes[0].body, "引用の本文");
    assert_eq!(
        frames(body),
        vec![(
            some("山田（Taro Yamada） <taro@example.jp>"),
            some("August 9, 2026 at 12:34:56")
        )]
    );
}

/// 本文の 2 行目の「Cc：各位」を 1 通の始まりにしない（その先に次の 1 通の見出しがあっても）。
#[test]
fn cc_line_in_body_does_not_start_a_message() {
    let body = "本文\n\nFrom: 山田 <taro@example.jp<mailto:taro@example.jp>>\n\
Sent: Friday, August 7, 2026 9:05 AM\nTo: 佐藤 <sato@example.jp>\nSubject: RE: 件名\n\n\n\
株式会社　佐藤　様\n\nCc：鈴木　様　各位\n\n\n\nお世話になります。\n\n\
From: 佐藤 <sato@example.jp>\nSent: Thursday, August 6, 2026 9:05 AM\nTo: 山田 <taro@example.jp>\n\
Cc: 鈴木 <suzuki@example.jp>\nSubject: 件名\n\n最初の本文";
    let s = split_reply(body);
    assert_eq!(
        frames(body),
        vec![
            (
                some("山田 <taro@example.jp<mailto:taro@example.jp>>"),
                some("Friday, August 7, 2026 9:05 AM")
            ),
            (
                some("佐藤 <sato@example.jp>"),
                some("Thursday, August 6, 2026 9:05 AM")
            ),
        ]
    );
    assert!(s.quotes[0].body.contains("Cc：鈴木"));
}

/// 項目名の後ろにコロンが無い見出し（「------ 元のメッセージ ------」の次の「差出人 "名前" <A>」）。
#[test]
fn reads_colonless_header_after_separator() {
    let body = "本文\n\n=========================\n------ 元のメッセージ ------\n\
差出人 \"山田\" <taro@example.jp>\n日付 2026年10月2日 17:55\n宛先 sato@example.jp\n件名 打ち合わせ\n\n引用";
    let s = split_reply(body);
    assert_eq!(s.quotes[0].subject.as_deref(), Some("打ち合わせ"));
    assert_eq!(s.quotes[0].body, "引用");
    assert_eq!(
        frames(body),
        vec![(
            some("\"山田\" <taro@example.jp>"),
            some("2026年10月2日 17:55")
        )]
    );
    // 区切りの無い本文の「日付 未定」は見出しにしない。
    let s = split_reply("打ち合わせの件\n\n日付 未定\n件名 未定\n\nよろしく");
    assert!(s.quotes.is_empty());
}

/// 1 行に詰まった見出し（"From：…To：…日時：…件名：…"）は、項目ごとに分けて読む。
#[test]
fn reads_header_packed_into_one_line() {
    let body = "本文\n\n----- 引用元メッセージ -----\n\
From：\"Taro Yamada\" <taro@example.jp>To：\"佐藤 花子\" <hanako@example.jp>日時：2026/10/02 金 12:10件名：Re: 打ち合わせ\n\
引用の本文";
    let s = split_reply(body);
    assert_eq!(s.quotes[0].subject.as_deref(), Some("Re: 打ち合わせ"));
    assert_eq!(s.quotes[0].body, "引用の本文");
    assert_eq!(
        frames(body),
        vec![(
            some("\"Taro Yamada\" <taro@example.jp>"),
            some("2026/10/02 金 12:10")
        )]
    );
}

/// 罫線の後で、"On <日時>" と "名前 <addr> wrote:" の 2 行に割れた属性行。
#[test]
fn reads_on_wrote_split_after_date() {
    let body = "本文\n\n****************************************************\n\
On Fri, 2 Oct 2026 12:10:00 +0900\nTaro Yamada <taro@example.jp> wrote:\n> 引用";
    let s = split_reply(body);
    assert_eq!(s.quotes[0].body, "> 引用");
    assert_eq!(
        frames(body),
        vec![(
            some("Taro Yamada <taro@example.jp>"),
            some("Fri, 2 Oct 2026 12:10:00 +0900")
        )]
    );
}

/// 「>」の中に入った見出し（"> Begin forwarded message:" / "> 転送: ⏎ > ⏎ > 差出人: …"）も読む。
#[test]
fn reads_header_inside_gt_quote() {
    let apple =
        "本文\n\n> Begin forwarded message:\n>\n> From: 山田 <taro@example.jp>\n> Subject: 件名\n\
> Date: October 2, 2026 at 17:55:00 GMT+9\n> To: sato@example.jp\n>\n> 引用";
    assert_eq!(
        frames(apple),
        vec![(
            some("山田 <taro@example.jp>"),
            some("October 2, 2026 at 17:55:00 GMT+9")
        )]
    );
    let labeled =
        "本文\n\n> 転送:\n>\n> 差出人: 山田 <taro@example.jp>\n> 日付: 2026年10月2日 17:55\n\
> 宛先: sato@example.jp\n>\n> 引用";
    assert_eq!(
        frames(labeled),
        vec![(some("山田 <taro@example.jp>"), some("2026年10月2日 17:55"))]
    );
}

/// 罫線の後の、通知メールの「送信元：店名 ⏎ 〒…」と、催しの案内の「日時：…」は引用にしない。
#[test]
fn notices_after_rule_are_not_quotes() {
    let shop = "ご注文ありがとうございました。\n\n======================================\n\
送信元：山田商店\n〒900-0001 那覇市1-2-3\nTEL：098-000-0000\n\nまたのご利用をお待ちしております。";
    assert!(split_reply(shop).quotes.is_empty());
    let event = "お知らせ\n\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n日時： 10月2日（金） 10:00～12:00\n\
場所： 公民館\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n詳細は以下のとおりです。";
    let s = split_reply(event);
    assert!(s.quotes.is_empty());
    assert!(s.clean.contains("日時： 10月2日（金）"));
}

/// 本文の中の全角の「ｃｃ：…」（項目名は半角にそろえると "cc"）も、1 通の始まりにしない。
#[test]
fn fullwidth_cc_sentence_in_body_does_not_start_a_message() {
    let body = "本文\n\nFrom: 山田 <taro@example.jp>\nSent: Friday, August 7, 2026 9:05 AM\n\
To: 佐藤 <sato@example.jp>\nSubject: RE: 図面\n\n佐藤様\n\n\
ｃｃ：鈴木さんに、ＣＧのデータ送信お願いできますか\n\n\
From: 佐藤 <sato@example.jp>\nSent: Thursday, August 6, 2026 9:05 AM\nTo: 山田 <taro@example.jp>\n\
Subject: 図面\n\n最初の本文";
    let s = split_reply(body);
    assert_eq!(
        frames(body),
        vec![
            (
                some("山田 <taro@example.jp>"),
                some("Friday, August 7, 2026 9:05 AM")
            ),
            (
                some("佐藤 <sato@example.jp>"),
                some("Thursday, August 6, 2026 9:05 AM")
            ),
        ]
    );
    assert!(s.quotes[0].body.contains("ｃｃ：鈴木さんに"));
}

/// ISO 8601 の日時の属性行（"2023-10-30T10:00:00Z に addr さんは書きました:" / "+09:00" 付き）。
#[test]
fn reads_iso_8601_attribution() {
    let body = "本文\n\n2023-10-30T10:00:00Z に taro@example.jp さんは書きました:\n> 引用";
    assert_eq!(
        frames(body),
        vec![(some("taro@example.jp"), some("2023-10-30T10:00:00Z"))]
    );
    let body = "本文\n\n2023-10-30T10:00:00+09:00 に taro@example.jp が書きました:\n> 引用";
    assert_eq!(
        frames(body),
        vec![(some("taro@example.jp"), some("2023-10-30T10:00:00+09:00"))]
    );
}

/// 区切り行の後の、「>>」の 2 段の中に入った見出しも読む（段の数だけ「>」を外す）。
#[test]
fn reads_header_inside_two_gt_levels() {
    let body =
        "本文\n\n----- Original Message -----\n>> From: taro@example.jp\n>> To: me@example.jp\n\
>> Date: 2023-10-30 10:00:00\n>> Subject: 件名\n>>\n>> 引用";
    let s = split_reply(body);
    assert_eq!(s.quotes[0].subject.as_deref(), Some("件名"));
    assert_eq!(
        frames(body),
        vec![(some("taro@example.jp"), some("2023-10-30 10:00:00"))]
    );
}
