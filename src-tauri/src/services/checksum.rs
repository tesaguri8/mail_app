//! 非暗号の簡易チェックサム（キャッシュの整合とキャッシュ名の目安。改ざん検知用ではない）。

/// バイト列の簡易チェックサム（16 桁の 16 進）。
pub fn simple_checksum(bytes: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:016x}", h.finish())
}
