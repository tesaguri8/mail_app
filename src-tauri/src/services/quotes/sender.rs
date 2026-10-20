//! 引用の見出しの差出人の書き方（"名前 <addr>" / "'名前' [mailto:addr]" / addr 単体 ほか）を読む。

/// 引用の差出人の書き方（"山田 花子 <hanako@example.com>" / "hanako@example.com" /
/// "'山田 花子' [mailto:hanako@example.com]" / "山田 花子"）から、アドレスと名前を取り出す。
pub fn split_sender(from: &str) -> (Option<String>, Option<String>) {
    let s = from.trim();
    let addr = if let Some(open) = s.find("mailto:") {
        let rest = &s[open + "mailto:".len()..];
        Some(
            rest.split([']', '>', ' '])
                .next()
                .unwrap_or(rest)
                .to_string(),
        )
    } else if let (Some(a), Some(b)) = (s.find('<'), s.rfind('>')) {
        (a < b).then(|| s[a + 1..b].to_string())
    } else {
        s.split_whitespace()
            .find(|w| w.contains('@'))
            .map(str::to_string)
    }
    .map(|a| a.trim().to_lowercase())
    .filter(|a| a.contains('@'));
    // 名前は、アドレスの部分（<...> / [mailto:...] / アドレス単体）より前。
    let name_end = s.find(['<', '[']).unwrap_or(s.len());
    let name = s[..name_end]
        .trim()
        .trim_matches(['"', '\'', '“', '”'])
        .trim();
    let name = (!name.is_empty() && !name.contains('@')).then(|| name.to_string());
    (addr, name)
}

/// 引用の差出人を画面に出す形にする（名前があれば名前、無ければアドレス、どちらも取れなければ見出しのまま）。
/// "<addr> <mailto:addr>" / "addr <addr> <mailto:addr>" のように、名前の欄にアドレスが入った書き方でも
/// アドレスだけを出す。
pub fn sender_label(from: &str) -> String {
    match split_sender(from) {
        (_, Some(name)) => name,
        (Some(addr), None) => addr,
        (None, None) => from.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_sender_forms() {
        assert_eq!(
            split_sender("山田 花子 <Hanako@Example.com>"),
            (Some("hanako@example.com".into()), Some("山田 花子".into()))
        );
        assert_eq!(
            split_sender("'Yamada Hanako' [mailto:hanako@example.com]"),
            (
                Some("hanako@example.com".into()),
                Some("Yamada Hanako".into())
            )
        );
        assert_eq!(
            split_sender("hanako@example.com"),
            (Some("hanako@example.com".into()), None)
        );
        assert_eq!(split_sender("山田 花子"), (None, Some("山田 花子".into())));
    }

    /// 実物の引用にあった差出人の書き方（名前とアドレスは作り物）。
    #[test]
    fn labels_sender_by_name_or_address() {
        assert_eq!(
            sender_label("<taro@maeda.example> <mailto:taro@maeda.example>"),
            "taro@maeda.example"
        );
        assert_eq!(
            sender_label("taro@maeda.example <taro@maeda.example> <mailto:taro@maeda.example>"),
            "taro@maeda.example"
        );
        assert_eq!(sender_label("高橋 健<12345TAKA@city.example.lg.jp>"), "高橋 健");
        assert_eq!(
            sender_label("木材商事　　山田花子<hanako@mokuzai.example.jp>"),
            "木材商事　　山田花子"
        );
        assert_eq!(sender_label("鈴木次郎"), "鈴木次郎");
        assert_eq!(sender_label("Hanako@Example.com"), "hanako@example.com");
    }
}
