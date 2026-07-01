use mail_parser::{MessageParser, MimeHeaders};

/// 添付メタ（本体は同期時に落とさず、ダウンロード時に再取得する）。
pub struct ParsedAttachment {
    /// message 内の attachment 序数（再取得時に attachment(pos) で特定）。
    pub part_index: i64,
    pub filename: String,
    pub content_type: Option<String>,
    pub size: i64,
    /// 'attachment'（本来の添付）| 'inline'（本文埋め込み画像）。
    pub kind: &'static str,
    /// Content-ID（cid: 参照の解決用。前後の山括弧は除去済み）。
    pub content_id: Option<String>,
}

/// 添付パートを「本来の添付」と「本文埋め込み(inline)」に分類する。
/// Content-Disposition が inline、または Content-ID を持つ画像は inline 扱い。
fn classify_part(part: &mail_parser::MessagePart) -> &'static str {
    let disp_inline = part
        .content_disposition()
        .map(|d| d.ctype().eq_ignore_ascii_case("inline"))
        .unwrap_or(false);
    let has_cid = part.content_id().is_some();
    if disp_inline || has_cid {
        "inline"
    } else {
        "attachment"
    }
}

/// MIME 解析結果（内部）。docs/THREADING.md の解析基盤の最小実装。
pub struct ParsedEmail {
    pub message_id: Option<String>,
    pub canonical_key: String,
    pub subject: Option<String>,
    pub from_address: Option<String>,
    pub to_addresses: Option<String>,
    pub date: Option<String>,
    pub body_plain: Option<String>,
    pub clean_body: Option<String>,
    pub body_html: Option<String>,
    pub has_attachments: bool,
    pub attachments: Vec<ParsedAttachment>,
    pub preview: String,
}

/// 添付パートの MIME 型を "type/subtype" 文字列に整形する。
pub fn part_content_type(part: &mail_parser::MessagePart) -> Option<String> {
    part.content_type().map(|ct| match ct.subtype() {
        Some(sub) => format!("{}/{}", ct.ctype(), sub),
        None => ct.ctype().to_string(),
    })
}

/// 添付パートの表示用ファイル名（名前が無ければ序数から合成）。
pub fn part_filename(part: &mail_parser::MessagePart, index: usize) -> String {
    part.attachment_name()
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("attachment-{}", index + 1))
}

/// 生の RFC822 メッセージを解析する。
pub fn parse_message(raw: &[u8]) -> Option<ParsedEmail> {
    let msg = MessageParser::default().parse(raw)?;

    let subject = msg.subject().map(|s| s.to_string());
    let from_address = msg
        .from()
        .and_then(|a| a.first())
        .and_then(|addr| addr.address.as_deref())
        .map(|s| s.to_string());
    let to_addresses = msg
        .to()
        .and_then(|a| a.first())
        .and_then(|addr| addr.address.as_deref())
        .map(|s| s.to_string());
    let message_id = msg.message_id().map(|s| s.to_string());
    let date = msg.date().map(|d| d.to_rfc3339());
    let body_plain = msg.body_text(0).map(|c| c.to_string());
    let body_html = msg.body_html(0).map(|c| c.to_string());
    let attachments: Vec<ParsedAttachment> = msg
        .attachments()
        .enumerate()
        .map(|(i, part)| ParsedAttachment {
            part_index: i as i64,
            filename: part_filename(part, i),
            content_type: part_content_type(part),
            size: part.contents().len() as i64,
            kind: classify_part(part),
            // Content-ID は通常 <...> で囲まれる。cid: 参照と突き合わせるため山括弧を除去。
            content_id: part
                .content_id()
                .map(|c| c.trim_matches(|ch| ch == '<' || ch == '>').to_string()),
        })
        .collect();
    // 📎 は「実ファイルの添付」があるときだけ立てる。
    // 本文埋め込み画像（inline）だけの HTML メールでは立てない。
    let has_attachments = attachments.iter().any(|a| a.kind == "attachment");

    let clean_body = body_plain.as_deref().map(strip_quotes);
    let preview: String = clean_body
        .as_deref()
        .or(body_plain.as_deref())
        .unwrap_or("")
        .chars()
        .take(140)
        .collect();

    // 正準キー: Message-ID があればそれ、無ければ from|date|subject（docs/CROSS_CUTTING.md #1）
    let canonical_key = message_id.clone().unwrap_or_else(|| {
        format!(
            "{}|{}|{}",
            from_address.clone().unwrap_or_default(),
            date.clone().unwrap_or_default(),
            subject.clone().unwrap_or_default()
        )
    });

    Some(ParsedEmail {
        message_id,
        canonical_key,
        subject,
        from_address,
        to_addresses,
        date,
        body_plain,
        clean_body,
        body_html,
        has_attachments,
        attachments,
        preview,
    })
}

/// 素朴な引用除去（行頭 `>` を落とすだけ。本格版は docs/THREADING.md で実装）。
fn strip_quotes(s: &str) -> String {
    s.lines()
        .filter(|l| !l.trim_start().starts_with('>'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_email() {
        let raw = b"From: Taro <taro@example.com>\r\n\
To: Hanako <hanako@example.com>\r\n\
Subject: Hello\r\n\
Message-ID: <abc123@example.com>\r\n\
Date: Mon, 30 Jun 2025 10:00:00 +0900\r\n\
\r\n\
This is the new part.\r\n\
> quoted old line\r\n";
        let p = parse_message(raw).expect("should parse");
        assert_eq!(p.from_address.as_deref(), Some("taro@example.com"));
        assert_eq!(p.subject.as_deref(), Some("Hello"));
        assert_eq!(p.message_id.as_deref(), Some("abc123@example.com"));
        assert!(p.clean_body.as_deref().unwrap().contains("new part"));
        assert!(!p.clean_body.as_deref().unwrap().contains("quoted old line"));
    }
}
