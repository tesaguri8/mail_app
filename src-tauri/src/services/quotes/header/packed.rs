//! 1 行に詰まった見出し（"From：…To：…日時：…件名：…"）を、項目ごとの行に分ける。
//!
//! HTML の見出しを文字にしたとき、項目の間の改行が落ちて 1 行に詰まることがある
//! （2026-10-21 vaio: 「----- 引用元メッセージ -----」の次の 1 行）。そのままだと差出人の値に
//! 宛先・日時・件名まで入り、日時が読めない。行頭が既知の項目で、行の途中にも既知の項目が続くときだけ分ける。

use std::borrow::Cow;

use super::{header_field, Field};

/// 行の途中で区切りとみなす項目名（長いものから照らす。"送信日時" を "日時" より先に）。
const INNER_KEYS: &[&str] = &[
    "送信日時",
    "差出人",
    "送信者",
    "宛先",
    "件名",
    "日時",
    "日付",
    "Subject",
    "From",
    "Sent",
    "Date",
    "To",
    "Cc",
];

/// `line` の `at` から既知の項目名と「:」「：」が始まるなら、その項目名の長さ（バイト）。
fn key_at(line: &str, at: usize) -> Option<usize> {
    let rest = &line[at..];
    INNER_KEYS.iter().find_map(|k| {
        let after = rest.get(..k.len()).filter(|h| h.eq_ignore_ascii_case(k))?;
        rest[after.len()..]
            .starts_with([':', '：'])
            .then_some(after.len())
    })
}

/// 項目名が始まってよい位置か。英字の項目名は直前が英字でないこと（"mailto:" の "to:" を除く）、
/// 和文の項目名は直前が和文の項目名の一部でないこと（"送信日時" の "日時" を除く）。
fn boundary_ok(line: &str, at: usize, key_len: usize) -> bool {
    let prev = line[..at].chars().next_back();
    let key = &line[at..at + key_len];
    match prev {
        None => true,
        Some(p) if key.is_ascii() => !p.is_ascii_alphabetic(),
        Some(p) => {
            !INNER_KEYS.iter().any(|k| {
                k.ends_with(key)
                    && k.len() > key.len()
                    && line[..at].ends_with(&k[..k.len() - key.len()])
            }) && p != '：'
        }
    }
}

/// 1 行を、途中の既知の項目の前で分ける。分けなければ None。分けるのは、行頭が差出人の項目
/// （"From：" / "差出人：" 等）の行だけ（本文の申込書の「件名: … 日付: …」は分けない）。
fn split_packed(line: &str) -> Option<Vec<&str>> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('>') || !matches!(header_field(trimmed), Some((Field::From, _))) {
        return None;
    }
    let mut cuts = Vec::new();
    // 行頭の項目名のコロンより後ろから探す（"*From:*" の "From:" で切らない）。
    let start = line.find([':', '：'])? + 1;
    for (at, _) in line.char_indices().filter(|(i, _)| *i >= start) {
        if let Some(len) = key_at(line, at) {
            if boundary_ok(line, at, len) {
                cuts.push(at);
            }
        }
    }
    if cuts.is_empty() {
        return None;
    }
    let mut parts = Vec::with_capacity(cuts.len() + 1);
    let mut from = 0;
    for at in cuts {
        parts.push(line[from..at].trim_end());
        from = at;
    }
    parts.push(&line[from..]);
    Some(parts)
}

/// 本文のうち、1 行に詰まった見出しを項目ごとの行に分ける。分ける行が無ければ元のまま。
pub fn expand(plain: &str) -> Cow<'_, str> {
    if !plain.lines().any(|l| split_packed(l).is_some()) {
        return Cow::Borrowed(plain);
    }
    let lines: Vec<String> = plain
        .lines()
        .map(|l| split_packed(l).map_or_else(|| l.to_string(), |parts| parts.join("\n")))
        .collect();
    Cow::Owned(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_packed_header_line() {
        let packed = "From：\"Taro Yamada\" <taro@example.jp>To：\"佐藤 花子\" <hanako@example.jp>日時：2026/10/02 金 12:10件名：Re: 打ち合わせ";
        assert_eq!(
            expand(packed),
            "From：\"Taro Yamada\" <taro@example.jp>\nTo：\"佐藤 花子\" <hanako@example.jp>\n日時：2026/10/02 金 12:10\n件名：Re: 打ち合わせ"
        );
    }

    /// 1 項目だけの行、"mailto:"・"送信日時:"・本文の「日時：」は分けない。
    #[test]
    fn leaves_ordinary_lines() {
        for line in [
            "From: 山田 <taro@example.jp<mailto:taro@example.jp>>",
            "送信日時: 2026年10月2日 17:55",
            "本文の中の 日時：10月2日 と 件名：ご案内",
            "> From：a@example.jp To：b@example.jp",
        ] {
            assert_eq!(expand(line), line);
        }
    }
}
