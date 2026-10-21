//! 1 行に詰まった見出し（"From：…To：…日時：…件名：…"）を、項目ごとの行に分ける。
//!
//! HTML の見出しを文字にしたとき、項目の間の改行が落ちて 1 行に詰まることがある
//! （2026-10-21 vaio: 「----- 引用元メッセージ -----」の次の 1 行）。そのままだと差出人の値に
//! 宛先・日時・件名まで入り、日時が読めない。行頭が既知の項目で、行の途中にも既知の項目が続くときだけ分ける。

use std::borrow::Cow;

use super::{header_field, is_separator_text, Field};

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

/// 区切りの文字列（"----- 引用元メッセージ -----" / "-----Original Message-----"）の後ろに、同じ行で
/// 差出人の項目が続く行を、区切りと見出しに分ける。分けなければ None。
fn split_separator(line: &str) -> Option<(&str, &str)> {
    if line.trim_start().starts_with('>') || !is_separator_text(line) {
        return None;
    }
    let cut = line.char_indices().map(|(i, _)| i).find(|&at| {
        key_at(line, at).is_some_and(|len| {
            boundary_ok(line, at, len)
                && matches!(header_field(&line[at..]), Some((Field::From, _)))
        })
    })?;
    let sep = line[..cut].trim_end();
    // 区切りの文字列が行頭から始まっている（本文の一文の途中の「元のメッセージ」ではない）。
    is_separator_text(sep).then_some((sep, &line[cut..]))
}

/// 件名の項目の値の後ろに、半角の空白 2 つ以上を挟んで本文が続く形（詰まった見出しの最後）を、
/// 件名と本文に分ける。件名の項目でなければ、または空白 2 つ以上が無ければ、そのまま 1 つ。
fn split_subject_body(part: &str) -> Vec<&str> {
    if !matches!(header_field(part), Some((Field::Subject, _))) {
        return vec![part];
    }
    let value_at = part.find([':', '：']).map_or(0, |i| {
        i + part[i..].chars().next().map_or(1, char::len_utf8)
    });
    // 値の頭の空白は飛ばしてから探す（"件名：  Re: …" で切らない）。
    let lead = part[value_at..].len() - part[value_at..].trim_start().len();
    let from = value_at + lead;
    match part[from..].find("  ") {
        Some(gap) if gap > 0 => {
            let cut = from + gap;
            vec![part[..cut].trim_end(), part[cut..].trim_start()]
        }
        _ => vec![part],
    }
}

/// 1 行を、詰まった見出しなら区切り・項目・本文の行に分ける。分けなければ None。
fn expand_line(line: &str) -> Option<Vec<&str>> {
    let (sep, rest) = match split_separator(line) {
        Some((sep, rest)) => (Some(sep), rest),
        None => (None, line),
    };
    let fields = split_packed(rest);
    if sep.is_none() && fields.is_none() {
        return None;
    }
    let fields = fields.unwrap_or_else(|| vec![rest]);
    let last = fields.len() - 1;
    let mut out: Vec<&str> = sep.into_iter().collect();
    for (k, part) in fields.into_iter().enumerate() {
        if k == last {
            out.extend(split_subject_body(part));
        } else {
            out.push(part);
        }
    }
    Some(out)
}

/// 本文のうち、1 行に詰まった見出しを項目ごとの行に分ける。分ける行が無ければ元のまま。
/// 区切りの文字列から始まって同じ行に見出しが続く行（"----- 引用元メッセージ ----- From：…"）は、
/// 区切り・項目・本文（件名の値の後ろに空白 2 つ以上を挟んで続く部分）の行に分ける。
pub fn expand(plain: &str) -> Cow<'_, str> {
    if !plain.lines().any(|l| expand_line(l).is_some()) {
        return Cow::Borrowed(plain);
    }
    let lines: Vec<String> = plain
        .lines()
        .map(|l| expand_line(l).map_or_else(|| l.to_string(), |parts| parts.join("\n")))
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

    /// 区切りの文字列の後ろに同じ行で見出しが続く行は、区切りと見出しに分け、件名の後ろの空白 2 つ以上の
    /// 先を本文の行にする（件名の中の全角の空白・空白 1 つでは分けない）。
    #[test]
    fn splits_separator_header_and_body_on_one_line() {
        let line = "----- 引用元メッセージ ----- From：\"山田\" <taro@example.jp>日時：2023/10/02 月 9:30件名：Re: 打ち合わせ　の件  本文です。";
        assert_eq!(
            expand(line),
            "----- 引用元メッセージ -----\nFrom：\"山田\" <taro@example.jp>\n日時：2023/10/02 月 9:30\n件名：Re: 打ち合わせ　の件\n本文です。"
        );
        let line = "-----Original Message----- From: Taro <taro@example.jp> Sent: Monday, October 2, 2023 9:30 AM Subject: Re: plan";
        assert_eq!(
            expand(line),
            "-----Original Message-----\nFrom: Taro <taro@example.jp>\nSent: Monday, October 2, 2023 9:30 AM\nSubject: Re: plan"
        );
        // 見出しの続かない区切り行・本文の「元のメッセージ」は分けない。
        for line in [
            "----- 引用元メッセージ -----",
            "元のメッセージをご確認ください。",
        ] {
            assert_eq!(expand(line), line);
        }
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
