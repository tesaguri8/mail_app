//! 残っていた読めない引用の形（2026-10-20 の vaio の集計で見つかった書き方）の試験。名前・アドレスは作り物。

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

/// アドレスの中に mailto が入れ子になった差出人でも、見出しのほかの行（Sent ほか）まで読む。
#[test]
fn reads_header_with_nested_mailto() {
    let body = "本文\n\nFrom: 山田 花子 <hanako@example.com<mailto:hanako@example.com>>\n\
Sent: Friday, October 2, 2026 5:55 PM\nTo: 佐藤 <s@example.com<mailto:s@example.com>>\n\
Subject: RE: 件\n\n引用本文";
    let s = split_reply(body);
    assert_eq!(s.quotes.len(), 1);
    assert_eq!(s.quotes[0].subject.as_deref(), Some("RE: 件"));
    assert_eq!(s.quotes[0].body, "引用本文");
    assert_eq!(
        frames(body),
        vec![(
            some("山田 花子 <hanako@example.com<mailto:hanako@example.com>>"),
            some("Friday, October 2, 2026 5:55 PM")
        )]
    );
    let body = "本文\n\nFrom: Hanako <hanako@example.com <mailto:hanako@example.com> >\n\
Sent: Friday, October 2, 2026 5:55 PM\n\n引用本文";
    assert_eq!(
        frames(body),
        vec![(
            some("Hanako <hanako@example.com <mailto:hanako@example.com> >"),
            some("Friday, October 2, 2026 5:55 PM")
        )]
    );
}

/// HTML から文字にしたときの CSS の行が区切りの前にあっても、見出しを読む。
#[test]
fn reads_header_after_css_garbage() {
    let body =
        "本文\n\n.mail-sig { font-size: 12px; color: #e9e9e9; }\n----- 引用元メッセージ -----\n\
差出人: 山田 花子 <hanako@example.com>\n日付: 2026/10/02 12:10\n件名: x\n\n引用本文";
    assert_eq!(
        frames(body),
        vec![(
            some("山田 花子 <hanako@example.com>"),
            some("2026/10/02 12:10")
        )]
    );
}

/// "On … wrote:" の名前に空白や和文が混じっても読む。
#[test]
fn reads_on_wrote_with_spaced_names() {
    for name in ["山田 花子", "Hanako Yamada", "山田花子"] {
        let body = format!("本文\n\nOn 2026/10/02 12:10, {name} wrote:\n> 引用");
        assert_eq!(frames(&body), vec![(some(name), some("2026/10/02 12:10"))]);
    }
}

/// アドレスも続く見出しも無い「送信者：名前」は、通知メールの本文とみなして切らない。
/// 続く行に見出しがあれば、これまでどおり引用の始まり。
#[test]
fn bare_sender_line_needs_following_header() {
    let s = split_reply("ご注文ありがとうございます。\n\n送信者：山田\n\nお知らせ本文。");
    assert!(s.quotes.is_empty());
    assert!(s.clean.contains("お知らせ本文。"));
    let body = "本文\n\n送信者：山田\n送信日時：2026年10月2日 17:55\n\n引用";
    assert_eq!(
        frames(body),
        vec![(some("山田"), some("2026年10月2日 17:55"))]
    );
}

/// 「に」の無い "2026/10/02 12:10:00 名前 addr 名前:" も日時を読む。
#[test]
fn reads_time_then_sender_without_ni() {
    let body = "本文\n\n2026/10/02 12:10:00 山田 hanako@example.com 花子:\n> 引用";
    assert_eq!(
        frames(body),
        vec![(
            some("山田 hanako@example.com 花子"),
            some("2026/10/02 12:10:00")
        )]
    );
}

/// Apple の転送（"Begin forwarded message:" の後に From / Subject / Date / To）。前に罫線があってもよい。
#[test]
fn reads_apple_forward() {
    let header = "Begin forwarded message:\n\nFrom: 山田 花子 <hanako@example.com>\nSubject: 件\n\
Date: October 2, 2026 at 17:55:00 GMT+9\nTo: s@example.com\n\n引用本文";
    let want = vec![(
        some("山田 花子 <hanako@example.com>"),
        some("October 2, 2026 at 17:55:00 GMT+9"),
    )];
    for body in [
        format!("本文\n\n{header}"),
        format!("本文\n\n=========================\n{header}"),
    ] {
        let s = split_reply(&body);
        assert_eq!(s.clean, "本文");
        assert_eq!(s.quotes[0].body, "引用本文");
        assert_eq!(frames(&body), want);
    }
}

/// Android 版 Gmail の "火, 20 10月 2026、11:21 名前 <addr>:"（アドレスの無い形も）。
#[test]
fn reads_android_gmail_attribution() {
    let body = "本文\n\n火, 20 10月 2026、11:21 山田花子<hanako@example.com>:\n> 引用";
    assert_eq!(
        frames(body),
        vec![(
            some("山田花子<hanako@example.com>"),
            some("火, 20 10月 2026、11:21")
        )]
    );
    let body = "本文\n\n火, 20 10月 2026、11:21 山田 Hanako 花子:\n> 引用";
    assert_eq!(
        frames(body),
        vec![(some("山田 Hanako 花子"), some("火, 20 10月 2026、11:21"))]
    );
}

/// Gmail 日本語版の差出人に mailto が入れ子になった形、曜日が「(月)」の形。
#[test]
fn reads_gmail_japanese_attribution_variants() {
    let body = "本文\n\n2026年10月2日(金) 17:55 hanako@example.com<mailto:hanako@example.com> \
<hanako@example.com<mailto:hanako@example.com>>:\n> 引用";
    assert_eq!(frames(body)[0].1, some("2026年10月2日(金) 17:55"));
    let body = "本文\n\n2026年10月5日(月) 17:55 Hanako <hanako@example.com>:\n> 引用";
    assert_eq!(
        frames(body),
        vec![(
            some("Hanako <hanako@example.com>"),
            some("2026年10月5日(月) 17:55")
        )]
    );
}

/// 属性行が改行で割れた形（"<addr>:" / "addr>:" / "<addr>のメール:" が次の行）は、つないで読む。
#[test]
fn reads_wrapped_attribution() {
    let apple = (
        some("山田\u{3000}花子 <hanako@example.com>"),
        some("Oct 2, 2026, 9:05 PM +0900"),
    );
    let iphone = (
        some("山田 花子 <hanako@example.com>"),
        some("2026/10/02 12:10"),
    );
    let cases = [
        (
            "Oct 2, 2026, 9:05 PM +0900, 山田\u{3000}花子\n<hanako@example.com>:",
            &apple,
        ),
        (
            "Oct 2, 2026, 9:05 PM +0900, 山田\u{3000}花子 <\nhanako@example.com>:",
            &apple,
        ),
        (
            "2026/10/02 12:10、山田 花子\n<hanako@example.com>のメール:",
            &iphone,
        ),
    ];
    for (attribution, want) in cases {
        let body = format!("本文です。\n\n{attribution}\n> 引用");
        let s = split_reply(&body);
        assert_eq!(s.clean, "本文です。", "{attribution}");
        assert_eq!(s.quotes[0].body, "> 引用", "{attribution}");
        assert_eq!(frames(&body), vec![want.clone()], "{attribution}");
    }
}

/// 引用の中の 2 通目の属性行が割れていても、つないで読む。
#[test]
fn reads_wrapped_attribution_inside_quote() {
    let body = "本文\n\nOn 2026/10/02 12:10, Taro <taro@example.com> wrote:\n\n返事です。\n\n\
2026/10/01 12:10、山田 花子\n<hanako@example.com>のメール:\n\n最初の文";
    let s = split_reply(body);
    assert_eq!(s.quotes.len(), 2);
    assert_eq!(s.quotes[0].body, "返事です。");
    assert_eq!(s.quotes[1].body, "最初の文");
    assert_eq!(
        frames(body)[1],
        (
            some("山田 花子 <hanako@example.com>"),
            some("2026/10/01 12:10")
        )
    );
}

/// 名前の無い Apple の "Oct 2, 2026, 9:05 PM +0900, addr:"。
#[test]
fn reads_apple_attribution_without_name() {
    let body = "本文\n\nOct 2, 2026, 9:05 PM +0900, hanako@example.com:\n> 引用";
    assert_eq!(
        frames(body),
        vec![(
            some("hanako@example.com"),
            some("Oct 2, 2026, 9:05 PM +0900")
        )]
    );
}

/// メールマガジンの飾りの罫線は、引用の始まりにしない（罫線の後ろの本文が本文として残る）。
#[test]
fn decorative_rule_is_not_a_quote() {
    for rule in [
        "____________________________________________________________________________☆☆",
        "____________________________________________________________________________",
        "=========================",
        "━━━━━━━━━━━━━━━━━━━",
        "****************************************************",
    ] {
        // 罫線の後ろが短いと、署名・フッタとして本文から落とす（`strip_signature`）。
        // メールマガジンらしく、罫線の後ろに本文が長く続く形で確かめる。
        let articles = (1..=12)
            .map(|n| format!("記事{n}の紹介です。"))
            .collect::<Vec<_>>()
            .join("\n");
        let body =
            format!("本日のお知らせです。\n\n{rule}\n新商品のご案内\n{rule}\n{articles}\n\n以上");
        let s = split_reply(&body);
        assert!(s.quotes.is_empty(), "{rule}");
        assert!(s.clean.contains("新商品のご案内"), "{rule}");
        assert!(s.clean.contains("以上"), "{rule}");
    }
}

/// Outlook の下線の次に "From:" が来る形は、これまでどおり枠になる（間に空行が 2〜3 行あってもよい）。
#[test]
fn outlook_rule_followed_by_header_is_a_quote() {
    let want = vec![(
        some("山田 <hanako@example.com>"),
        some("Friday, October 2, 2026 5:55 PM"),
    )];
    for gap in ["\n", "\n\n\n", "\n\n\n\n"] {
        let body = format!(
            "本文\n\n________________________________{gap}From: 山田 <hanako@example.com>\n\
Sent: Friday, October 2, 2026 5:55 PM\nTo: s@example.com\nSubject: x\n\n引用"
        );
        let s = split_reply(&body);
        assert_eq!(s.clean, "本文", "{gap:?}");
        assert_eq!(s.quotes[0].body, "引用", "{gap:?}");
        assert_eq!(frames(&body), want, "{gap:?}");
    }
}

/// 引用の中で、罫線の次に見出しが来れば次の 1 通。飾りの罫線では分けない。
#[test]
fn rule_inside_quote_splits_only_before_header() {
    let body = "本文\n\nOn 2026/10/02 12:10, Taro <taro@example.com> wrote:\n\n返事です。\n\
=========================\n飾り\n=========================\n\n\
________________________________\nFrom: 山田 <hanako@example.com>\n\
Sent: Friday, October 2, 2026 5:55 PM\n\n最初の文";
    let s = split_reply(body);
    assert_eq!(s.quotes.len(), 2);
    assert!(s.quotes[0].body.contains("飾り"));
    assert_eq!(s.quotes[1].body, "最初の文");
}
