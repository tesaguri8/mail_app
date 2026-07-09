//! 添付本体を落とさずに本文だけを取り出すための「生メッセージ組み直し」（docs/SYNC.md・Stage2）。
//!
//! 同期では `BODY[]`（本文＋添付本体まで全部）を取らず、ヘッダと本文テキストの section だけを取る。
//! ここでは取得したパーツを **mail_parser が解釈できる 1 通の生 RFC822** に組み直す純関数を提供する。
//! 組み直したものを既存の `parser::parse_message` に渡せば、本文・引用・スレッド解析をそのまま流用できる。
//!
//! - 添付なしメール: `ヘッダ + BODY[TEXT]` は元メッセージそのもの（[reassemble_full]）。
//! - 添付ありメール: 本文テキストパートの MIME ヘッダ＋本体を multipart/alternative に組み直す
//!   （[reassemble_multipart_text]）。添付本体は含めない。

/// 末尾の CR/LF を取り除いたスライスを返す。
fn trim_trailing_crlf(b: &[u8]) -> &[u8] {
    let mut end = b.len();
    while end > 0 && (b[end - 1] == b'\r' || b[end - 1] == b'\n') {
        end -= 1;
    }
    &b[..end]
}

/// `ヘッダ + 本文（BODY[TEXT]）` を 1 通に結合する。添付が無いメールは本文＝全パートなので、これで
/// 元メッセージを完全再現できる（合成不要）。ヘッダと本文は必ず空行 1 つで区切る。
pub fn reassemble_full(header: &[u8], text: &[u8]) -> Vec<u8> {
    let head = trim_trailing_crlf(header);
    let mut out = Vec::with_capacity(head.len() + text.len() + 4);
    out.extend_from_slice(head);
    out.extend_from_slice(b"\r\n\r\n");
    out.extend_from_slice(text);
    out
}

/// トップレベルヘッダから Content-* / MIME-Version を取り除く（合成 multipart を被せるため）。
/// 折り返し行（先頭が空白）も対象フィールドに属していれば一緒に落とす。末尾に CRLF を 1 つ残す。
fn strip_content_headers(header: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(header.len());
    let mut skipping = false;
    let mut i = 0;
    let n = header.len();
    while i < n {
        let start = i;
        while i < n && header[i] != b'\n' {
            i += 1;
        }
        if i < n {
            i += 1; // 改行を含める
        }
        let line = &header[start..i];
        if trim_trailing_crlf(line).is_empty() {
            break; // ヘッダ／本文境界の空行で終了
        }
        let is_continuation = line[0] == b' ' || line[0] == b'\t';
        if is_continuation {
            if !skipping {
                out.extend_from_slice(line);
            }
            continue;
        }
        let colon = line.iter().position(|&c| c == b':').unwrap_or(line.len());
        let name = line[..colon].trim_ascii();
        let drop_field = name.eq_ignore_ascii_case(b"Content-Type")
            || name.eq_ignore_ascii_case(b"Content-Transfer-Encoding")
            || name.eq_ignore_ascii_case(b"Content-Disposition")
            || name.eq_ignore_ascii_case(b"Content-ID")
            || name.eq_ignore_ascii_case(b"Content-Description")
            || name.eq_ignore_ascii_case(b"MIME-Version");
        if drop_field {
            skipping = true;
            continue;
        }
        skipping = false;
        out.extend_from_slice(line);
    }
    if !out.ends_with(b"\n") {
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// 本文テキストパート（各 `(MIME ヘッダ, 本体)`）を multipart/alternative として組み直す。
/// 元ヘッダの識別情報（From/To/Subject/Message-ID/…）は残し、Content-* だけ差し替える。添付は含めない。
/// `parts` が空なら本文なし（ヘッダのみ）として返す。
pub fn reassemble_multipart_text(header: &[u8], parts: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let stripped = strip_content_headers(header);
    if parts.is_empty() {
        // 本文テキストが無い（添付のみ）: ヘッダだけ返す（本文は空になる）。
        let mut out = stripped;
        out.extend_from_slice(b"\r\n");
        return out;
    }
    let boundary = "=_rondine_body_boundary_=";
    let mut out = stripped;
    out.extend_from_slice(b"MIME-Version: 1.0\r\n");
    out.extend_from_slice(
        format!("Content-Type: multipart/alternative; boundary=\"{boundary}\"\r\n\r\n").as_bytes(),
    );
    for (mime, body) in parts {
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        // パートの MIME ヘッダ（charset/転送エンコード）を付けてから本体。空行 1 つで区切る。
        out.extend_from_slice(trim_trailing_crlf(mime));
        out.extend_from_slice(b"\r\n\r\n");
        out.extend_from_slice(body);
        if !body.ends_with(b"\n") {
            out.extend_from_slice(b"\r\n");
        }
    }
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::parser::parse_message;

    // 添付なし multipart/alternative は「ヘッダ + TEXT」で元通りに解析できる。
    #[test]
    fn full_reassembly_preserves_plain_and_html() {
        let header = b"From: a@example.com\r\n\
To: b@example.com\r\n\
Subject: hi\r\n\
Message-ID: <m1@example.com>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/alternative; boundary=\"BND\"\r\n\r\n";
        let text = b"--BND\r\n\
Content-Type: text/plain; charset=utf-8\r\n\r\n\
hello world\r\n\
--BND\r\n\
Content-Type: text/html; charset=utf-8\r\n\r\n\
<p>hello world</p>\r\n\
--BND--\r\n";
        let raw = reassemble_full(header, text);
        let p = parse_message(&raw).expect("parses");
        assert_eq!(p.message_id.as_deref(), Some("m1@example.com"));
        assert_eq!(p.subject.as_deref(), Some("hi"));
        assert!(p.body_plain.as_deref().unwrap().contains("hello world"));
        assert!(p.body_html.as_deref().unwrap().contains("<p>hello world</p>"));
    }

    // strip_content_headers は Content-* だけ落とし識別情報は残す。
    #[test]
    fn strip_removes_only_content_headers() {
        let header = b"From: a@example.com\r\n\
Subject: keep me\r\n\
Content-Type: multipart/mixed; boundary=\"X\"\r\n\
Content-Transfer-Encoding: 7bit\r\n\
Message-ID: <k@example.com>\r\n\r\n";
        let out = strip_content_headers(header);
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("From: a@example.com"));
        assert!(s.contains("Subject: keep me"));
        assert!(s.contains("Message-ID: <k@example.com>"));
        assert!(!s.to_ascii_lowercase().contains("content-type"));
        assert!(!s.to_ascii_lowercase().contains("content-transfer-encoding"));
    }

    // 折り返し（継続行）を持つ Content-Type も丸ごと落とす。
    #[test]
    fn strip_drops_folded_content_type() {
        let header = b"From: a@example.com\r\n\
Content-Type: multipart/mixed;\r\n\tboundary=\"X\"\r\n\
Subject: after\r\n\r\n";
        let out = strip_content_headers(header);
        let s = String::from_utf8_lossy(&out);
        assert!(!s.to_ascii_lowercase().contains("boundary"));
        assert!(s.contains("Subject: after"));
    }

    // 添付ありメール: 本文パートだけ組み直して本文・引用が取れる（添付は入れない）。
    #[test]
    fn multipart_reassembly_yields_body_and_quotes() {
        // 元は multipart/mixed[ multipart/alternative[plain, html], pdf ] を想定。
        let header = b"From: sender@example.com\r\n\
To: me@example.com\r\n\
Subject: with attachment\r\n\
Message-ID: <att1@example.com>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"OUTER\"\r\n\r\n";
        // 本文テキスト（引用付き）。属性行より上が新規本文。
        let plain_mime = b"Content-Type: text/plain; charset=utf-8".to_vec();
        let plain_body = b"New reply text.\r\n\r\n\
On Mon, someone wrote:\r\n\
> quoted old line\r\n"
            .to_vec();
        let parts = vec![(plain_mime, plain_body)];
        let raw = reassemble_multipart_text(header, &parts);
        let p = parse_message(&raw).expect("parses");
        assert_eq!(p.message_id.as_deref(), Some("att1@example.com"));
        assert_eq!(p.subject.as_deref(), Some("with attachment"));
        let clean = p.clean_body.as_deref().unwrap_or("");
        assert!(clean.contains("New reply text."));
        assert!(!clean.contains("quoted old line"));
    }

    // base64 の text/html パートも MIME ヘッダのおかげでデコードされる。
    #[test]
    fn multipart_reassembly_decodes_base64_html() {
        let header = b"From: s@example.com\r\n\
Subject: b64\r\n\
Message-ID: <b64@example.com>\r\n\
Content-Type: multipart/mixed; boundary=\"O\"\r\n\r\n";
        // "<p>hi base64</p>" を base64 化。
        let html_mime =
            b"Content-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: base64".to_vec();
        let html_body = b"PHA+aGkgYmFzZTY0PC9wPg==".to_vec();
        let parts = vec![(html_mime, html_body)];
        let raw = reassemble_multipart_text(header, &parts);
        let p = parse_message(&raw).expect("parses");
        assert!(p.body_html.as_deref().unwrap().contains("hi base64"));
    }
}
