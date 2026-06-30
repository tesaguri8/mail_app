use crate::models::AutoconfigResult;

/// メールアドレスのドメインから IMAP/SMTP 接続設定を推定する（docs/ONBOARDING.md）。
/// まず内蔵テーブル（主要プロバイダ）、無ければ imap./smtp.<domain> を推測。
/// ※ ネットワーク経由の ISPDB / autodiscover は後続で追加。
pub fn resolve(email: &str) -> AutoconfigResult {
    let domain = email.rsplit('@').next().unwrap_or("").to_lowercase();

    // (imap_host, imap_port, smtp_host, smtp_port, note)
    let builtin: Option<(&str, u16, &str, u16, &str)> = match domain.as_str() {
        "gmail.com" | "googlemail.com" => Some((
            "imap.gmail.com",
            993,
            "smtp.gmail.com",
            587,
            "Gmail はアプリパスワード（2段階認証の設定が必要）で接続します。",
        )),
        "outlook.com" | "hotmail.com" | "hotmail.co.jp" | "live.com" | "live.jp" | "msn.com" => {
            Some((
                "outlook.office365.com",
                993,
                "smtp.office365.com",
                587,
                "Outlook はアプリパスワードが必要な場合があります。",
            ))
        }
        "yahoo.com" | "ymail.com" => Some((
            "imap.mail.yahoo.com",
            993,
            "smtp.mail.yahoo.com",
            465,
            "Yahoo はアプリパスワードが必要です。",
        )),
        "yahoo.co.jp" => Some((
            "imap.mail.yahoo.co.jp",
            993,
            "smtp.mail.yahoo.co.jp",
            465,
            "Yahoo!メール（日本）は IMAP の有効化が必要な場合があります。",
        )),
        "icloud.com" | "me.com" | "mac.com" => Some((
            "imap.mail.me.com",
            993,
            "smtp.mail.me.com",
            587,
            "iCloud は App 用パスワードが必要です。",
        )),
        _ => None,
    };

    if let Some((imap_host, imap_port, smtp_host, smtp_port, note)) = builtin {
        let smtp_security = if smtp_port == 465 { "ssl" } else { "starttls" };
        return AutoconfigResult {
            email: email.to_string(),
            display_name: None,
            imap_host: imap_host.to_string(),
            imap_port,
            imap_security: "ssl".to_string(),
            smtp_host: smtp_host.to_string(),
            smtp_port,
            smtp_security: smtp_security.to_string(),
            source: "builtin".to_string(),
            note: Some(note.to_string()),
        };
    }

    // フォールバック推測
    AutoconfigResult {
        email: email.to_string(),
        display_name: None,
        imap_host: format!("imap.{domain}"),
        imap_port: 993,
        imap_security: "ssl".to_string(),
        smtp_host: format!("smtp.{domain}"),
        smtp_port: 587,
        smtp_security: "starttls".to_string(),
        source: "guess".to_string(),
        note: Some("自動判定できなかったため推測値です。必要に応じて修正してください。".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmail_builtin() {
        let r = resolve("a@gmail.com");
        assert_eq!(r.imap_host, "imap.gmail.com");
        assert_eq!(r.source, "builtin");
    }

    #[test]
    fn unknown_domain_guesses() {
        let r = resolve("user@example.org");
        assert_eq!(r.imap_host, "imap.example.org");
        assert_eq!(r.smtp_host, "smtp.example.org");
        assert_eq!(r.source, "guess");
    }
}
