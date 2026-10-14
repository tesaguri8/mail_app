//! Outlook・Gmail・Thunderbird の「元のメッセージ」見出しを読む。
//!
//! ```text
//! -----Original Message-----          ← 区切り（無い書式もある）
//! From: 山田 花子 <hanako@example.com>
//! Sent: Friday, October 2, 2026 5:55 PM
//! To: 佐藤 一郎 <ichiro@example.com>
//! Subject: RE: 打ち合わせの件
//!                                     ← 空行で見出しが終わる
//! 本文…
//! ```

/// 見出しの項目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    From,
    Date,
    Subject,
    /// To / Cc など、読むが使わない項目。
    Other,
}

/// "Key: value" の行を (項目, 値) に読む。知らない項目なら None。
///
/// HTML の太字を text/plain に落とした "*From:* 山田 <…>" も読む（項目名の前後の `*` と、
/// 値の先頭に残る `*` を外す）。
fn header_field(line: &str) -> Option<(Field, &str)> {
    let l = line.trim();
    let (key, value) = l.split_once([':', '：'])?;
    let field = match key.trim().trim_matches('*').trim().to_lowercase().as_str() {
        "from" | "差出人" | "送信者" => Field::From,
        "sent" | "date" | "送信日時" | "日付" | "日時" => Field::Date,
        "subject" | "件名" => Field::Subject,
        "to" | "cc" | "bcc" | "宛先" | "reply-to" => Field::Other,
        _ => return None,
    };
    Some((field, value.trim().trim_start_matches('*').trim_start()))
}

/// 「元のメッセージ」の区切り行か（"-----Original Message-----" / "-----元のメッセージ-----" /
/// Gmail の "---------- Forwarded message ---------" / Outlook の下線 / 転送の区切り）。
pub fn is_separator(line: &str) -> bool {
    let l = line.trim();
    if l.starts_with('>') {
        return false;
    }
    let lower = l.to_lowercase();
    lower.contains("original message")
        || lower.contains("forwarded message")
        || l.contains("元のメッセージ")
        || l.contains("転送メッセージ")
        || l.starts_with("________________")
}

/// 見出しの最初の行（"From:" / "差出人:" / "送信者:"）なら、その値（差出人）を返す。
pub fn from_value(line: &str) -> Option<&str> {
    if line.trim_start().starts_with('>') {
        return None;
    }
    match header_field(line) {
        Some((Field::From, value)) => Some(value),
        _ => None,
    }
}

/// 見出しの最初の行（"From:" / "差出人:" / "送信者:"）か。
pub fn is_from_line(line: &str) -> bool {
    from_value(line).is_some()
}

/// 見出しの日時の行（"Sent:" / "Date:" / "送信日時:" / "日付:"）か。
pub fn is_date_line(line: &str) -> bool {
    !line.trim_start().starts_with('>') && matches!(header_field(line), Some((Field::Date, _)))
}

/// 見出しを読んだ結果。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub from: Option<String>,
    pub at: Option<String>,
    pub subject: Option<String>,
    /// 本文が始まる行（ブロック内の添字）。
    pub body_start: usize,
}

/// 見出しとして読む行数の上限（宛先が多いと To が折り返して長くなる）。
const MAX_HEADER_LINES: usize = 20;

/// 値が空なら None。
fn non_empty(v: &str) -> Option<String> {
    let v = v.trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// 引用ブロックの先頭の見出しを読む。区切り行・属性行（`skip_first`）を飛ばし、"Key: value" の
/// 並びを空行まで読む。見出しが無ければ、本文は区切り行・属性行の次から始まる。
pub fn parse_header(block: &[&str], skip_first: bool) -> Header {
    let mut i = usize::from(skip_first || block.first().is_some_and(|l| is_separator(l)));
    while block.get(i).is_some_and(|l| l.trim().is_empty()) {
        i += 1;
    }
    let mut h = Header {
        body_start: i,
        ..Header::default()
    };
    let mut seen = false;
    for (j, line) in block.iter().enumerate().skip(i).take(MAX_HEADER_LINES) {
        if line.trim().is_empty() {
            if seen {
                h.body_start = j + 1;
            }
            break;
        }
        match header_field(line) {
            Some((field, value)) => {
                seen = true;
                let slot = match field {
                    Field::From => &mut h.from,
                    Field::Date => &mut h.at,
                    Field::Subject => &mut h.subject,
                    Field::Other => {
                        h.body_start = j + 1;
                        continue;
                    }
                };
                if slot.is_none() {
                    *slot = non_empty(value);
                }
                h.body_start = j + 1;
            }
            // 見出しの途中の、項目名の無い行は折り返し（長い To）とみなす。見出しより前なら本文。
            None if seen => h.body_start = j + 1,
            None => break,
        }
    }
    h
}

/// 引用の差出人の書き方（"山田 花子 <hanako@example.com>" / "hanako@example.com" /
/// "'山田 花子' [mailto:hanako@example.com]" / "山田 花子"）から、アドレスと名前を取り出す。
pub fn split_sender(from: &str) -> (Option<String>, Option<String>) {
    let s = from.trim();
    let addr = if let Some(open) = s.find("mailto:") {
        let rest = &s[open + "mailto:".len()..];
        Some(
            rest.split([']', '>', ' '])
                .next()
                .unwrap_or(rest)
                .to_string(),
        )
    } else if let (Some(a), Some(b)) = (s.find('<'), s.rfind('>')) {
        (a < b).then(|| s[a + 1..b].to_string())
    } else {
        s.split_whitespace()
            .find(|w| w.contains('@'))
            .map(str::to_string)
    }
    .map(|a| a.trim().to_lowercase())
    .filter(|a| a.contains('@'));
    // 名前は、アドレスの部分（<...> / [mailto:...] / アドレス単体）より前。
    let name_end = s.find(['<', '[']).unwrap_or(s.len());
    let name = s[..name_end]
        .trim()
        .trim_matches(['"', '\'', '“', '”'])
        .trim();
    let name = (!name.is_empty() && !name.contains('@')).then(|| name.to_string());
    (addr, name)
}

/// 引用の差出人を画面に出す形にする（名前があれば名前、無ければアドレス、どちらも取れなければ見出しのまま）。
/// "<addr> <mailto:addr>" / "addr <addr> <mailto:addr>" のように、名前の欄にアドレスが入った書き方でも
/// アドレスだけを出す。
pub fn sender_label(from: &str) -> String {
    match split_sender(from) {
        (_, Some(name)) => name,
        (Some(addr), None) => addr,
        (None, None) => from.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_outlook_english_header() {
        let block = [
            "-----Original Message-----",
            "From: Hanako Yamada <hanako@example.com>",
            "Sent: Friday, October 2, 2026 5:55 PM",
            "To: Ichiro Sato <ichiro@example.com>; Jiro Suzuki",
            "  <jiro@example.com>",
            "Cc: team@example.com",
            "Subject: RE: drawings",
            "",
            "Please check the attached.",
        ];
        let h = parse_header(&block, false);
        assert_eq!(
            h.from.as_deref(),
            Some("Hanako Yamada <hanako@example.com>")
        );
        assert_eq!(h.at.as_deref(), Some("Friday, October 2, 2026 5:55 PM"));
        assert_eq!(h.subject.as_deref(), Some("RE: drawings"));
        assert_eq!(block[h.body_start], "Please check the attached.");
    }

    #[test]
    fn reads_outlook_japanese_header_without_separator() {
        let block = [
            "差出人: 山田 花子 <hanako@example.com>",
            "送信日時: 2026年10月2日 17:55",
            "宛先: 佐藤 一郎",
            "件名: RE: 図面の件",
            "",
            "ご確認ください。",
        ];
        let h = parse_header(&block, false);
        assert_eq!(h.from.as_deref(), Some("山田 花子 <hanako@example.com>"));
        assert_eq!(h.at.as_deref(), Some("2026年10月2日 17:55"));
        assert_eq!(block[h.body_start], "ご確認ください。");
    }

    #[test]
    fn attribution_without_header_starts_body_on_next_line() {
        let block = [
            "On Mon, 30 Jun 2026 10:00, Taro <t@example.com> wrote:",
            "> hello",
        ];
        let h = parse_header(&block, true);
        assert_eq!(h.from, None);
        assert_eq!(h.body_start, 1);
    }

    /// HTML の太字が text/plain に残った "*From:*" / "*Sent:*" も見出しとして読む。
    #[test]
    fn reads_bold_marked_header() {
        let block = [
            "*From:*山田 花子<hanako@example.com>",
            "*Sent:* Wednesday, October 7, 2026 7:36 PM",
            "*To:* 佐藤 一郎",
            "*Subject:* RE: 図面",
            "",
            "本文です。",
        ];
        assert!(is_from_line(block[0]));
        assert!(is_date_line(block[1]));
        let h = parse_header(&block, false);
        assert_eq!(h.from.as_deref(), Some("山田 花子<hanako@example.com>"));
        assert_eq!(h.at.as_deref(), Some("Wednesday, October 7, 2026 7:36 PM"));
        assert_eq!(h.subject.as_deref(), Some("RE: 図面"));
        assert_eq!(block[h.body_start], "本文です。");
    }

    #[test]
    fn splits_sender_forms() {
        assert_eq!(
            split_sender("山田 花子 <Hanako@Example.com>"),
            (Some("hanako@example.com".into()), Some("山田 花子".into()))
        );
        assert_eq!(
            split_sender("'Yamada Hanako' [mailto:hanako@example.com]"),
            (
                Some("hanako@example.com".into()),
                Some("Yamada Hanako".into())
            )
        );
        assert_eq!(
            split_sender("hanako@example.com"),
            (Some("hanako@example.com".into()), None)
        );
        assert_eq!(split_sender("山田 花子"), (None, Some("山田 花子".into())));
    }

    /// 実物の引用にあった差出人の書き方（名前とアドレスは作り物）。
    #[test]
    fn labels_sender_by_name_or_address() {
        assert_eq!(
            sender_label("<taro@maeda.example> <mailto:taro@maeda.example>"),
            "taro@maeda.example"
        );
        assert_eq!(
            sender_label("taro@maeda.example <taro@maeda.example> <mailto:taro@maeda.example>"),
            "taro@maeda.example"
        );
        assert_eq!(sender_label("高橋 健<12345TAKA@city.example.lg.jp>"), "高橋 健");
        assert_eq!(
            sender_label("木材商事　　山田花子<hanako@mokuzai.example.jp>"),
            "木材商事　　山田花子"
        );
        assert_eq!(sender_label("鈴木次郎"), "鈴木次郎");
        assert_eq!(sender_label("Hanako@Example.com"), "hanako@example.com");
    }
}
