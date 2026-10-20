use super::*;
use base64::Engine as _;

/// 中に PDF を 1 つ持つ転送メール（.eml の中身）。
fn inner_eml() -> String {
    let pdf = base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.4 mitsumori");
    format!(
        "From: =?UTF-8?B?5YmN55SwIOWkqumDjg==?= <taro@maeda.invalid>\r\n\
Subject: =?UTF-8?B?6KaL56mN5pu4?=\r\n\
Date: Wed, 15 Oct 2026 11:21:00 +0900\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"in\"\r\n\
\r\n\
--in\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
見積書を添付します。\r\n\
--in\r\n\
Content-Type: application/pdf; name=\"mitsumori.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
Content-Disposition: attachment; filename=\"mitsumori.pdf\"\r\n\
\r\n\
{pdf}\r\n\
--in--\r\n"
    )
}

/// 転送メールを message/rfc822 として付けた外側のメール。
fn outer_with_rfc822() -> String {
    format!(
        "From: hanako@example.invalid\r\n\
Subject: Fwd: 見積書\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"out\"\r\n\
\r\n\
--out\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
転送します。\r\n\
--out\r\n\
Content-Type: message/rfc822; name=\"mitsumori.eml\"\r\n\
Content-Disposition: attachment; filename=\"mitsumori.eml\"\r\n\
\r\n\
{}\r\n\
--out--\r\n",
        inner_eml()
    )
}

/// 外側のメールから、同期・取得と同じ手順（実添付の一覧の 0 番目）で .eml の中身を取り出す。
fn eml_bytes_from_outer() -> Vec<u8> {
    let outer = outer_with_rfc822();
    let msg = MessageParser::default().parse(outer.as_bytes()).unwrap();
    let parts = parser::real_attachment_parts(&msg);
    assert_eq!(
        parts.len(),
        1,
        "外側の実添付は .eml の 1 つだけ（中の PDF は外側に出ない）"
    );
    assert_eq!(parser::part_filename(parts[0], 0), "mitsumori.eml");
    parts[0].contents().to_vec()
}

#[test]
fn rfc822_attachment_lists_inner_attachments_with_headers() {
    let c = contents(&eml_bytes_from_outer()).unwrap();
    assert_eq!(c.subject.as_deref(), Some("見積書"));
    assert_eq!(c.from.as_deref(), Some("前田 太郎 <taro@maeda.invalid>"));
    assert_eq!(c.date.as_deref(), Some("2026-10-15T11:21:00+09:00"));
    assert_eq!(c.attachments.len(), 1);
    let a = &c.attachments[0];
    assert_eq!((a.index, a.filename.as_str()), (0, "mitsumori.pdf"));
    assert_eq!(a.content_type.as_deref(), Some("application/pdf"));
    assert_eq!(a.size, b"%PDF-1.4 mitsumori".len() as i32);
}

#[test]
fn extract_to_file_writes_inner_attachment_next_to_eml_and_reuses_it() {
    let dir = tempfile::tempdir().unwrap();
    let eml = dir.path().join("mitsumori.eml");
    std::fs::write(&eml, eml_bytes_from_outer()).unwrap();

    let path = extract_to_file(&eml, 0).unwrap();
    assert_eq!(
        path,
        dir.path().join("inner").join("0").join("mitsumori.pdf")
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"%PDF-1.4 mitsumori");
    assert!(!dir.path().join("inner").join("0.part").exists());

    // 書き出し済みなら .eml を読み直さない（消しても同じパスが返る）。
    std::fs::remove_file(&eml).unwrap();
    assert_eq!(extract_to_file(&eml, 0).unwrap(), path);
}

#[test]
fn missing_index_is_an_error() {
    let raw = eml_bytes_from_outer();
    assert!(matches!(extract(&raw, 1), Err(EmlError::NoPart(1))));
}

#[test]
fn eml_without_attachments_lists_nothing() {
    let raw = b"From: a@example.invalid\r\nSubject: hi\r\n\r\nbody\r\n";
    let c = contents(raw).unwrap();
    assert_eq!(c.subject.as_deref(), Some("hi"));
    assert!(c.attachments.is_empty());
}
