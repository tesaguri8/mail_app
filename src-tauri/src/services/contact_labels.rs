//! 連絡先のラベル（見出し）の語彙。vCard / iCloud / Google の種別を Rondine の表記にそろえる。
//!
//! docs/CONTACT_MODEL.md §1-2: ラベルの語彙は vCard 取り込みに揃える（自宅 / 職場 / 携帯 / FAX /
//! 代表 …）。Google の既定値 `other` は無ラベル、利用者のカスタム名はそのまま使う。
//! 取り込み（vCard の TYPE・iCloud の X-ABLabel・Google の type）と送信（Google の type）が
//! 同じ表を引くので、往復で表記がずれない。

/// 1 つの語: Google の種別（camelCase）と Rondine の表記、別名（vCard の TYPE など）。
struct Term {
    google: &'static str,
    label: &'static str,
    aliases: &'static [&'static str],
}

const TERMS: &[Term] = &[
    Term {
        google: "home",
        label: "自宅",
        aliases: &[],
    },
    Term {
        google: "work",
        label: "職場",
        aliases: &[],
    },
    Term {
        google: "mobile",
        label: "携帯",
        aliases: &["cell", "iphone"],
    },
    Term {
        google: "otherFax",
        label: "FAX",
        aliases: &["fax", "homefax", "workfax"],
    },
    Term {
        google: "main",
        label: "代表",
        aliases: &[],
    },
    Term {
        google: "pager",
        label: "ポケベル",
        aliases: &[],
    },
    Term {
        google: "homePage",
        label: "ホームページ",
        aliases: &["homepage"],
    },
    Term {
        google: "blog",
        label: "ブログ",
        aliases: &[],
    },
    Term {
        google: "profile",
        label: "プロフィール",
        aliases: &[],
    },
    Term {
        google: "anniversary",
        label: "記念日",
        aliases: &[],
    },
    Term {
        google: "spouse",
        label: "配偶者",
        aliases: &[],
    },
    Term {
        google: "child",
        label: "子",
        aliases: &[],
    },
    Term {
        google: "mother",
        label: "母",
        aliases: &[],
    },
    Term {
        google: "father",
        label: "父",
        aliases: &[],
    },
    Term {
        google: "parent",
        label: "親",
        aliases: &[],
    },
    Term {
        google: "brother",
        label: "兄弟",
        aliases: &[],
    },
    Term {
        google: "sister",
        label: "姉妹",
        aliases: &[],
    },
    Term {
        google: "friend",
        label: "友人",
        aliases: &[],
    },
    Term {
        google: "relative",
        label: "親戚",
        aliases: &[],
    },
    Term {
        google: "domesticPartner",
        label: "同居人",
        aliases: &[],
    },
    Term {
        google: "partner",
        label: "パートナー",
        aliases: &[],
    },
    Term {
        google: "manager",
        label: "上司",
        aliases: &[],
    },
    Term {
        google: "assistant",
        label: "アシスタント",
        aliases: &[],
    },
    Term {
        google: "referredBy",
        label: "紹介者",
        aliases: &["referredby"],
    },
];

/// ラベルにしない種別（情報が増えない既定値・vCard の補助的な TYPE）。
const NO_LABEL: &[&str] = &["", "other", "internet", "pref", "voice", "x400", "x-other"];

/// ラベルにしない種別の、各言語での表記。Google の連絡先には種別そのものが「その他」という
/// 文字列（カスタム種別）で入っていることがある（CSV やほかの端末から入った連絡先。`[実測]`
/// 2026-10-08 に試験用アカウントのメール 2,010 件・電話 4,541 件）。既定値と同じ意味なので
/// 無ラベルにする（docs/CONTACT_MODEL.md §1-2）。
const NO_LABEL_LOCALIZED: &[&str] = &["その他"];

/// iCloud の `_$!<Anniversary>!$_` 形式を中身だけにする。それ以外はそのまま。
fn unwrap_apple(raw: &str) -> &str {
    raw.strip_prefix("_$!<")
        .and_then(|r| r.strip_suffix(">!$_"))
        .unwrap_or(raw)
}

/// 取り込んだ種別（vCard の TYPE / iCloud の X-ABLabel / Google の type）を Rondine の表記へ。
///
/// 既知の語は日本語の表記へ、既定値（`other` 等）は None、それ以外はカスタム名としてそのまま返す。
pub fn label_from_term(raw: &str) -> Option<String> {
    let t = unwrap_apple(raw.trim()).trim();
    let lower = t.to_ascii_lowercase();
    if NO_LABEL.contains(&lower.as_str()) || NO_LABEL_LOCALIZED.contains(&t) {
        return None;
    }
    TERMS
        .iter()
        .find(|term| {
            term.google.eq_ignore_ascii_case(&lower) || term.aliases.contains(&lower.as_str())
        })
        .map(|term| term.label.to_string())
        .or_else(|| Some(t.to_string()))
}

/// Google の種別と表示用の種別（`formattedType`。カスタム名はここにだけ入る）から表記を決める。
pub fn label_from_google(value_type: Option<&str>, formatted: Option<&str>) -> Option<String> {
    match value_type.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) if t.eq_ignore_ascii_case("other") => None,
        Some(t) => label_from_term(t),
        // 種別が無いときは表示用の種別（カスタム名）を使う。「その他」は label_from_term が落とす。
        None => formatted.and_then(label_from_term),
    }
}

/// Rondine の表記を Google の種別へ（送信用）。語彙に無い表記はカスタム種別としてそのまま返す。
pub fn google_type_for(label: Option<&str>) -> Option<String> {
    let l = label.map(str::trim).filter(|l| !l.is_empty())?;
    Some(
        TERMS
            .iter()
            .find(|term| term.label == l)
            .map(|term| term.google.to_string())
            .unwrap_or_else(|| l.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_terms_map_to_japanese_labels() {
        assert_eq!(label_from_term("HOME").as_deref(), Some("自宅"));
        assert_eq!(label_from_term("CELL").as_deref(), Some("携帯"));
        assert_eq!(label_from_term("workFax").as_deref(), Some("FAX"));
        assert_eq!(
            label_from_term("_$!<Anniversary>!$_").as_deref(),
            Some("記念日")
        );
        assert_eq!(label_from_term("_$!<Spouse>!$_").as_deref(), Some("配偶者"));
        assert_eq!(label_from_term("INTERNET"), None);
        assert_eq!(label_from_term("_$!<Other>!$_"), None);
        // 未知の語はカスタム名のまま。
        assert_eq!(label_from_term("実家").as_deref(), Some("実家"));
    }

    #[test]
    fn google_types_and_custom_names() {
        assert_eq!(
            label_from_google(Some("mobile"), Some("携帯")).as_deref(),
            Some("携帯")
        );
        assert_eq!(label_from_google(Some("other"), Some("その他")), None);
        // 種別そのものが「その他」（カスタム種別）でも無ラベル。
        assert_eq!(label_from_google(Some("その他"), Some("その他")), None);
        assert_eq!(label_from_google(Some(" その他 "), None), None);
        assert_eq!(label_from_google(None, Some("その他")), None);
        assert_eq!(label_from_term("_$!<その他>!$_"), None);
        assert_eq!(
            label_from_google(None, Some("実家")).as_deref(),
            Some("実家")
        );
        assert_eq!(
            label_from_google(Some("実家"), Some("実家")).as_deref(),
            Some("実家")
        );
    }

    #[test]
    fn labels_round_trip_to_google_types() {
        for t in [
            "home",
            "work",
            "mobile",
            "otherFax",
            "main",
            "homePage",
            "anniversary",
            "spouse",
        ] {
            let label = label_from_term(t);
            assert_eq!(google_type_for(label.as_deref()).as_deref(), Some(t));
        }
        assert_eq!(google_type_for(Some("直通")).as_deref(), Some("直通"));
        assert_eq!(google_type_for(None), None);
    }
}
