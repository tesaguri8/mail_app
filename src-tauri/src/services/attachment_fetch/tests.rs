use super::*;

/// MIME ヘッダ＋分割して貯めた本体（base64 の途中で切れていてよい）から、元の中身に戻る。
#[test]
fn chunks_split_inside_base64_assemble_back() {
    let original: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&original);
    // 76 桁で折り返した本文（メールの base64 と同じ形）を、半端な位置で 3 つに分けて貯める。
    let body: String = encoded
        .as_bytes()
        .chunks(76)
        .map(|l| format!("{}\r\n", std::str::from_utf8(l).unwrap()))
        .collect();
    let mut partial = Vec::new();
    for piece in body.as_bytes().chunks(CHUNK as usize - 7) {
        partial.extend_from_slice(piece);
    }
    let mime = b"Content-Type: application/pdf; name=\"mitsumori.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
Content-Disposition: attachment; filename=\"mitsumori.pdf\"\r\n";
    let f = assemble(mime, &partial, 0).unwrap();
    assert_eq!(f.bytes, original);
    assert_eq!(f.filename, "mitsumori.pdf");
}

/// 時間切れ・切断は張り直せば続きから取れる。サーバーの NO や解析の失敗はそうではない。
#[test]
fn only_timeouts_and_disconnects_are_retried() {
    let retry = |e: AttachmentError| e.is_retryable();
    assert!(retry(ImapError::Timeout.into()));
    assert!(retry(ImapError::Imap(imap::Error::ConnectionLost).into()));
    assert!(retry(
        ImapError::Imap(imap::Error::Io(std::io::Error::from(
            std::io::ErrorKind::ConnectionReset
        )))
        .into()
    ));
    assert!(!retry(ImapError::Imap(imap::Error::No("見つかりません".into())).into()));
    assert!(!retry(AttachmentError::Parse("x")));
}

/// 締め切りを過ぎたら、次の接続を張らずに時間切れで戻る。
#[test]
fn no_new_connection_after_the_deadline() {
    let past = Instant::now() - Duration::from_millis(1);
    assert!(matches!(
        remaining(ImapTimeouts::ATTACHMENT, Some(past)),
        Err(ImapError::Timeout)
    ));
    let left = remaining(
        ImapTimeouts::ATTACHMENT,
        Some(Instant::now() + Duration::from_secs(5)),
    )
    .unwrap();
    assert!(left.total.unwrap() <= Duration::from_secs(5));
}

/// 同じ添付の札は 1 つ（二重に取らずに合流する）。別の添付は別の札。
#[test]
fn one_gate_per_attachment() {
    let f = AttachmentFetches::default();
    assert!(Arc::ptr_eq(&f.gate(1), &f.gate(1)));
    assert!(!Arc::ptr_eq(&f.gate(1), &f.gate(2)));
}

#[test]
fn file_names_are_made_safe() {
    assert_eq!(sanitize_filename("a/b:c?.pdf"), "a_b_c_.pdf");
    assert_eq!(sanitize_filename(" .. "), "attachment");
}
