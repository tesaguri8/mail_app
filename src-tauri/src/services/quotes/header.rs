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
    /// 宛先（To / 宛先）。読むが使わない（見出しのかたまりの手がかりには数える）。
    To,
    /// Cc。読むが使わない（見出しのかたまりの手がかりには数える）。
    Cc,
    /// Bcc / Reply-To など、読むが使わない項目。
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
        "to" | "宛先" => Field::To,
        "cc" => Field::Cc,
        "bcc" | "reply-to" | "返信先" => Field::Other,
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

/// 見出しの項目の行か（差出人・日付・宛先・件名・Cc など。`>` の付いた行は除く）。
pub fn is_header_line(line: &str) -> bool {
    !line.trim_start().starts_with('>') && header_field(line).is_some()
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

/// 見出しとして読む行数の上限（宛先が多いと To が折り返して長くなる）。空行は数えない。
const MAX_HEADER_LINES: usize = 20;

/// 項目名と値が別の行にある書式で、項目名と値・値と次の項目名の間に挟まってよい空行の数。
const MAX_TABLE_GAP: usize = 3;

/// 値が空なら None。
fn non_empty(v: &str) -> Option<String> {
    let v = v.trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// `from` の次から、空行を `MAX_TABLE_GAP` 個まで飛ばした先の行の添字。
fn next_non_blank(block: &[&str], from: usize) -> Option<usize> {
    (from + 1..block.len())
        .take(MAX_TABLE_GAP + 1)
        .find(|&k| !block[k].trim().is_empty())
}

/// 項目名と値が別の行にある書式か。Thunderbird が HTML の表（項目名の欄・値の欄）を文字にすると
/// こうなる。最初の項目の値が空で、空行を挟んだ次の行が項目でなければ、その行を値とみなす。
///
/// ```text
/// -------- Forwarded Message --------
/// Subject:
///
/// RE: 打ち合わせの件
///
/// Date:
///
/// Tue, 20 Oct 2026 11:21:08 +0900
/// ```
fn is_table_form(block: &[&str], first_field: usize) -> bool {
    let value_empty = matches!(header_field(block[first_field]), Some((_, v)) if v.is_empty());
    value_empty
        && next_non_blank(block, first_field).is_some_and(|k| header_field(block[k]).is_none())
}

/// 引用ブロックの先頭の見出しを読む。区切り行・属性行（`skip_first`）を飛ばし、"Key: value" の
/// 並びを空行まで読む。見出しが無ければ、本文は区切り行・属性行の次から始まる。
///
/// 項目名と値が別の行にある書式（[`is_table_form`]）では、値は空行を挟んだ次の行から取り、
/// 空行の先に次の項目が続く間は見出しとして読み続ける。
pub fn parse_header(block: &[&str], skip_first: bool) -> Header {
    let mut i = usize::from(skip_first || block.first().is_some_and(|l| is_separator(l)));
    while block.get(i).is_some_and(|l| l.trim().is_empty()) {
        i += 1;
    }
    let mut h = Header {
        body_start: i,
        ..Header::default()
    };
    let table = i < block.len() && is_table_form(block, i);
    let mut seen = false;
    let mut read = 0;
    let mut j = i;
    while j < block.len() && read < MAX_HEADER_LINES {
        let line = block[j];
        if line.trim().is_empty() {
            // 表の形なら、空行の先に次の項目が続く。そうでなければ空行で見出しが終わる。
            match next_non_blank(block, j) {
                Some(k) if table && header_field(block[k]).is_some() => {
                    j = k;
                    continue;
                }
                _ => {
                    if seen {
                        h.body_start = j + 1;
                    }
                    break;
                }
            }
        }
        read += 1;
        match header_field(line) {
            Some((field, value)) => {
                seen = true;
                // 表の形で値が空なら、値は空行を挟んだ次の行（項目名の行なら値は空のまま）。
                let (value, end) = match next_non_blank(block, j) {
                    Some(k) if table && value.is_empty() && header_field(block[k]).is_none() => {
                        read += 1;
                        (block[k].trim(), k)
                    }
                    _ => (value, j),
                };
                let slot = match field {
                    Field::From => Some(&mut h.from),
                    Field::Date => Some(&mut h.at),
                    Field::Subject => Some(&mut h.subject),
                    Field::To | Field::Cc | Field::Other => None,
                };
                if let Some(slot) = slot.filter(|s| s.is_none()) {
                    *slot = non_empty(value);
                }
                h.body_start = end + 1;
                j = end + 1;
            }
            // 見出しの途中の、項目名の無い行は折り返し（長い To）とみなす。見出しより前なら本文。
            None if seen => {
                h.body_start = j + 1;
                j += 1;
            }
            None => break,
        }
    }
    h
}

/// 見出しのかたまりとみなす範囲の行数（空行は数えない）。
const CLUSTER_LINES: usize = 12;

/// 見出しのかたまりに要る、見出しの種類の数（差出人・日付・宛先・件名・Cc のうち）。
const CLUSTER_KINDS: usize = 3;

/// 項目の値。同じ行に無ければ、空行を挟んだ次の行（それも項目なら値は無い）。
fn field_value<'a>(block: &[&'a str], j: usize, inline: &'a str) -> &'a str {
    if !inline.is_empty() {
        return inline;
    }
    next_non_blank(block, j)
        .filter(|&k| header_field(block[k]).is_none())
        .map_or("", |k| block[k].trim())
}

/// 既知の書式で読めなかったときの予備の判定: `j` の行から、見出しがかたまって始まるか。
///
/// 空行を除いた [`CLUSTER_LINES`] 行の中に、差出人・日付・宛先・件名・Cc の見出しが
/// [`CLUSTER_KINDS`] 種類以上あり、そのうちに**差出人と日付**があり、値に**アドレス**（`@`）と
/// **日時として読めるもの**が 1 つ以上ある。値は同じ行でも、空行を挟んだ次の行でもよい。
/// To と Subject は必須にしない（Bcc で受けたメール・"On … wrote:" の形には無い）。
/// 本文の申込書や見積の「件名:」「日付:」だけの段落は、差出人・アドレスが無いので区切らない。
/// かたまりの途中の行では始まらない（直前の空でない行が見出しなら false）。
pub fn starts_header_cluster(block: &[&str], j: usize) -> bool {
    let is_field = is_header_line;
    if !is_field(block[j]) {
        return false;
    }
    if block[..j]
        .iter()
        .rev()
        .find(|l| !l.trim().is_empty())
        .is_some_and(|l| is_field(l))
    {
        return false;
    }
    let mut kinds: Vec<Field> = Vec::new();
    let (mut addr, mut date) = (false, false);
    let window = (j..block.len())
        .filter(|&k| !block[k].trim().is_empty())
        .take(CLUSTER_LINES);
    for k in window {
        let Some((field, inline)) = header_field(block[k]).filter(|_| is_field(block[k])) else {
            continue;
        };
        let value = field_value(block, k, inline);
        if field != Field::Other && !kinds.contains(&field) {
            kinds.push(field);
        }
        addr |= value.contains('@');
        date |= field == Field::Date && super::when::parse_quoted_time(value).is_some();
    }
    kinds.len() >= CLUSTER_KINDS && kinds.contains(&Field::From) && addr && date
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

    /// Thunderbird が HTML の表を文字にした見出し（項目名と値が別の行・間に空行。名前は作り物）。
    #[test]
    fn reads_thunderbird_table_header() {
        let block = [
            "-------- Forwarded Message -------- ",
            "",
            "",
            "Subject: ",
            "",
            "RE: Fwd: 図面 その3",
            "",
            "",
            "Date: ",
            "",
            "Tue, 20 Oct 2026 11:21:08 +0900",
            "",
            "",
            "From: ",
            "",
            "木材商事 山田花子  <mailto:hanako@mokuzai.example.jp> <hanako@mokuzai.example.jp>",
            "",
            "",
            "To: ",
            "",
            "'佐藤 一郎'  <mailto:ichiro@example.jp> <ichiro@example.jp>",
            "",
            "",
            "",
            "佐藤様",
            "図面はこちらから取ってください。",
        ];
        let h = parse_header(&block, false);
        assert_eq!(
            h.from.as_deref(),
            Some(
                "木材商事 山田花子  <mailto:hanako@mokuzai.example.jp> <hanako@mokuzai.example.jp>"
            )
        );
        assert_eq!(h.at.as_deref(), Some("Tue, 20 Oct 2026 11:21:08 +0900"));
        assert_eq!(h.subject.as_deref(), Some("RE: Fwd: 図面 その3"));
        assert_eq!(
            block[h.body_start..].iter().find(|l| !l.is_empty()),
            Some(&"佐藤様")
        );
    }

    /// 日本語版の Thunderbird の同じ形（"-------- 転送メッセージ --------" / 件名・日付・差出人・宛先）。
    #[test]
    fn reads_japanese_thunderbird_table_header() {
        let block = [
            "-------- 転送メッセージ --------",
            "",
            "件名:",
            "",
            "見積もりの件",
            "",
            "日付:",
            "",
            "Fri, 9 Oct 2026 15:29:01 +0900",
            "",
            "差出人:",
            "",
            "鈴木 次郎 <jiro@example.com>",
            "",
            "宛先:",
            "",
            "佐藤 一郎 <ichiro@example.jp>",
            "",
            "佐藤様",
        ];
        let h = parse_header(&block, false);
        assert_eq!(h.from.as_deref(), Some("鈴木 次郎 <jiro@example.com>"));
        assert_eq!(h.at.as_deref(), Some("Fri, 9 Oct 2026 15:29:01 +0900"));
        assert_eq!(h.subject.as_deref(), Some("見積もりの件"));
        assert_eq!(block[h.body_start], "佐藤様");
    }

    /// 普通の見出しで件名が空でも、空行の後の本文の 1 行目を件名として読まない。
    #[test]
    fn empty_subject_in_ordinary_header_stays_empty() {
        let block = [
            "From: 山田 花子 <hanako@example.com>",
            "Sent: Friday, October 2, 2026 5:55 PM",
            "Subject: ",
            "",
            "ご確認ください。",
        ];
        let h = parse_header(&block, false);
        assert_eq!(h.subject, None);
        assert_eq!(block[h.body_start], "ご確認ください。");
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
