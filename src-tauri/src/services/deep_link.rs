//! deep link（`rondine-dev://oauth/callback?…`）の受け口の判定と、単一インスタンスの名前。
//!
//! Windows / Linux では、ブラウザから戻ると OS がアプリを**もう 1 つ**起こし、URL を引数に渡す。
//! 単一インスタンスのプラグインがそれを受け止め、いま開いている窓へ引数を渡す（`lib.rs`）。
//! macOS は OS が 2 つ目を起こさず、deep link のプラグインから届く。

use sha2::{Digest, Sha256};
use std::path::Path;

/// TSG One のログインの戻り（`<scheme>://oauth/callback…`）か。ほかのスキーム・経路は受けない。
pub fn is_tsg_callback(url: &str, scheme: &str) -> bool {
    url.strip_prefix(scheme)
        .and_then(|rest| rest.strip_prefix("://oauth/callback"))
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('?') || rest.starts_with('#'))
}

/// 開発版の単一インスタンスの名前（Linux の DBus 名）。**実行ファイルごとに分ける。**
///
/// プラグインの既定は識別子（`tesaguri.rondine.dev`）で、raytrek ではメインと作業木（wt-1 / wt-2）の
/// 開発版が同じ識別子で同じセッションのバスに乗る。分けないと、あとから起こした方が前の方へ
/// 引数を渡して閉じてしまい、並べて動かせない。OS が戻りに起こすのは deep link を登録した実行ファイル
/// （`register_all` が起動のたびに自分を登録する）なので、戻りはその実行ファイルの窓に届く。
pub fn dev_instance_id(identifier: &str, exe: &Path) -> String {
    let digest = Sha256::digest(exe.to_string_lossy().as_bytes());
    let hex: String = digest[..4].iter().map(|b| format!("{b:02x}")).collect();
    // DBus 名の要素は数字で始められないので、頭に字を置く
    format!("{identifier}.b{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ログインの戻りだけを受ける() {
        assert!(is_tsg_callback(
            "rondine-dev://oauth/callback?code=a&state=b",
            "rondine-dev"
        ));
        assert!(is_tsg_callback(
            "rondine-dev://oauth/callback",
            "rondine-dev"
        ));
        // 本番のスキーム・別の経路・似た綴りは受けない
        assert!(!is_tsg_callback(
            "rondine://oauth/callback?code=a",
            "rondine-dev"
        ));
        assert!(!is_tsg_callback(
            "rondine-dev://oauth/callbackx?code=a",
            "rondine-dev"
        ));
        assert!(!is_tsg_callback("rondine-dev://other", "rondine-dev"));
        assert!(!is_tsg_callback("/usr/bin/rondine", "rondine-dev"));
    }

    #[test]
    fn 開発版の名前は実行ファイルごとに分かれ_dbus_の名前として通る() {
        let a = dev_instance_id("tesaguri.rondine.dev", Path::new("/a/rondine"));
        let b = dev_instance_id("tesaguri.rondine.dev", Path::new("/b/rondine"));
        assert_ne!(a, b);
        assert_eq!(
            a,
            dev_instance_id("tesaguri.rondine.dev", Path::new("/a/rondine"))
        );
        let last = a.rsplit('.').next().expect("区切りがある");
        assert!(last.starts_with('b') && last.len() == 9);
    }
}
