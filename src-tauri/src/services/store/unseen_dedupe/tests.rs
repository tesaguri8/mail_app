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
