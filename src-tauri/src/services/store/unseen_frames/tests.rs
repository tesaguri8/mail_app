use super::*;

/// 試験用の未受信の枠（名前・アドレスは作り物）。
fn frame(from: &str, date: Option<&str>, body: &str) -> UnseenQuote {
    UnseenQuote {
        from: from.into(),
        sender: crate::services::quotes::sender::sender_label(from),
        at: date.map(str::to_string),
        date: date.map(str::to_string),
        guessed: false,
        guess_depth: None,
        subject: Some("スラブ案の見積り".into()),
        body: body.into(),
        quoted_in: 0,
        root_of_ref: None,
    }
}

const FROM: &str = "末松 太郎 <taro@example.invalid>";
const AT: &str = "2026-09-24T10:33:00+09:00";
const BODY: &str = "各位\nスラブ案の見積りをお送りします。\n合計は別紙のとおりです。";

fn bodies(lists: &[Vec<UnseenQuote>]) -> Vec<usize> {
    lists.iter().map(Vec::len).collect()
}

#[test]
fn the_same_unseen_quote_is_shown_once_above_the_oldest_reply() {
    // 返信 4 通が同じ元メールを引用し続ける。2 通目は自分の書いた分だけ（枠なし）。
    let mut lists = vec![
        vec![frame(FROM, Some(AT), BODY)],
        vec![],
        vec![frame(FROM, Some(AT), BODY)],
        vec![frame(
            FROM,
            Some(AT),
            &format!("> {}", BODY.replace('\n', "\n> ")),
        )],
    ];
    dedupe(&mut lists);
    assert_eq!(bodies(&lists), [1, 0, 0, 0]);
    assert_eq!(lists[0][0].body, BODY);
}

#[test]
fn a_longer_later_copy_moves_its_body_into_the_kept_frame() {
    let longer = format!("{BODY}\n\n（続き）図面は来週お送りします。");
    let mut lists = vec![
        vec![frame(FROM, Some(AT), BODY)],
        vec![frame(FROM, Some(AT), &longer)],
    ];
    dedupe(&mut lists);
    assert_eq!(bodies(&lists), [1, 0]);
    assert_eq!(lists[0][0].body, longer);
}

#[test]
fn different_content_from_the_same_sender_and_time_is_kept() {
    let other = "各位\n別件の請求書の宛名を確認させてください。よろしくお願いします。";
    let mut lists = vec![
        vec![frame(FROM, Some(AT), BODY)],
        vec![frame(FROM, Some(AT), other)],
    ];
    dedupe(&mut lists);
    assert_eq!(bodies(&lists), [1, 1]);
}

#[test]
fn different_sender_or_time_is_kept() {
    let mut lists = vec![
        vec![frame(FROM, Some(AT), BODY)],
        vec![frame("花子 <hanako@example.invalid>", Some(AT), BODY)],
        vec![frame(FROM, Some("2026-09-24T10:34:00+09:00"), BODY)],
    ];
    dedupe(&mut lists);
    assert_eq!(bodies(&lists), [1, 1, 1]);
}

#[test]
fn undated_and_guessed_frames_are_never_merged() {
    let mut guessed = frame(FROM, None, BODY);
    guessed.guessed = true;
    guessed.guess_depth = Some(1);
    let mut lists = vec![
        vec![frame(FROM, None, BODY), guessed.clone()],
        vec![frame(FROM, None, BODY), guessed],
    ];
    dedupe(&mut lists);
    assert_eq!(bodies(&lists), [2, 2]);
}

#[test]
fn a_short_body_is_not_merged_by_containment() {
    // 挨拶だけの短い本文は、長い本文に含まれていても同じとみなさない（一致ならまとめる）。
    let mut lists = vec![
        vec![frame(FROM, Some(AT), "お世話になります。")],
        vec![frame(FROM, Some(AT), &format!("お世話になります。{BODY}"))],
        vec![frame(FROM, Some(AT), "お世話になります。")],
    ];
    dedupe(&mut lists);
    assert_eq!(bodies(&lists), [1, 1, 0]);
}

#[test]
fn the_sender_is_compared_by_address_ignoring_case_and_name() {
    let mut lists = vec![
        vec![frame(FROM, Some(AT), BODY)],
        vec![frame(
            "'Taro' [mailto:TARO@example.invalid]",
            Some(AT),
            BODY,
        )],
    ];
    dedupe(&mut lists);
    assert_eq!(bodies(&lists), [1, 0]);
}

#[test]
fn width_and_quote_marks_do_not_split_the_same_frame() {
    // 段ごとに「>」が増え、メールソフトが英数字を全角にした写し。
    let wide =
        "各位\n> スラブ案の見積りをお送りします。\n> 合計は別紙のとおりです。　（ＰＤＦ　２枚）";
    let narrow = "各位\nスラブ案の見積りをお送りします。\n合計は別紙のとおりです。 (PDF 2枚)";
    let mut lists = vec![
        vec![frame(FROM, Some(AT), narrow)],
        vec![frame(FROM, Some(AT), wide)],
    ];
    dedupe(&mut lists);
    assert_eq!(bodies(&lists), [1, 0]);
}

#[test]
fn the_time_is_compared_by_the_minute_and_mixed_offsets_stay_apart() {
    let mut lists = vec![
        vec![frame(FROM, Some("2026-09-24T10:33:00"), BODY)],
        // 秒だけ違う（分は同じ）: 同じ。
        vec![frame(FROM, Some("2026-09-24T10:33:41"), BODY)],
        // 時差が片方にだけ書かれている: 迷うので別。
        vec![frame(FROM, Some(AT), BODY)],
        // 時差の書き方が違っても同じ時刻: 同じ。
        vec![frame(FROM, Some("2026-09-24T01:33:00+00:00"), BODY)],
    ];
    dedupe(&mut lists);
    assert_eq!(bodies(&lists), [1, 0, 1, 0]);
}

/// 試験用の会話のメールの日時。
fn times(dates: &[&str]) -> Vec<Option<MessageAt>> {
    dates.iter().map(|d| message_at(Some(d))).collect()
}

#[test]
fn a_dated_frame_moves_to_the_first_message_at_or_after_its_time() {
    // 会話: 9/24 13:22・10/5 9:36・10/6 15:10。4 通目（10/6）が 9/24 10:33 と 10/1 8:00 の 2 通を含む。
    let ts = times(&[
        "2026-09-24T13:22:00+09:00",
        "2026-10-05T09:36:00+09:00",
        "2026-10-06T15:10:00+09:00",
    ]);
    let mut lists = vec![
        vec![],
        vec![],
        vec![
            frame(
                FROM,
                Some("2026-10-01T08:00:00"),
                "十月一日の連絡です。日程の候補を三つお送りします。",
            ),
            frame(FROM, Some("2026-09-24T10:33:00"), BODY),
        ],
    ];
    place(&mut lists, &ts);
    assert_eq!(bodies(&lists), [1, 1, 0]);
    assert_eq!(lists[0][0].date.as_deref(), Some("2026-09-24T10:33:00"));
    assert_eq!(lists[1][0].date.as_deref(), Some("2026-10-01T08:00:00"));
}

#[test]
fn undated_guessed_and_newest_frames_stay_on_the_quoting_message() {
    let ts = times(&["2026-09-24T13:22:00+09:00", "2026-10-05T09:36:00+09:00"]);
    let mut guessed = frame(FROM, None, BODY);
    guessed.guessed = true;
    let mut lists = vec![
        vec![],
        vec![
            frame(FROM, None, BODY),
            guessed,
            // 前のメールより新しい（含んでいたメールの直前）: 動かない。
            frame(FROM, Some("2026-10-05T09:00:00+09:00"), BODY),
        ],
    ];
    place(&mut lists, &ts);
    assert_eq!(bodies(&lists), [0, 3]);
}

#[test]
fn a_frame_in_the_same_minute_as_a_message_goes_above_it() {
    let ts = times(&["2026-09-24T10:33:20+09:00", "2026-10-05T09:36:00+09:00"]);
    let mut lists = vec![vec![], vec![frame(FROM, Some("2026-09-24T10:33:00"), BODY)]];
    place(&mut lists, &ts);
    assert_eq!(bodies(&lists), [1, 0]);
}
