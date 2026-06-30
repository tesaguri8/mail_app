use mail_parser::MessageParser;

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
    pub preview: String,
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
    let has_attachments = msg.attachments().count() > 0;

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
