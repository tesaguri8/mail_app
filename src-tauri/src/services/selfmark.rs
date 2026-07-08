//! 自分宛メールの「本物の自分から」検証（docs/SPAM.md）。
//!
//! 送信時、自分のアドレス宛のメールに `X-Rondine-Self: HMAC-SHA256(secret, Message-ID)` を付与する。
//! 受信時に同じ秘密で再計算し一致すれば「この端末の Rondine が送った本物」と判定する。
//! 差出人（From）は詐称できるが、この HMAC は秘密を知らないと作れず、メッセージごとに値が
//! 変わるため（Message-ID に束縛）盗み見ても別メールへ再利用できない。
//!
//! 秘密はアカウント単位（accounts.self_secret に 16 進で保存。資格情報ではなくマーク用の鍵で、
//! これ単体では何のアクセス権も持たない）。

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// 32 バイトの乱数を 16 進文字列で生成する（新規アカウントの self_secret 用）。
pub fn generate_secret_hex() -> String {
    let mut buf = [0u8; 32];
    // 失敗時（極めて稀）でもゼロ埋めのまま進める（検証が働かないだけで実害はない）。
    let _ = getrandom::getrandom(&mut buf);
    hex_encode(&buf)
}

/// Message-ID を送受で同じ形に正規化する（山括弧を外して trim）。
pub fn canon_msgid(s: &str) -> String {
    s.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_string()
}

/// secret(16進) と Message-ID から X-Rondine-Self ヘッダ値（16進）を計算する。
pub fn compute_mark(secret_hex: &str, message_id: &str) -> Option<String> {
    let key = hex_decode(secret_hex)?;
    let mut mac = HmacSha256::new_from_slice(&key).ok()?;
    mac.update(canon_msgid(message_id).as_bytes());
    Some(hex_encode(&mac.finalize().into_bytes()))
}

/// 受信メールの (Message-ID, X-Rondine-Self) が secret で検証できるか。
pub fn verify_mark(secret_hex: &str, message_id: &str, mark_hex: &str) -> bool {
    match compute_mark(secret_hex, message_id) {
        Some(expected) => constant_time_eq(expected.as_bytes(), mark_hex.trim().as_bytes()),
        None => false,
    }
}

/// タイミング差を避ける固定時間比較（長さが違えば即 false）。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_roundtrip_verifies() {
        let secret = generate_secret_hex();
        let mid = "<abc.123@example.com>";
        let mark = compute_mark(&secret, mid).unwrap();
        // 山括弧あり/なしどちらでも同じ HMAC（canon で正規化）。
        assert!(verify_mark(&secret, "abc.123@example.com", &mark));
        assert!(verify_mark(&secret, mid, &mark));
    }

    #[test]
    fn wrong_secret_or_msgid_fails() {
        let secret = generate_secret_hex();
        let other = generate_secret_hex();
        let mark = compute_mark(&secret, "id@x").unwrap();
        assert!(!verify_mark(&other, "id@x", &mark)); // 別の秘密
        assert!(!verify_mark(&secret, "different@x", &mark)); // 別の Message-ID
        assert!(!verify_mark(&secret, "id@x", "deadbeef")); // 別の値
    }
}
