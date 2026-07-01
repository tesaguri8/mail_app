//! 本文テキストの保存圧縮（zstd）。
//!
//! HTML 本文はメール容量の大半を占めるが、テキストなので zstd で 80% 以上縮む。
//! 検索対象の clean_body は非圧縮のまま残し、表示専用の body_html だけ圧縮する。

/// 圧縮レベル。12 は速度と圧縮率のバランス点で、HTML テキストなら数倍〜十倍縮む。
/// 起動時の一括圧縮や同期時の逐次圧縮でも体感を損なわない速度。
const LEVEL: i32 = 12;

/// テキストを zstd 圧縮してバイト列にする。
pub fn compress_text(text: &str) -> Vec<u8> {
    // 入力は妥当な UTF-8。圧縮失敗は実質起き得ないが、保険で元バイトにフォールバックしない
    // （decompress 側が zstd 前提のため）。zstd::encode_all はメモリ上で完結する。
    zstd::encode_all(text.as_bytes(), LEVEL).unwrap_or_default()
}

/// zstd 圧縮バイト列をテキストへ復元する。
pub fn decompress_text(bytes: &[u8]) -> Result<String, String> {
    let raw = zstd::decode_all(bytes).map_err(|e| format!("本文の展開に失敗: {e}"))?;
    String::from_utf8(raw).map_err(|e| format!("本文の文字コード変換に失敗: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = "<html><body>".to_string() + &"これはテスト。".repeat(500) + "</body></html>";
        let z = compress_text(&s);
        assert!(z.len() < s.len(), "圧縮で小さくなるはず");
        assert_eq!(decompress_text(&z).unwrap(), s);
    }
}
