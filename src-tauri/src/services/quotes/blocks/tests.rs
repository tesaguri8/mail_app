//! 1 通ずつに分ける試験（連なった見出し・字下げした転送・表の形・見出しのかたまり・順番違い）。
//! 名前・宛先・URL はすべて作り物。

use crate::services::quotes::split_reply;

/// Outlook の英語の見出しで連なった「元のメッセージ」を、1 通ずつのブロックに分ける
/// （実例の形。個人名と宛先は作り物）。
#[test]
fn splits_chained_outlook_original_messages() {
    let body = "鈴木様\n\nチェックバックその3を送ります。\n\n田中\n\n\
-----Original Message-----\n\
From: 田中 一郎 <tanaka@example.jp>\n\
Sent: Friday, October 2, 2026 5:55 PM\n\
To: 鈴木 次郎 <suzuki@example.jp>\n\
Subject: RE: プレカット図 チェックバック\n\
\n\
鈴木様\n修正点をまとめました。\n\
\n\
-----Original Message-----\n\
From: 高橋 花子 <takahashi@example.com>\n\
Sent: Friday, October 2, 2026 2:50 PM\n\
To: 田中 一郎 <tanaka@example.jp>; 鈴木 次郎 <suzuki@example.jp>\n\
Subject: プレカット図 チェックバック\n\
\n\
田中様、鈴木様\n図面を送ります。";
    let s = split_reply(body);
    assert_eq!(s.clean, "鈴木様\n\nチェックバックその3を送ります。\n\n田中");
    assert_eq!(s.quotes.len(), 2);
    let (a, b) = (&s.quotes[0], &s.quotes[1]);
    assert_eq!((a.order, b.order), (0, 1));
    assert_eq!(
        a.quoted_from.as_deref(),
        Some("田中 一郎 <tanaka@example.jp>")
    );
    assert_eq!(
        a.quoted_at.as_deref(),
        Some("Friday, October 2, 2026 5:55 PM")
    );
    assert_eq!(
        a.subject.as_deref(),
        Some("RE: プレカット図 チェックバック")
    );
    assert_eq!(a.body, "鈴木様\n修正点をまとめました。");
    assert_eq!(
        b.quoted_from.as_deref(),
        Some("高橋 花子 <takahashi@example.com>")
    );
    assert_eq!(
        b.quoted_at.as_deref(),
        Some("Friday, October 2, 2026 2:50 PM")
    );
    assert_eq!(b.body, "田中様、鈴木様\n図面を送ります。");
    assert_ne!(a.fingerprint, b.fingerprint);
}

/// 日本語の Outlook（区切り無し・差出人/送信日時）と、Gmail の転送も 1 通ずつに分ける。
#[test]
fn splits_japanese_outlook_header_and_gmail_forward() {
    let body = "転送します。\n\n\
---------- Forwarded message ---------\n\
From: 高橋 花子 <takahashi@example.com>\n\
Date: 2026年10月2日(金) 14:50\n\
Subject: 図面\n\
To: <tanaka@example.jp>\n\
\n\
図面です。\n\
\n\
差出人: 田中 一郎 <tanaka@example.jp>\n\
送信日時: 2026年10月1日 9:30\n\
宛先: 高橋 花子\n\
件名: 図面のお願い\n\
\n\
図面をお願いします。";
    let s = split_reply(body);
    assert_eq!(s.clean, "転送します。");
    assert_eq!(s.quotes.len(), 2);
    assert_eq!(
        s.quotes[0].quoted_at.as_deref(),
        Some("2026年10月2日(金) 14:50")
    );
    assert_eq!(s.quotes[0].body, "図面です。");
    assert_eq!(
        s.quotes[1].quoted_from.as_deref(),
        Some("田中 一郎 <tanaka@example.jp>")
    );
    assert_eq!(s.quotes[1].quoted_at.as_deref(), Some("2026年10月1日 9:30"));
    assert_eq!(s.quotes[1].body, "図面をお願いします。");
}

/// `>` の中に入った見出しでは分けない（引用の中の引用は、外側の 1 通の一部）。
#[test]
fn does_not_split_inside_gt_quotes() {
    let body = "OK\n\nOn Mon, 30 Jun 2026 10:00, Taro <taro@example.com> wrote:\n> hi\n> -----Original Message-----\n> From: Jiro <jiro@example.com>\n> Sent: Monday, June 29, 2026 9:00 AM\n> older";
    let s = split_reply(body);
    assert_eq!(s.quotes.len(), 1);
    assert!(s.quotes[0].body.starts_with("> hi"));
}

/// 転送と返信が重なった本文（実例の区切りの書き方のまま。個人名とアドレスは作り物）。
/// Thunderbird の転送・太字の Outlook 見出し・"On … wrote:"・字下げした Gmail 転送と
/// Outlook の「元のメッセージ」が、1 本の引用の中に続いている。
fn stacked() -> String {
    [
        "皆様",
        "",
        "下記、転送します。",
        "",
        "田中",
        "",
        "-------- Forwarded Message --------",
        "Subject: \tRE: 図面の件",
        "Date: \tFri, 9 Oct 2026 15:29:01 +0900",
        "From: \t木材商事 山田花子 <hanako@mokuzai.example.jp>",
        "To: \t田中 <tanaka@example.jp>",
        "",
        "田中様",
        "10/9 の本文です。",
        "",
        "*From:*木材商事　　山田花子<hanako@mokuzai.example.jp>",
        "*Sent:* Wednesday, October 7, 2026 7:36 PM",
        "*To:* 佐藤一郎<i-sato@kensetsu.example.co.jp>",
        "*Subject:* RE: 図面の件",
        "",
        "佐藤様",
        "10/7 の本文です。",
        "",
        "*From:*佐藤一郎<i-sato@kensetsu.example.co.jp>",
        "*Sent:* Tuesday, October 6, 2026 11:28 AM",
        "*To:* 木材商事　　山田花子<hanako@mokuzai.example.jp>",
        "",
        "山田様",
        "10/6 の本文です。",
        "",
        "On 2026/10/05 6:28, 鈴木次郎wrote:",
        "        佐藤様",
        "        10/5 の本文です。",
        "",
        "        ---------- Forwarded message ---------",
        "        From: <taro@maeda.example> <mailto:taro@maeda.example>",
        "        Date: 2026年10月3日(土) 12:37",
        "        Subject: 図面",
        "        To: <jiro@example.jp>",
        "",
        "        10/3 の本文です。",
        "          字下げの続き。",
        "",
        "        -----Original Message-----",
        "        From: taro@maeda.example <taro@maeda.example> <mailto:taro@maeda.example>",
        "        Sent: Friday, October 2, 2026 5:55 PM",
        "        To: 高橋 健<12345TAKA@city.example.lg.jp>",
        "",
        "        10/2 夕方の本文です。",
        "",
        "        -----Original Message-----",
        "        From: 高橋 健<12345TAKA@city.example.lg.jp>",
        "        Sent: Friday, October 2, 2026 2:50 PM",
        "",
        "        10/2 昼の本文です。",
    ]
    .join("\n")
}

/// 太字の見出し（"*From:*"）・途中の "On … wrote:"・字下げした区切りのどれでも 1 通ずつに分ける。
#[test]
fn splits_stacked_forwards_bold_headers_and_inner_wrote() {
    let s = split_reply(&stacked());
    assert_eq!(s.clean, "皆様\n\n下記、転送します。\n\n田中");
    let got: Vec<(Option<&str>, Option<&str>)> = s
        .quotes
        .iter()
        .map(|q| (q.quoted_from.as_deref(), q.quoted_at.as_deref()))
        .collect();
    assert_eq!(
        got,
        vec![
            (
                Some("木材商事 山田花子 <hanako@mokuzai.example.jp>"),
                Some("Fri, 9 Oct 2026 15:29:01 +0900")
            ),
            (
                Some("木材商事　　山田花子<hanako@mokuzai.example.jp>"),
                Some("Wednesday, October 7, 2026 7:36 PM")
            ),
            (
                Some("佐藤一郎<i-sato@kensetsu.example.co.jp>"),
                Some("Tuesday, October 6, 2026 11:28 AM")
            ),
            (Some("鈴木次郎"), Some("2026/10/05 6:28")),
            (
                Some("<taro@maeda.example> <mailto:taro@maeda.example>"),
                Some("2026年10月3日(土) 12:37")
            ),
            (
                Some("taro@maeda.example <taro@maeda.example> <mailto:taro@maeda.example>"),
                Some("Friday, October 2, 2026 5:55 PM")
            ),
            (
                Some("高橋 健<12345TAKA@city.example.lg.jp>"),
                Some("Friday, October 2, 2026 2:50 PM")
            ),
        ]
    );
    let bodies: Vec<&str> = s.quotes.iter().map(|q| q.body.as_str()).collect();
    assert_eq!(bodies[0], "田中様\n10/9 の本文です。");
    assert_eq!(bodies[1], "佐藤様\n10/7 の本文です。");
    assert_eq!(bodies[3], "佐藤様\n10/5 の本文です。");
    // 字下げした転送の中の 1 通は左に寄せる（中の相対的な字下げは残す）。
    assert_eq!(bodies[4], "10/3 の本文です。\n  字下げの続き。");
    assert_eq!(bodies[6], "10/2 昼の本文です。");
    let orders: Vec<i64> = s.quotes.iter().map(|q| q.order).collect();
    assert_eq!(orders, (0..7).collect::<Vec<_>>());
}

/// 引用の途中でも、日付もアドレスも無い「…wrote:」「…書きました:」の文では分けない。
#[test]
fn inner_sentence_ending_with_wrote_is_not_a_boundary() {
    let body = "OK\n\n-----Original Message-----\nFrom: Taro <taro@example.com>\nSent: Friday, October 2, 2026 5:55 PM\n\nOn Monday, I wrote:\n議事録をまとめて共有すると彼は書きました:\nよろしく";
    let s = split_reply(body);
    assert_eq!(s.quotes.len(), 1);
    assert!(s.quotes[0].body.contains("On Monday, I wrote:"));
    assert!(s.quotes[0].body.contains("彼は書きました:"));
}

/// Outlook の見出しの間に、Thunderbird の表の形の転送（項目名と値が別の行）が挟まっても、
/// 1 通ずつに分け、差出人・日時を読む（名前・宛先・URL は作り物）。
#[test]
fn splits_thunderbird_table_forward_between_outlook_headers() {
    let body = "前田様\n\n転送します。\n\n\
From: 仲村 亮 <ryo@shinko.example.jp> \r\n\
Sent: Tuesday, October 20, 2026 11:36 AM\r\n\
To: Mak <mak@example.jp>\r\n\
Subject: Fwd: 図面 その3\r\n\
\r\n\
前田様\r\n\r\n下記の通りです。\r\n\r\n新興　仲村\r\n\r\n\r\n\
-------- Forwarded Message -------- \r\n\r\n\r\n\
Subject: \r\n\r\nRE: Fwd: 図面 その3\r\n\r\n\r\n\
Date: \r\n\r\nTue, 20 Oct 2026 11:21:08 +0900\r\n\r\n\r\n\
From: \r\n\r\n木材商事 山田花子  <mailto:hanako@mokuzai.example.jp> <hanako@mokuzai.example.jp>\r\n\r\n\r\n\
To: \r\n\r\n'仲村 亮'  <mailto:ryo@shinko.example.jp> <ryo@shinko.example.jp>\r\n\r\n\r\n\r\n\
仲村様\r\n\r\n図面はこちらから取ってください。\r\nhttps://files.example.jp/d/abc123\r\n\r\n\
From: 仲村 亮 <ryo@shinko.example.jp> \r\n\
Sent: Monday, October 19, 2026 6:46 AM\r\n\
To: 木材商事 山田花子 <hanako@mokuzai.example.jp>\r\n\
Subject: Fwd: 図面 その3\r\n\
\r\n\
山田様\r\nお願いします。";
    let s = split_reply(body);
    assert_eq!(s.clean, "前田様\n\n転送します。");
    assert_eq!(s.quotes.len(), 3);
    let tb = &s.quotes[1];
    assert_eq!(
        tb.quoted_from.as_deref(),
        Some("木材商事 山田花子  <mailto:hanako@mokuzai.example.jp> <hanako@mokuzai.example.jp>")
    );
    assert_eq!(
        tb.quoted_at.as_deref(),
        Some("Tue, 20 Oct 2026 11:21:08 +0900")
    );
    assert_eq!(tb.subject.as_deref(), Some("RE: Fwd: 図面 その3"));
    assert!(tb.body.starts_with("仲村様"), "{:?}", tb.body);
    assert!(tb.body.contains("https://files.example.jp/d/abc123"));
    // 直前の 1 通は自分の署名で終わり、転送の中身を抱えない。
    assert!(
        s.quotes[0].body.ends_with("新興　仲村"),
        "{:?}",
        s.quotes[0].body
    );
    assert_eq!(
        s.quotes[2].quoted_at.as_deref(),
        Some("Monday, October 19, 2026 6:46 AM")
    );
}

/// "-------- Original Message --------" の表の形も同じに読む。
#[test]
fn splits_thunderbird_table_original_message() {
    let body = "了解です。\n\n-------- Original Message --------\n\nSubject:\n\n見積もり\n\n\
Date:\n\nFri, 9 Oct 2026 15:29:01 +0900\n\nFrom:\n\n鈴木 次郎 <jiro@example.com>\n\n\
To:\n\n佐藤 一郎 <ichiro@example.jp>\n\n\n佐藤様\n見積もりを送ります。";
    let s = split_reply(body);
    assert_eq!(s.clean, "了解です。");
    assert_eq!(s.quotes.len(), 1);
    let q = &s.quotes[0];
    assert_eq!(
        q.quoted_from.as_deref(),
        Some("鈴木 次郎 <jiro@example.com>")
    );
    assert_eq!(
        q.quoted_at.as_deref(),
        Some("Fri, 9 Oct 2026 15:29:01 +0900")
    );
    assert_eq!(q.body, "佐藤様\n見積もりを送ります。");
}

/// 既知の形に当たらない見出し（"Date:" から始まる・区切りの無い表の形）も、見出しが近くに
/// 固まっていれば 1 通の区切りとして読む（予備の判定。名前・宛先は作り物）。
#[test]
fn splits_at_header_cluster_of_unknown_form() {
    let body = "了解です。\n\n\
Date: Fri, 9 Oct 2026 15:29:01 +0900\n\
From: 鈴木 次郎 <jiro@example.com>\n\
Subject: 見積もり\n\
\n\
佐藤様\n見積もりを送ります。\n\n\
Subject:\n\n見積もりの前の件\n\n\
Date:\n\n2026年10月8日(木) 9:05\n\n\
From:\n\n佐藤 一郎 <ichiro@example.jp>\n\n\n\
鈴木様\nお願いします。";
    let s = split_reply(body);
    assert_eq!(s.clean, "了解です。");
    assert_eq!(
        s.quotes.len(),
        2,
        "{:?}",
        s.quotes.iter().map(|q| &q.body).collect::<Vec<_>>()
    );
    assert_eq!(
        s.quotes[0].quoted_from.as_deref(),
        Some("鈴木 次郎 <jiro@example.com>")
    );
    assert_eq!(
        s.quotes[0].quoted_at.as_deref(),
        Some("Fri, 9 Oct 2026 15:29:01 +0900")
    );
    assert_eq!(s.quotes[0].body, "佐藤様\n見積もりを送ります。");
    assert_eq!(
        s.quotes[1].quoted_from.as_deref(),
        Some("佐藤 一郎 <ichiro@example.jp>")
    );
    assert_eq!(
        s.quotes[1].quoted_at.as_deref(),
        Some("2026年10月8日(木) 9:05")
    );
    assert_eq!(s.quotes[1].body, "鈴木様\nお願いします。");
}

/// 本文の申込書・見積の「件名:」「日付:」「宛先:」では区切らない（差出人が無い）。
#[test]
fn form_fields_in_body_are_not_a_header_cluster() {
    let forms = [
        "申込書を送ります。\n\n件名: 夏祭りの出店\n日付: 2026年8月1日 10:00\n担当: 佐藤\n\nよろしくお願いします。",
        "件名: 夏祭りの出店 日付: 2026年8月1日 10:00\n\nよろしくお願いします。",
        "見積です。\n\n件名: 見積\n日付: 2026年8月1日 10:00\n宛先: 鈴木 <suzuki@example.jp>\n\n合計 3 万円",
    ];
    for body in forms {
        let s = split_reply(body);
        assert!(s.quotes.is_empty(), "{body:?}");
        assert_eq!(s.clean, body.trim());
    }
}

/// 本文の後に、様々な書き方の 1 通が続く本文（見出しは作り物）。
fn outlook_then(second: &str) -> String {
    format!(
        "了解です。\n\n\
-----Original Message-----\n\
From: 高橋 花子 <takahashi@example.com>\n\
Sent: Friday, October 2, 2026 5:55 PM\n\
To: 田中 一郎 <tanaka@example.jp>\n\
Subject: RE: 図面\n\
\n\
田中様\n修正しました。\n\n{second}\n\n2 通目の本文です。"
    )
}

/// 見出しの順番が違っても（From が先頭でなくても）1 通に分けて差出人・日時・件名を読む。
#[test]
fn reads_headers_in_any_order() {
    let forms = [
        // Thunderbird の転送の見出し（値が同じ行）。
        "Subject: 図面\nDate: Fri, 2 Oct 2026 14:50:00 +0900\nFrom: 鈴木 次郎 <jiro@example.com>\nTo: 高橋 花子 <takahashi@example.com>",
        "From: 鈴木 次郎 <jiro@example.com>\nTo: 高橋 花子 <takahashi@example.com>\nDate: Fri, 2 Oct 2026 14:50:00 +0900\nSubject: 図面",
        "差出人: 鈴木 次郎 <jiro@example.com>\n件名: 図面\n日付: Fri, 2 Oct 2026 14:50:00 +0900\n宛先: 高橋 花子 <takahashi@example.com>",
    ];
    for second in forms {
        let s = split_reply(&outlook_then(second));
        assert_eq!(s.clean, "了解です。", "{second:?}");
        assert_eq!(s.quotes.len(), 2, "{second:?}");
        let q = &s.quotes[1];
        assert_eq!(
            q.quoted_from.as_deref(),
            Some("鈴木 次郎 <jiro@example.com>"),
            "{second:?}"
        );
        assert_eq!(
            q.quoted_at.as_deref(),
            Some("Fri, 2 Oct 2026 14:50:00 +0900"),
            "{second:?}"
        );
        assert_eq!(q.subject.as_deref(), Some("図面"), "{second:?}");
        assert_eq!(q.body, "2 通目の本文です。", "{second:?}");
        assert_eq!(s.quotes[0].body, "田中様\n修正しました。", "{second:?}");
    }
    // 本文の始まりでも、"From:" の上に続く見出しから引用とする。
    let s = split_reply(&format!("了解です。\n\n{}\n\n本文", forms[0]));
    assert_eq!(s.clean, "了解です。");
    assert_eq!(s.quotes.len(), 1);
    assert_eq!(s.quotes[0].subject.as_deref(), Some("図面"));
}

/// 「--- 引用元メッセージ ---」「--- 転送されたメール ---」で分け、「--- Original Message Ends ---」では分けない。
#[test]
fn reads_more_separator_forms() {
    for sep in ["--- 引用元メッセージ ---", "--- 転送されたメール ---"] {
        let second = format!(
            "{sep}\nFrom: 鈴木 次郎 <jiro@example.com>\nDate: Fri, 2 Oct 2026 14:50:00 +0900"
        );
        let s = split_reply(&outlook_then(&second));
        assert_eq!(s.quotes.len(), 2, "{sep:?}");
        assert_eq!(
            s.quotes[1].quoted_from.as_deref(),
            Some("鈴木 次郎 <jiro@example.com>")
        );
    }
    let s = split_reply(&outlook_then("--- Original Message Ends ---"));
    assert_eq!(s.quotes.len(), 1);
    assert!(s.quotes[0].body.contains("--- Original Message Ends ---"));
    assert!(s.quotes[0].body.ends_with("2 通目の本文です。"));
}
