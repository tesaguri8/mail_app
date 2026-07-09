//! IMAP `BODYSTRUCTURE` を辿って、各パートの「本来の添付」「本文テキスト」と、その取得キー
//! （IMAP section パス）を取り出す（docs/SYNC.md）。
//!
//! 本体（本文・添付の中身）を受信せず、構造だけから添付メタ・取得キー(section)を得るためのもの。
//! これにより同期で添付本体を落とさずに済み、添付は開いたときに `BODY[section]` で該当パートだけ
//! 取得できる。`mail_parser` の `.attachments()` がネスト構造でコンテナを返す問題も、ここを正本に
//! することで根治する（ネストした実添付も section 付きで取れる）。

use imap_proto::types::{BodyParams, BodyStructure};

/// BODYSTRUCTURE 上の 1 つの葉パート。
#[derive(Debug, Clone)]
pub struct StructPart {
    /// IMAP section パス（"1" / "2" / "1.1" …）。`BODY[section]` 取得に使う。
    pub section: String,
    /// "type/subtype"（小文字。例: "application/pdf"）。
    pub content_type: String,
    /// 表示名（Content-Disposition filename か Content-Type name。無ければ None）。
    pub filename: Option<String>,
    /// Content-ID（山括弧除去。cid: 参照解決・inline 判定用）。
    pub content_id: Option<String>,
    /// サイズ（octets）。
    pub size: i64,
    /// 本来の添付か（名前 or Content-ID を持つ葉、または message/rfc822）。
    pub is_attachment: bool,
    /// 本文テキスト葉か（text/* で添付でない）。Stage2 の本文取得で使う。
    pub is_body_text: bool,
}

/// BodyParams（`Option<Vec<(key, value)>>`）から key（大小無視）の値を取り出す。
fn param<'a>(params: &BodyParams<'a>, key: &str) -> Option<&'a str> {
    let list = params.as_ref()?;
    for &(k, v) in list.iter() {
        if k.eq_ignore_ascii_case(key) {
            return Some(v);
        }
    }
    None
}

/// BODYSTRUCTURE 全体を辿って、葉パートを文書順（section 昇順）に返す。
pub fn parse(bs: &BodyStructure) -> Vec<StructPart> {
    let mut out = Vec::new();
    walk(bs, "", &mut out);
    out
}

/// 実添付（is_attachment）だけを返す（Stage1 の添付メタ生成用）。
pub fn attachments(bs: &BodyStructure) -> Vec<StructPart> {
    parse(bs).into_iter().filter(|p| p.is_attachment).collect()
}

/// 本文テキスト葉（text/* で添付でない）の section を文書順で返す（Stage2 の本文取得用）。
/// これらの section だけ `BODY[section]` で取れば、添付本体を落とさず本文が得られる。
pub fn body_text_sections(bs: &BodyStructure) -> Vec<String> {
    parse(bs)
        .into_iter()
        .filter(|p| p.is_body_text)
        .map(|p| p.section)
        .collect()
}

fn walk(bs: &BodyStructure, prefix: &str, out: &mut Vec<StructPart>) {
    match bs {
        // コンテナ: 子を 1..N で番号付けして辿る（コンテナ自体は葉ではない）。
        BodyStructure::Multipart { bodies, .. } => {
            for (i, child) in bodies.iter().enumerate() {
                let path = if prefix.is_empty() {
                    (i + 1).to_string()
                } else {
                    format!("{prefix}.{}", i + 1)
                };
                walk(child, &path, out);
            }
        }
        // 中身の葉（application/pdf, image/*, text/plain 等）。
        BodyStructure::Basic { common, other, .. } | BodyStructure::Text { common, other, .. } => {
            // トップが単一パートのときは section "1"。
            let section = if prefix.is_empty() {
                "1".to_string()
            } else {
                prefix.to_string()
            };
            let ct = &common.ty;
            let filename = common
                .disposition
                .as_ref()
                .and_then(|d| param(&d.params, "filename"))
                .or_else(|| param(&ct.params, "name"))
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty());
            let content_id = other
                .id
                .map(|s| s.trim_matches(|c| c == '<' || c == '>').trim().to_string())
                .filter(|s| !s.is_empty());
            let is_text = ct.ty.eq_ignore_ascii_case("text");
            let is_attachment = filename.is_some() || content_id.is_some();
            out.push(StructPart {
                section,
                content_type: format!("{}/{}", ct.ty, ct.subtype).to_lowercase(),
                filename,
                content_id,
                size: other.octets as i64,
                is_attachment,
                is_body_text: is_text && !is_attachment,
            });
        }
        // message/rfc822（添付された転送メール等）は 1 つの添付として扱い、中は展開しない
        // （クライアント一般の挙動。開けば中の構造を辿れる）。
        BodyStructure::Message { common, other, .. } => {
            let section = if prefix.is_empty() {
                "1".to_string()
            } else {
                prefix.to_string()
            };
            let filename = common
                .disposition
                .as_ref()
                .and_then(|d| param(&d.params, "filename"))
                .or_else(|| param(&common.ty.params, "name"))
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty());
            let content_id = other
                .id
                .map(|s| s.trim_matches(|c| c == '<' || c == '>').trim().to_string())
                .filter(|s| !s.is_empty());
            out.push(StructPart {
                section,
                content_type: "message/rfc822".to_string(),
                filename,
                content_id,
                size: other.octets as i64,
                is_attachment: true,
                is_body_text: false,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use imap_proto::types::{
        BodyContentCommon, BodyContentSinglePart, ContentDisposition, ContentEncoding, ContentType,
        Envelope,
    };

    fn common<'a>(
        ty: &'a str,
        subtype: &'a str,
        name: Option<&'a str>,
        disp_filename: Option<&'a str>,
    ) -> BodyContentCommon<'a> {
        BodyContentCommon {
            ty: ContentType {
                ty,
                subtype,
                params: name.map(|n| vec![("name", n)]),
            },
            disposition: disp_filename.map(|f| ContentDisposition {
                ty: "attachment",
                params: Some(vec![("filename", f)]),
            }),
            language: None,
            location: None,
        }
    }

    fn single<'a>(id: Option<&'a str>, octets: u32) -> BodyContentSinglePart<'a> {
        BodyContentSinglePart {
            id,
            md5: None,
            description: None,
            transfer_encoding: ContentEncoding::Base64,
            octets,
        }
    }

    fn text_part<'a>() -> BodyStructure<'a> {
        BodyStructure::Text {
            common: common("text", "plain", None, None),
            other: single(None, 100),
            lines: 5,
            extension: None,
        }
    }

    fn html_part<'a>() -> BodyStructure<'a> {
        BodyStructure::Text {
            common: common("text", "html", None, None),
            other: single(None, 200),
            lines: 8,
            extension: None,
        }
    }

    fn pdf_part<'a>(name: &'a str) -> BodyStructure<'a> {
        BodyStructure::Basic {
            common: common("application", "pdf", None, Some(name)),
            other: single(None, 90_000),
            extension: None,
        }
    }

    fn inline_image<'a>(cid: &'a str) -> BodyStructure<'a> {
        BodyStructure::Basic {
            common: common("image", "png", None, None),
            other: single(Some(cid), 2_000),
            extension: None,
        }
    }

    fn multipart<'a>(subtype: &'a str, bodies: Vec<BodyStructure<'a>>) -> BodyStructure<'a> {
        BodyStructure::Multipart {
            common: BodyContentCommon {
                ty: ContentType {
                    ty: "multipart",
                    subtype,
                    params: None,
                },
                disposition: None,
                language: None,
                location: None,
            },
            bodies,
            extension: None,
        }
    }

    fn empty_env<'a>() -> Envelope<'a> {
        Envelope {
            date: None,
            subject: None,
            from: None,
            sender: None,
            reply_to: None,
            to: None,
            cc: None,
            bcc: None,
            in_reply_to: None,
            message_id: None,
        }
    }

    #[test]
    fn single_text_is_body_no_attachment() {
        let bs = text_part();
        let parts = parse(&bs);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].section, "1");
        assert!(parts[0].is_body_text);
        assert!(!parts[0].is_attachment);
        assert!(attachments(&bs).is_empty());
    }

    #[test]
    fn mixed_text_plus_pdf() {
        let bs = multipart("mixed", vec![text_part(), pdf_part("報告書.pdf")]);
        let parts = parse(&bs);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].section, "1");
        assert!(parts[0].is_body_text);
        assert_eq!(parts[1].section, "2");
        assert!(parts[1].is_attachment);

        let atts = attachments(&bs);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].section, "2");
        assert_eq!(atts[0].filename.as_deref(), Some("報告書.pdf"));
        assert_eq!(atts[0].content_type, "application/pdf");
        assert_eq!(atts[0].size, 90_000);
    }

    #[test]
    fn inline_image_is_attachment_via_content_id() {
        let bs = multipart("related", vec![html_part(), inline_image("<img001@host>")]);
        let atts = attachments(&bs);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].section, "2");
        assert_eq!(atts[0].content_id.as_deref(), Some("img001@host"));
        assert!(atts[0].filename.is_none());
        assert_eq!(atts[0].content_type, "image/png");
    }

    #[test]
    fn nested_alternative_plus_pdf_gets_correct_section() {
        // multipart/mixed [ multipart/alternative[plain, html], pdf ]
        let bs = multipart(
            "mixed",
            vec![
                multipart("alternative", vec![text_part(), html_part()]),
                pdf_part("古川邸スケジュール.pdf"),
            ],
        );
        let parts = parse(&bs);
        // 本文2つ（1.1, 1.2）＋ PDF（2）。
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].section, "1.1");
        assert_eq!(parts[1].section, "1.2");
        assert_eq!(parts[2].section, "2");

        let atts = attachments(&bs);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].section, "2");
        assert_eq!(atts[0].filename.as_deref(), Some("古川邸スケジュール.pdf"));
    }

    #[test]
    fn message_rfc822_is_single_attachment_not_descended() {
        let inner = BodyStructure::Message {
            common: common("message", "rfc822", None, Some("転送.eml")),
            other: single(None, 5_000),
            envelope: empty_env(),
            body: Box::new(text_part()),
            lines: 50,
            extension: None,
        };
        let bs = multipart("mixed", vec![text_part(), inner]);
        let parts = parse(&bs);
        // 本文(1) ＋ message(2)。中の text は展開しない。
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].section, "2");
        assert_eq!(parts[1].content_type, "message/rfc822");

        let atts = attachments(&bs);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].section, "2");
        assert_eq!(atts[0].filename.as_deref(), Some("転送.eml"));
    }
}
