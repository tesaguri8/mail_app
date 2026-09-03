//! メールヘッダのアドレス列を解析する小さなユーティリティ。
//! 宛先オートコンプリート（store/recipients.rs）と送信履歴の索引
//! （store/sent_addresses.rs）で共有する。

/// `"Some Name <a@b.com>"` -> `(Some("Some Name"), "a@b.com")`、素の `"a@b.com"` -> `(None, "a@b.com")`。
/// メールらしくない（'@' を含まない）ものは `None`。表示名の前後の引用符は剥がす。
pub fn parse_addr(raw: &str) -> Option<(Option<String>, String)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (name, email) = match (raw.rfind('<'), raw.rfind('>')) {
        (Some(lt), Some(gt)) if gt > lt => {
            let email = raw[lt + 1..gt].trim();
            let name = raw[..lt].trim().trim_matches('"').trim();
            let name = if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            };
            (name, email.to_string())
        }
        _ => (None, raw.to_string()),
    };
    if email.contains('@') && !email.is_empty() {
        Some((name, email))
    } else {
        None
    }
}

/// ヘッダのアドレス列 `"A <a@b>, c@d"` をカンマ/改行/セミコロンで分割し、各要素を解析する。
pub fn split_header_addrs(raw: &str) -> Vec<(Option<String>, String)> {
    raw.split([',', '\n', ';']).filter_map(parse_addr).collect()
}

/// ヘッダのアドレス列から素のメールアドレスだけを小文字で取り出す（表示名は捨てる）。
/// 照合キーに使うため、重複はそのまま（呼び出し側で集約する）。
pub fn lowercase_addrs(raw: &str) -> Vec<String> {
    split_header_addrs(raw)
        .into_iter()
        .map(|(_, email)| email.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_addr_handles_named_and_bare() {
        assert_eq!(
            parse_addr("Alice <alice@corp.com>"),
            Some((Some("Alice".into()), "alice@corp.com".into()))
        );
        assert_eq!(
            parse_addr("bob@corp.com"),
            Some((None, "bob@corp.com".into()))
        );
        // 引用符付きの表示名は剥がす
        assert_eq!(
            parse_addr("\"Carol\" <carol@corp.com>"),
            Some((Some("Carol".into()), "carol@corp.com".into()))
        );
        // メールでないものは無視
        assert_eq!(parse_addr("undisclosed-recipients"), None);
        assert_eq!(parse_addr("   "), None);
    }

    #[test]
    fn split_header_addrs_splits_multiple() {
        let got = split_header_addrs("A <a@x.com>, b@y.com;\nC <c@z.com>");
        assert_eq!(
            got,
            vec![
                (Some("A".into()), "a@x.com".into()),
                (None, "b@y.com".into()),
                (Some("C".into()), "c@z.com".into()),
            ]
        );
    }

    #[test]
    fn lowercase_addrs_drops_names_and_case() {
        assert_eq!(
            lowercase_addrs("Yamada <Yamada@Example.COM>, B@Example.com"),
            vec!["yamada@example.com".to_string(), "b@example.com".to_string()]
        );
    }
}
