//! 本文と引用の分け方の試験（属性行の書き方・`>` 引用・署名との関係）。名前は作り物。

use super::*;

#[test]
fn strips_english_top_post_and_signature() {
    let body = "Thanks, that works for me.\n\n--\nJohn\n\nOn Mon, 30 Jun 2026 10:00:00, Taro <taro@example.com> wrote:\n> Are you free next week?\n> Tuesday afternoon?";
    let s = split_reply(body);
    assert_eq!(s.clean, "Thanks, that works for me.");
    assert_eq!(s.quotes.len(), 1);
    assert_eq!(
        s.quotes[0].quoted_from.as_deref(),
        Some("Taro <taro@example.com>")
    );
    assert!(s.quotes[0].quoted_at.is_some());
}

#[test]
fn keeps_new_text_after_inline_quote() {
    // 本文途中にインライン引用（`>`）があり、その後にも新規テキストがある返信。
    // 末尾の属性行（"On … wrote:"）を引用開始とし、インライン引用の後の新規文は clean に残す。
    let body = "伊佐様\nお世話になっております。\n> ・4/6 (月) 午後\n> ・4/7 (火) 午前/午後\n→いづれかで大丈夫です。\n宜しくお願い致します。\n\nOn 2026/03/31 16:27, Isa <isa@example.com> wrote:\n> 末松 さま\n> お世話になっております。";
    let s = split_reply(body);
    assert!(
        s.clean.contains("→いづれかで大丈夫です。"),
        "インライン引用の後の新規文が clean に残る"
    );
    assert!(s.clean.contains("宜しくお願い致します。"));
    assert!(s.clean.contains("お世話になっております。"));
    // 末尾の返信引用（属性行以降）は clean に含まない。
    assert!(!s.clean.contains("末松 さま"));
    assert_eq!(s.quotes.len(), 1);
}

#[test]
fn strips_japanese_attribution() {
    let body = "承知しました。火曜の午後でお願いします。\n\n2026年6月30日(月) 10:00 佐藤 <sato@example.com>:\n> 来週の打ち合わせは可能ですか？";
    let s = split_reply(body);
    assert_eq!(s.clean, "承知しました。火曜の午後でお願いします。");
    assert_eq!(s.quotes.len(), 1);
    // 曜日の「(月)」を月と取り違えなくなったので、日時と「名前 <addr>」まで読む。
    assert_eq!(
        s.quotes[0].quoted_from.as_deref(),
        Some("佐藤 <sato@example.com>")
    );
    assert_eq!(
        s.quotes[0].quoted_at.as_deref(),
        Some("2026年6月30日(月) 10:00")
    );
}

#[test]
fn strips_japanese_wrote_line() {
    let body =
        "了解です。\n\n山田太郎さんは2026年6月30日 10:00に書きました:\n> よろしくお願いします";
    let s = split_reply(body);
    assert_eq!(s.clean, "了解です。");
    assert_eq!(s.quotes.len(), 1);
    assert_eq!(s.quotes[0].quoted_from.as_deref(), Some("山田太郎"));
}

#[test]
fn strips_apple_mail_addr_colon_attribution() {
    // Apple Mail / iOS メール系: "…, 名前 <addr>:" で終わる属性行（"On…wrote:" でも西暦始まりでもない）。
    // 実データで 400 通超がこの形の属性行を clean_body に残していた回帰の防止。
    let body = "松田様、伊佐様\n\n\
本年も引き続き宜しくお願い致します。\n\n\
末松\n\n\
Dec 26, 2025, 17:25 +0900, 伊佐　日和 <isa@matsudamariko.com>:\n\n\
> 末松 様\n> お世話になっております。";
    let s = split_reply(body);
    assert_eq!(
        s.clean,
        "松田様、伊佐様\n\n本年も引き続き宜しくお願い致します。\n\n末松"
    );
    assert_eq!(s.quotes.len(), 1);
    // 日時が読めるので、日時と「名前 <addr>」を取る。
    assert_eq!(
        s.quotes[0].quoted_from.as_deref(),
        Some("伊佐　日和 <isa@matsudamariko.com>")
    );
    assert_eq!(
        s.quotes[0].quoted_at.as_deref(),
        Some("Dec 26, 2025, 17:25 +0900")
    );
}

#[test]
fn addr_colon_attribution_without_gt_body() {
    // 引用本文が `>` 無し（Apple Mail の text/plain がインデントのみ）でも属性行で切れる。
    let body = "了解しました。\n\nNov 5, 2024, 3:15 PM +0900, shiradou <siradou@example.com>:\n本文の引用がそのまま続く";
    assert_eq!(split_reply(body).clean, "了解しました。");
}

#[test]
fn colon_line_without_angle_email_is_not_attribution() {
    // 末尾コロンでも山括弧アドレスが無ければ属性行扱いしない（本文の見出し等を守る）。
    let body = "変更点は以下の通りです:\n- 1点目\n- 2点目";
    let s = split_reply(body);
    assert_eq!(s.clean, body.trim());
    assert!(s.quotes.is_empty());
}

#[test]
fn no_quote_keeps_whole_body() {
    let body = "初めまして。お世話になります。\n\nよろしくお願いします。";
    let s = split_reply(body);
    assert_eq!(s.clean, body.trim());
    assert!(s.quotes.is_empty());
}

#[test]
fn bare_gt_quote_without_attribution() {
    let body = "はい、大丈夫です。\n> 明日は来られますか？";
    let s = split_reply(body);
    assert_eq!(s.clean, "はい、大丈夫です。");
    assert_eq!(s.quotes.len(), 1);
}

#[test]
fn splits_rondine_own_reply_format() {
    // Rondine が生成する返信（compose.quoteHeader: "{{date}} に {{from}} さんが書きました:"）を
    // 入れ子で重ねたもの。新規部分（再テスト4）だけが残るべき。
    let body = "再テスト4\n\n\
2026/7/2 0:24:21 に suematsu@sng-design.com さんが書きました:\n\
> 再テスト3\n\
> \n\
> 2026/7/1 20:36:04 に suematsu@sng-design.com さんが書きました:\n\
> > 再テスト2\n\
> > \n\
> > 2026/7/1 18:09:32 に suematsu@sng-design.com さんが書きました:\n\
> > > 再テスト\n\
> > > \n\
> > > 2026-07-01T08:55:35Z に suematsu@sng-design.com さんが書きました:\n\
> > > > test";
    let s = split_reply(body);
    assert_eq!(s.clean, "再テスト4", "clean was: {:?}", s.clean);
}

#[test]
fn body_sentence_ending_with_wrote_is_not_a_boundary() {
    // 本文中の「〜書きました:」（日付もアドレスも無い）は属性行と誤認しない。
    let body = "議事録をまとめて共有すると彼は書きました:\nよろしくお願いします。";
    let s = split_reply(body);
    assert_eq!(s.clean, body.trim());
    assert!(s.quotes.is_empty());
}

#[test]
fn attribution_with_date_or_email_still_splits() {
    // 日付/アドレスの裏付けがあれば従来どおり属性行として切る。
    let with_email =
        "本文です。\n\nsato@example.com さんが書きました:\n> 以前のメール";
    assert_eq!(split_reply(with_email).clean, "本文です。");
    let with_date = "本文です。\n\n2026/07/03 10:00 に 佐藤 さんが書きました:\n> 以前のメール";
    assert_eq!(split_reply(with_date).clean, "本文です。");
}

/// 日本語版・中国語版・フランス語版の属性行と、全角コロンの "wrote："（名前は作り物）。
#[test]
fn reads_more_attribution_forms() {
    let forms = [
        (
            "山田 花子 <hanako@example.com> のメール:",
            Some("hanako@example.com"),
        ),
        (
            "2026/10/20 11:21、山田 花子 <hanako@example.com> のメール:",
            Some("山田 花子 <hanako@example.com>"),
        ),
        (
            "山田 花子 <hanako@example.com> 于2026年10月20日周二 11:21写道：",
            Some("山田 花子 <hanako@example.com>"),
        ),
        (
            "Le mar. 20 oct. 2026 à 11:21, Hanako <hanako@example.com> a écrit :",
            Some("hanako@example.com"),
        ),
        (
            "Hanako <hanako@example.com> wrote：",
            Some("hanako@example.com"),
        ),
    ];
    for (attr, from) in forms {
        let s = split_reply(&format!("了解です。\n\n{attr}\n> 図面を送ります。"));
        assert_eq!(s.clean, "了解です。", "{attr:?}");
        assert_eq!(s.quotes.len(), 1, "{attr:?}");
        assert_eq!(s.quotes[0].quoted_from.as_deref(), from, "{attr:?}");
    }
    let s = split_reply(
        "了解です。\n\nOn Mon, 30 Jun 2026 10:00, Taro <t@example.com> wrote：\n> hello",
    );
    assert_eq!(
        s.quotes[0].quoted_from.as_deref(),
        Some("Taro <t@example.com>")
    );
    assert_eq!(
        s.quotes[0].quoted_at.as_deref(),
        Some("Mon, 30 Jun 2026 10:00")
    );
    // 日付が先の iPhone の形は、日時も読む（読点の後ろを差出人に）。
    let s = split_reply(
        "了解です。\n\n2026/10/02 12:10、佐藤 一郎 <ichiro@example.jp> のメール:\n\n> 高橋様",
    );
    assert_eq!(
        s.quotes[0].quoted_from.as_deref(),
        Some("佐藤 一郎 <ichiro@example.jp>")
    );
    assert_eq!(s.quotes[0].quoted_at.as_deref(), Some("2026/10/02 12:10"));
    // アドレスの無い「写道：」は本文の一文とみなす。
    let body = "他这样写道：\n今天天气很好。";
    assert!(split_reply(body).quotes.is_empty());
}

/// 日時が先で差出人が続く属性行から、差出人と日時の両方を取る（vaio の集計で多かった形。名前は作り物）。
#[test]
fn reads_dates_of_attribution_lines() {
    let forms = [
        // Apple Mail（月違い・午前午後・24 時間）
        (
            "Sep 9, 2026, 9:09 AM +0900, 山田 花子 <hanako@example.com>:",
            "山田 花子 <hanako@example.com>",
            "Sep 9, 2026, 9:09 AM +0900",
        ),
        (
            "Jun 3, 2026, 4:12 PM +0900, hanako@example.com:",
            "hanako@example.com",
            "Jun 3, 2026, 4:12 PM +0900",
        ),
        (
            "Jun 3, 2026, 4:12 PM +0900, hanako@example.com <mailto:hanako@example.com> :",
            "hanako@example.com <mailto:hanako@example.com>",
            "Jun 3, 2026, 4:12 PM +0900",
        ),
        (
            "Dec 26, 2025, 17:25 +0900, 山田 花子 <hanako@example.com>:",
            "山田 花子 <hanako@example.com>",
            "Dec 26, 2025, 17:25 +0900",
        ),
        // Gmail 日本語版
        (
            "2026年10月2日(金) 17:55 山田 花子 <hanako@example.com>:",
            "山田 花子 <hanako@example.com>",
            "2026年10月2日(金) 17:55",
        ),
        (
            "2026年10月2日(金) 午後5:55 山田 花子 <hanako@example.com>:",
            "山田 花子 <hanako@example.com>",
            "2026年10月2日(金) 午後5:55",
        ),
        // 全角コロン
        (
            "2026年10月2日 17:55:00 hanako@example.com：",
            "hanako@example.com",
            "2026年10月2日 17:55:00",
        ),
        // 「に」の形
        (
            "2026/10/2 12:10:00 に hanako@example.com 山田 花子:",
            "hanako@example.com 山田 花子",
            "2026/10/2 12:10:00",
        ),
        // 中国語
        (
            "在 2026-10-02 12:10:00，\"hanako@example.com\" <hanako@example.com> 写道：",
            "\"hanako@example.com\" <hanako@example.com>",
            "2026-10-02 12:10:00",
        ),
        (
            "山田 花子 <hanako@example.com> 于2026年10月20日周二 11:21写道：",
            "山田 花子 <hanako@example.com>",
            "2026年10月20日周二 11:21",
        ),
        // Gmail 英語版（日付が数字）
        (
            "On 2026/10/02 12:10, 山田 花子 wrote:",
            "山田 花子",
            "2026/10/02 12:10",
        ),
        // iPhone（読点の前後の空白・カンマ・名前なし）
        (
            "2026/10/02 12:10、山田 花子 <hanako@example.com>のメール:",
            "山田 花子 <hanako@example.com>",
            "2026/10/02 12:10",
        ),
        (
            "2026/10/02 12:10 、 山田 花子 <hanako@example.com> のメール:",
            "山田 花子 <hanako@example.com>",
            "2026/10/02 12:10",
        ),
        (
            "2026/10/02 12:10, 山田 花子 <hanako@example.com>のメール:",
            "山田 花子 <hanako@example.com>",
            "2026/10/02 12:10",
        ),
        (
            "2026/10/02 12:10、<hanako@example.com>のメール:",
            "<hanako@example.com>",
            "2026/10/02 12:10",
        ),
    ];
    for (attr, from, at) in forms {
        let s = split_reply(&format!("了解です。\n\n{attr}\n\n> 図面を送ります。"));
        assert_eq!(s.clean, "了解です。", "{attr:?}");
        assert_eq!(s.quotes.len(), 1, "{attr:?}");
        assert_eq!(s.quotes[0].quoted_from.as_deref(), Some(from), "{attr:?}");
        assert_eq!(s.quotes[0].quoted_at.as_deref(), Some(at), "{attr:?}");
        assert!(when::parse_quoted_time(at).is_some(), "{attr:?}");
    }
}

/// 日時を取るのは、日時として読めて差出人にアドレスがあるときだけ（本文の時刻入りの一文では日時を取らない）。
#[test]
fn does_not_take_dates_from_body_sentences() {
    // アドレスが無い: 本文のまま。
    let body = "了解です。\n2026年10月2日(金) 17:55 に伺います。\n以上";
    assert!(split_reply(body).quotes.is_empty());
    // 日時が読めない Apple Mail の形は、これまでどおり差出人だけ。
    let s = split_reply("了解です。\n\nyesterday, 山田 花子 <hanako@example.com>:\n> 図面");
    assert_eq!(s.quotes.len(), 1);
    assert_eq!(
        s.quotes[0].quoted_from.as_deref(),
        Some("hanako@example.com")
    );
    assert_eq!(s.quotes[0].quoted_at, None);
}

#[test]
fn fingerprint_is_whitespace_stable() {
    assert_eq!(fingerprint("hello   world"), fingerprint("hello world\n"));
    assert_ne!(fingerprint("a"), fingerprint("b"));
}

/// 手元確認用: 実物の本文を `RONDINE_QUOTE_SAMPLE`（ファイル。先頭 3 行は DB の行情報）から読み、
/// 枠ごとの差出人・日時・本文の最初と最後の行を出す。本物の本文はリポジトリに入れない。
/// `RONDINE_QUOTE_SAMPLE=… cargo test dump_quote_sample -- --ignored --nocapture`
#[test]
#[ignore = "手元の実物の本文が要る"]
fn dump_quote_sample() {
    let Ok(path) = std::env::var("RONDINE_QUOTE_SAMPLE") else {
        return;
    };
    let text = std::fs::read_to_string(path).unwrap();
    let body: String = text.lines().skip(3).collect::<Vec<_>>().join("\n");
    let s = split_reply(&body);
    for q in &s.quotes {
        let lines: Vec<&str> = q.body.lines().filter(|l| !l.trim().is_empty()).collect();
        println!(
            "#{} from={:?} at={:?} subject={:?} lines={}\n    first={:?}\n    last={:?}",
            q.order,
            q.quoted_from,
            q.quoted_at,
            q.subject,
            lines.len(),
            lines.first(),
            lines.last()
        );
    }
}

mod forms;
