//! text/plain の本文に、HTML の文字参照（"&nbsp;" / "&amp;" ほか）がそのまま書かれたメールを直す。
//!
//! 送り手のメールソフトが HTML から text/plain を作るときに、文字参照を解かずに書くことがある
//! （"&nbsp;&nbsp;2026/10/14&nbsp;12:50:57&nbsp;…さんが書きました:"）。そのままでは本文が読みにくく、
//! 引用の見出しも読めない。本文で「&」を普通に使う文（"A&B社" / "R&D"）は壊さないよう、
//! 文字参照が書かれていると判断できるときだけ解く。

/// 判定に数える文字参照（"&nbsp;" 以外）の数の下限。"&amp;" 1 つだけ（URL の "?a=1&amp;b=2" 等）では解かない。
const MIN_ENTITIES: usize = 3;

/// "&nbsp;" 以外で数える文字参照（"&amp;" / "&lt;" / "&gt;" / "&#数字;" / "&#x16進;"）か。
/// `rest` は "&" から始まる文字列。当たれば文字参照の長さ（"&" と ";" を含む）を返す。
fn counted_entity_len(rest: &str) -> Option<usize> {
    if let Some(named) = ["&amp;", "&lt;", "&gt;"]
        .iter()
        .find(|e| rest.starts_with(**e))
    {
        return Some(named.len());
    }
    let num = rest.strip_prefix("&#")?;
    let digits = num.strip_prefix(['x', 'X']);
    let is_digit = |c: char| match digits {
        Some(_) => c.is_ascii_hexdigit(),
        None => c.is_ascii_digit(),
    };
    let digits = digits.unwrap_or(num);
    let len = digits.find(|c: char| !is_digit(c)).unwrap_or(digits.len());
    (len > 0 && digits[len..].starts_with(';')).then(|| rest.len() - digits.len() + len + 1)
}

/// 本文に文字参照がそのまま書かれているとみなすか。
/// "&nbsp;" が 1 つでもあるか、"&amp;" / "&lt;" / "&gt;" / 数値の文字参照が [`MIN_ENTITIES`] 個以上あるとき。
pub fn has_literal_entities(text: &str) -> bool {
    if text.contains("&nbsp;") {
        return true;
    }
    text.match_indices('&')
        .filter(|&(i, _)| counted_entity_len(&text[i..]).is_some())
        .take(MIN_ENTITIES)
        .count()
        >= MIN_ENTITIES
}

/// text/plain の本文の文字参照を解く（[`has_literal_entities`] のときだけ）。
/// 解く対象は既知の名前（nbsp / amp / lt / gt / quot / apos）と数値の文字参照だけで、
/// "&nbsp;" は普通の空白にする。空白・改行は畳まない（text/plain の行の形を保つ）。
/// 解く必要が無い・解いても変わらないときは None。
pub fn decode_literal_entities(text: &str) -> Option<String> {
    if !has_literal_entities(text) {
        return None;
    }
    let decoded = super::decode_entities(text);
    (decoded != text).then_some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実例の形（名前・アドレスは作り物）。"&nbsp;" が並んで、引用の見出しが読めなかった。
    #[test]
    fn decodes_nbsp_in_plain_text() {
        let body = "了解です。\n\n&nbsp;&nbsp;2026/10/14&nbsp;12:50:57&nbsp;前田 三郎 &lt;maeda@example.invalid&gt; さんが書きました:\n> 図面を送ります。";
        assert_eq!(
            decode_literal_entities(body).as_deref(),
            Some("了解です。\n\n  2026/10/14 12:50:57 前田 三郎 <maeda@example.invalid> さんが書きました:\n> 図面を送ります。")
        );
    }

    #[test]
    fn decodes_when_three_or_more_other_entities() {
        assert_eq!(
            decode_literal_entities("A &amp; B &lt;b&gt;").as_deref(),
            Some("A & B <b>")
        );
        assert_eq!(
            decode_literal_entities("&#12354;&#x3044;&#X3046;").as_deref(),
            Some("あいう")
        );
    }

    /// 閾値: "&nbsp;" 以外の文字参照が 2 つまでなら解かない。
    #[test]
    fn spares_two_or_fewer_entities() {
        assert_eq!(
            decode_literal_entities("https://example.invalid/?a=1&amp;b=2"),
            None
        );
        assert_eq!(decode_literal_entities("x &lt; y かつ y &gt; z"), None);
        assert!(!has_literal_entities("A &amp; B &lt;b"));
        assert!(has_literal_entities("A &amp; B &lt;b&gt;"));
    }

    /// 本文で「&」を普通に使う文は、文字参照と数えず、壊さない。
    #[test]
    fn spares_plain_ampersands() {
        for body in [
            "A&B社の件です。",
            "R&D 部門へ。",
            "Q&A をまとめました。",
            "AT&T と P&G と M&A。",
            "&で始まる行 & 途中の & と 末尾の &",
            "&amp 書きかけ（セミコロン無し）&lt &gt",
            "&#; &#x; &#xyz; &#12a;",
        ] {
            assert!(!has_literal_entities(body), "{body}");
            assert_eq!(decode_literal_entities(body), None, "{body}");
        }
    }

    /// 解く対象は既知の名前だけ。知らない名前（"&copy;" / "&hellip;"）や "&nbsp" の書きかけは残す。
    #[test]
    fn decodes_only_known_named_entities() {
        assert_eq!(
            decode_literal_entities(
                "&nbsp;&copy; 2026&hellip;&nbsp;R&D &quot;x&quot; &apos;y&apos; &nbsp"
            )
            .as_deref(),
            Some(" &copy; 2026&hellip; R&D \"x\" 'y' &nbsp")
        );
    }

    /// 改行・空白の形はそのまま（HTML から作るときと違い、畳まない）。
    #[test]
    fn keeps_lines_and_spaces() {
        assert_eq!(
            decode_literal_entities("1行目&nbsp;&nbsp;\r\n\r\n  3行目　全角").as_deref(),
            Some("1行目  \r\n\r\n  3行目　全角")
        );
    }

    /// 取り込み: text/plain のパートに文字参照がそのまま書かれていれば、解いてから body_plain にする。
    /// 「&」を普通に使う本文はそのまま。
    #[test]
    fn parse_message_decodes_literal_entities_in_plain_part() {
        let mail = |body: &str| {
            format!(
                "From: Maeda <maeda@example.invalid>\r\nTo: suzuki@example.invalid\r\nSubject: x\r\n\
                 Content-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n"
            )
        };
        let raw = mail("了解です。\r\n\r\n&nbsp;2026/10/14&nbsp;12:50&nbsp;前田 三郎 &lt;maeda@example.invalid&gt; さんが書きました:\r\n> 図面を送ります。");
        let p = super::super::parse_message(raw.as_bytes()).expect("parses");
        let body = p.body_plain.expect("has body");
        assert!(
            body.contains(" 2026/10/14 12:50 前田 三郎 <maeda@example.invalid> さんが書きました:"),
            "{body:?}"
        );
        assert_eq!(p.clean_body.as_deref(), Some("了解です。"));

        let raw = mail("A&B社と R&D の件です。&amp; は 1 つだけ。");
        let p = super::super::parse_message(raw.as_bytes()).expect("parses");
        assert_eq!(
            p.body_plain.as_deref().map(str::trim_end),
            Some("A&B社と R&D の件です。&amp; は 1 つだけ。")
        );
    }
}
