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
    /// 差出人の表示名（ヘッダ From の名前部。無ければ None）。
    pub from_name: Option<String>,
    pub to_addresses: Option<String>,
    /// 宛先（先頭）の表示名（ヘッダ To の名前部。無ければ None）。
    pub to_name: Option<String>,
    /// Reply-To（差出人が指定する返信先。"名前 <addr>, ..." の表示用文字列。無ければ None）。
    /// 設定されていれば返信の宛先を From ではなくこちらにする（ML・no-reply＋実返信先 等）。
    pub reply_to: Option<String>,
    /// Cc の全アドレス（"名前 <addr>, ..." の表示用文字列。無ければ None）。
    pub cc_addresses: Option<String>,
    pub date: Option<String>,
    /// 並び替え用の epoch 秒（date を UTC 換算）。インデックスで新しい順に引くのに使う。
    pub date_ts: Option<i64>,
    pub body_plain: Option<String>,
    pub clean_body: Option<String>,
    pub body_html: Option<String>,
    /// Authentication-Results の生テキスト（SPF/DKIM/DMARC 判定。docs/MAIL_SECURITY.md / SPAM.md §7.7）。
    pub auth_result: Option<String>,
    /// List-Id の生テキスト（メルマガ/ML 判定。docs/SPAM.md §7.7）。
    pub list_id: Option<String>,
    /// In-Reply-To（返信元 Message-ID。山括弧なし。スレッド束ね。docs/THREADING.md §2）。
    pub in_reply_to: Option<String>,
    /// References（祖先 Message-ID の連鎖。空白区切り・山括弧なし・古い順）。
    pub references_ids: Option<String>,
    /// Thread-Index（Outlook/Exchange の会話ツリー。References 欠落時の補完）。
    pub thread_index: Option<String>,
    /// ヘッダ部の生テキスト（後からの解析やり直し・素性抽出用）。
    pub raw_headers: Option<String>,
    pub has_attachments: bool,
    pub attachments: Vec<ParsedAttachment>,
    /// 引用ブロック（属性行から from+時刻、本文から fingerprint）。docs/THREADING.md §7。
    pub quotes: Vec<crate::services::quotes::QuoteBlock>,
    pub preview: String,
}

/// Message-ID 系ヘッダの生値から、山括弧内の ID 群（無ければ空白区切りトークン）を取り出す。
fn extract_ids(raw: Option<String>) -> Vec<String> {
    let Some(s) = raw else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    let mut rest = s.as_str();
    while let Some(a) = rest.find('<') {
        if let Some(rel) = rest[a + 1..].find('>') {
            let id = rest[a + 1..a + 1 + rel].trim();
            if !id.is_empty() {
                ids.push(id.to_string());
            }
            rest = &rest[a + 1 + rel + 1..];
        } else {
            break;
        }
    }
    if ids.is_empty() {
        for tok in s.split_whitespace() {
            let t = tok.trim_matches(|c| c == '<' || c == '>').trim();
            if !t.is_empty() {
                ids.push(t.to_string());
            }
        }
    }
    ids
}

/// 生メッセージのヘッダ部（最初の空行まで）を取り出す。
fn header_block(raw: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(raw);
    let idx = s.find("\r\n\r\n").or_else(|| s.find("\n\n"))?;
    Some(s[..idx].to_string())
}

/// アドレスヘッダ（To/Cc）を "名前 <addr>, ..." の表示用文字列へ整形する（無ければ None）。
/// 複数宛先も全件を join する（受信ヘッダに Cc を表示するため）。
fn format_address_list(addr: Option<&mail_parser::Address>) -> Option<String> {
    let parts: Vec<String> = addr?
        .iter()
        .filter_map(|x| {
            let email = x
                .address
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())?;
            let name = x.name.as_deref().map(str::trim).filter(|s| !s.is_empty());
            Some(match name {
                Some(n) => format!("{n} <{email}>"),
                None => email.to_string(),
            })
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

/// 指定ヘッダの生テキストを取り出す（無い/空なら None）。
/// List-Id のように構造化されて as_text() が効かないヘッダも拾えるよう、生値（header_raw）を使う。
fn header_text(msg: &mail_parser::Message, name: &str) -> Option<String> {
    msg.header_raw(name)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
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

/// ブロック要素（この境界で改行を入れる）。フロントの HtmlText の BLOCK と同方針。
const BLOCK_TAGS: &[&str] = &[
    "p", "div", "br", "tr", "li", "ul", "ol", "table", "thead", "tbody", "h1", "h2", "h3", "h4",
    "h5", "h6", "blockquote", "hr", "section", "article", "header", "footer", "pre", "dd", "dt",
    "figure", "address", "form",
];

/// HTML 実体参照を復元する（よく出るものと数値参照 &#nn; / &#xHH;）。
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        if rest.starts_with('&') {
            if let Some(semi) = rest.find(';') {
                if semi <= 12 {
                    let ent = &rest[1..semi];
                    let rep = match ent {
                        "nbsp" => Some(' '),
                        "amp" => Some('&'),
                        "lt" => Some('<'),
                        "gt" => Some('>'),
                        "quot" => Some('"'),
                        "apos" => Some('\''),
                        _ => ent.strip_prefix('#').and_then(|num| {
                            let cp = match num.strip_prefix(['x', 'X']) {
                                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                                None => num.parse::<u32>().ok(),
                            };
                            cp.and_then(char::from_u32)
                        }),
                    };
                    if let Some(c) = rep {
                        out.push(c);
                        i += semi + 1;
                        continue;
                    }
                }
            }
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// 各行の ASCII 空白（半角スペース/タブ/CR）の連続を 1 つに畳み、行端を整え、空行を除く。
/// 全角空白 U+3000 等の Unicode 空白は日本語本文で意味を持つため畳まず保持する。
fn normalize_ws(s: &str) -> String {
    fn is_ascii_sp(c: char) -> bool {
        c == ' ' || c == '\t' || c == '\r'
    }
    s.lines()
        .map(|l| {
            let mut r = String::with_capacity(l.len());
            let mut prev_sp = false;
            for c in l.chars() {
                if is_ascii_sp(c) {
                    if !prev_sp {
                        r.push(' ');
                    }
                    prev_sp = true;
                } else {
                    r.push(c);
                    prev_sp = false;
                }
            }
            r.trim_matches(is_ascii_sp).to_string()
        })
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// HTML をブロック要素で改行しながらプレーンテキスト化する（mail_parser の平坦化対策の自前変換）。
/// text/plain の実パートが無い HTML 専用メール（multipart/related 等）向け。
/// - ブロック要素／`<br>` の境界で改行（連続改行は 1 つに畳む）
/// - `<script>`/`<style>`/`<head>` 等は中身ごと破棄、コメントも除去
/// - 実体参照を復元し、行内の余分な空白は畳む
pub fn html_to_text(html: &str) -> String {
    fn push_break(out: &mut String) {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
    }
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut i = 0usize;
    while i < html.len() {
        let rest = &html[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").map(|p| p + 3).unwrap_or(rest.len());
            continue;
        }
        if rest.starts_with('<') {
            let Some(gt) = rest.find('>') else { break };
            let inner = &rest[1..gt];
            let is_close = inner.starts_with('/');
            let nm = inner.trim_start_matches('/');
            let end = nm
                .find(|c: char| !c.is_ascii_alphanumeric())
                .unwrap_or(nm.len());
            let tag = nm[..end].to_ascii_lowercase();
            if !is_close && matches!(tag.as_str(), "script" | "style" | "head" | "title" | "noscript")
            {
                let close = format!("</{tag}>");
                i += match lower[i + gt..].find(&close) {
                    Some(pos) => gt + pos + close.len(),
                    None => gt + 1,
                };
                continue;
            }
            if tag == "br" || BLOCK_TAGS.contains(&tag.as_str()) {
                push_break(&mut out);
            }
            i += gt + 1;
            continue;
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    normalize_ws(&decode_entities(&out))
}

/// メッセージが「本物の text/plain パート」を持つか。持たない（HTML 専用）なら false。
/// mail_parser は text/plain が無いと HTML を本文テキストとして扱う（＝平坦化）ため、その判別に使う。
fn has_real_plain_part(msg: &mail_parser::Message) -> bool {
    msg.text_body
        .first()
        .and_then(|&idx| msg.parts.get(idx))
        .map(|p| matches!(p.body, mail_parser::PartType::Text(_)))
        .unwrap_or(false)
}

/// mail_parser の HTML→text 生成でよくある「本文が 1 行に潰れた」プレーンテキストか。
/// text/plain の実パートが無い HTML 専用メールで起きる。長さの割に改行が極端に少ないと真。
/// （再構築の後付け判定に使う。取り込み時は has_real_plain_part で直接判別する。）
pub fn is_flattened_plaintext(t: &str) -> bool {
    let chars = t.chars().count();
    chars > 400 && t.matches('\n').count().saturating_mul(200) < chars
}

/// 保存済みヘッダ生テキスト（raw_headers）から Reply-To を取り出す（"名前 <addr>, ..."）。
/// 既存メールへの後付け（再構築）用。本文の無いヘッダ塊でもパースできるよう空行を補って解析する。
pub fn reply_to_from_headers(raw_headers: &str) -> Option<String> {
    let mut buf = raw_headers.trim_end().to_string();
    buf.push_str("\r\n\r\n");
    let msg = MessageParser::default().parse(buf.as_bytes())?;
    format_address_list(msg.reply_to())
}

/// 生の RFC822 メッセージを解析する。
pub fn parse_message(raw: &[u8]) -> Option<ParsedEmail> {
    let msg = MessageParser::default().parse(raw)?;

    let subject = msg.subject().map(|s| s.to_string());
    let from = msg.from().and_then(|a| a.first());
    let from_address = from
        .and_then(|addr| addr.address.as_deref())
        .map(|s| s.to_string());
    // ヘッダの表示名（From: "名前" <addr>）。空や引用符だけは None にする。
    let from_name = from
        .and_then(|addr| addr.name.as_deref())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let to = msg.to().and_then(|a| a.first());
    let to_addresses = to
        .and_then(|addr| addr.address.as_deref())
        .map(|s| s.to_string());
    let to_name = to
        .and_then(|addr| addr.name.as_deref())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // Cc は全件を表示用文字列に（受信メールのヘッダに Cc を出すため）。
    let cc_addresses = format_address_list(msg.cc());
    // Reply-To（返信先指定）。From と別なら返信の宛先をこちらにする。全件を表示用文字列に。
    let reply_to = format_address_list(msg.reply_to());
    let message_id = msg.message_id().map(|s| s.to_string());
    let date = msg.date().map(|d| d.to_rfc3339());
    // 並び替え用の epoch 秒（UTC）。無効な日付は None。
    let date_ts = msg.date().map(|d| d.to_timestamp());
    let body_html = msg.body_html(0).map(|c| c.to_string());
    // text/plain の実パートが無い HTML 専用メール（multipart/related 等）は、mail_parser の
    // HTML→text がブロック改行を落として本文が 1 行に潰れる。実 plain パートが無く HTML があるときは
    // 自前のブロック対応変換で改行を復元する。これで表示だけでなく、返信で引用する本文も崩れない。
    let body_plain = if has_real_plain_part(&msg) {
        msg.body_text(0).map(|c| c.to_string())
    } else if let Some(h) = body_html.as_deref() {
        Some(html_to_text(h))
    } else {
        msg.body_text(0).map(|c| c.to_string())
    };
    // ヘッダ素性（§7.7）: 認証結果・メール種別。トークン化と認証バッジで共有する。
    let auth_result = header_text(&msg, "Authentication-Results");
    let list_id = header_text(&msg, "List-Id");
    // スレッド束ね用ヘッダ（docs/THREADING.md §2）。ID は山括弧を外して保存する。
    let in_reply_to = extract_ids(header_text(&msg, "In-Reply-To"))
        .into_iter()
        .next();
    let references_vec = extract_ids(header_text(&msg, "References"));
    let references_ids = if references_vec.is_empty() {
        None
    } else {
        Some(references_vec.join(" "))
    };
    let thread_index = header_text(&msg, "Thread-Index");
    let raw_headers = header_block(raw);
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

    // 引用/署名を分離して新規本文（clean_body）と引用ブロックを得る（docs/THREADING.md §7）。
    let split = body_plain
        .as_deref()
        .map(crate::services::quotes::split_reply);
    let clean_body = split.as_ref().map(|s| s.clean.clone());
    let quotes = split.map(|s| s.quotes).unwrap_or_default();
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
        from_name,
        to_addresses,
        to_name,
        reply_to,
        cc_addresses,
        date,
        date_ts,
        body_plain,
        clean_body,
        body_html,
        auth_result,
        list_id,
        in_reply_to,
        references_ids,
        thread_index,
        raw_headers,
        has_attachments,
        attachments,
        quotes,
        preview,
    })
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

    #[test]
    fn parses_header_only_for_metadata_index() {
        // BODY.PEEK[HEADER] 相当（ヘッダのみ・本文なし）。全件メタ索引はこれを parse_message へ
        // 通す（docs/SYNC.md §3.6）。canonical_key 等がフル取得と一致し、本文は空になること。
        let raw = b"From: Taro <taro@example.com>\r\n\
To: Hanako <hanako@example.com>\r\n\
Subject: Hello\r\n\
Message-ID: <abc123@example.com>\r\n\
In-Reply-To: <parent@example.com>\r\n\
References: <root@example.com> <parent@example.com>\r\n\
Date: Mon, 30 Jun 2025 10:00:00 +0900\r\n\
\r\n";
        let p = parse_message(raw).expect("header-only should parse");
        // 重複排除キーは Message-ID 由来＝フル取得と一致（後で本文取得しても同じ行に統合）。
        assert_eq!(p.canonical_key, "abc123@example.com");
        assert_eq!(p.message_id.as_deref(), Some("abc123@example.com"));
        assert_eq!(p.subject.as_deref(), Some("Hello"));
        assert_eq!(p.from_address.as_deref(), Some("taro@example.com"));
        assert_eq!(p.in_reply_to.as_deref(), Some("parent@example.com"));
        let refs = p.references_ids.as_deref().unwrap_or("");
        assert!(refs.contains("root@example.com") && refs.contains("parent@example.com"));
        // 本文3列は空 → insert_email 側で body_state='absent' になる。
        assert!(p.clean_body.as_deref().unwrap_or("").trim().is_empty());
        assert!(p.body_plain.as_deref().unwrap_or("").is_empty());
    }

    #[test]
    fn html_to_text_breaks_on_block_elements() {
        // Apple Mail 系の <div> 1 行構造を改行付きで復元する。
        let html = "<div>末松　さま</div><div>お世話になっております。伊佐です。</div>\
<div>江島邸につきまして、ご連絡いたしました。</div>";
        assert_eq!(
            html_to_text(html),
            "末松　さま\nお世話になっております。伊佐です。\n江島邸につきまして、ご連絡いたしました。"
        );
    }

    #[test]
    fn html_to_text_handles_br_entities_and_drops_style() {
        let html = "<style>.x{color:red}</style>A&nbsp;B<br>C&amp;D<!-- comment -->";
        assert_eq!(html_to_text(html), "A B\nC&D");
    }

    #[test]
    fn html_to_text_preserves_fullwidth_space() {
        // 全角空白 U+3000 は日本語本文で意味を持つので畳まない。
        assert_eq!(html_to_text("<div>末松　さま</div>"), "末松　さま");
    }

    #[test]
    fn html_only_message_recovers_line_breaks() {
        // text/plain が無く HTML のみ（multipart/related）のメールでも body_plain が 1 行に潰れない。
        let raw = "From: A <a@example.com>\r\n\
To: B <b@example.com>\r\n\
Subject: x\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<div>1行目</div><div>2行目</div><div>3行目</div>\r\n";
        let p = parse_message(raw.as_bytes()).expect("should parse");
        let body = p.body_plain.expect("has body");
        assert!(body.contains("1行目\n2行目"), "body was: {body:?}");
    }

    #[test]
    fn is_flattened_detects_collapsed_and_spares_normal() {
        let collapsed = "あ".repeat(500); // 500 文字・改行なし
        assert!(is_flattened_plaintext(&collapsed));
        let normal = "一行目\n".repeat(60); // 60 行
        assert!(!is_flattened_plaintext(&normal));
        assert!(!is_flattened_plaintext("短い本文")); // 400 字以下は対象外
    }

    #[test]
    fn extracts_reply_to() {
        let raw = b"From: No Reply <no-reply@service.example>\r\n\
To: Me <me@example.com>\r\n\
Reply-To: Support <support@service.example>\r\n\
Subject: Ticket\r\n\
\r\n\
body\r\n";
        let p = parse_message(raw).expect("should parse");
        assert_eq!(p.reply_to.as_deref(), Some("Support <support@service.example>"));
        // ヘッダ塊だけからの後付け抽出も同じ結果になる。
        let hdr = "From: No Reply <no-reply@service.example>\r\nReply-To: Support <support@service.example>\r\n";
        assert_eq!(
            reply_to_from_headers(hdr).as_deref(),
            Some("Support <support@service.example>")
        );
    }

    #[test]
    fn no_reply_to_is_none() {
        let raw = b"From: A <a@example.com>\r\nTo: B <b@example.com>\r\nSubject: x\r\n\r\nbody\r\n";
        assert_eq!(parse_message(raw).unwrap().reply_to, None);
    }

    #[test]
    fn extracts_auth_result_and_list_id() {
        let raw = b"From: News <news@example.com>\r\n\
Subject: Bulletin\r\n\
Authentication-Results: mx.example.com; spf=fail smtp.mailfrom=example.com; dkim=pass\r\n\
List-Id: Example News <news.example.com>\r\n\
\r\n\
body\r\n";
        let p = parse_message(raw).expect("should parse");
        assert!(p.auth_result.as_deref().unwrap().contains("spf=fail"));
        assert!(p.list_id.as_deref().unwrap().contains("news.example.com"));
    }
}
