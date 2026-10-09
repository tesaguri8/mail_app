//! アカウント（設定のカード）の境界型（docs/ACCOUNTS.md）。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// アカウントの提供元。カードの見出しと、サービスをオンにするときに求める認証を決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum AccountProvider {
    /// メールは App 用パスワード（IMAP）、連絡先・カレンダーは Google でログイン（OAuth）。
    Google,
    /// メール・連絡先・カレンダーとも同じ App 用パスワード（連絡先・カレンダーは後続）。
    Icloud,
    /// その他の IMAP（メールだけ）。
    Imap,
}

impl AccountProvider {
    /// DB に保存する綴り（`account_profiles.provider`）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Icloud => "icloud",
            Self::Imap => "imap",
        }
    }

    /// DB の綴りから戻す。未知の綴りは None。
    pub fn from_db(s: &str) -> Option<Self> {
        match s {
            "google" => Some(Self::Google),
            "icloud" => Some(Self::Icloud),
            "imap" => Some(Self::Imap),
            _ => None,
        }
    }

    /// アドレスと IMAP サーバーから提供元を推し量る（追加の流れで提供元を選ばなかったとき用）。
    ///
    /// マイグレーション 0063 の判定と同じ規則（Gmail / iCloud のサーバーかドメインか）。
    pub fn infer(email: &str, imap_host: &str) -> Self {
        let host = imap_host.trim().to_ascii_lowercase();
        let domain = email
            .rsplit_once('@')
            .map(|(_, d)| d.trim().to_ascii_lowercase())
            .unwrap_or_default();
        match (host.as_str(), domain.as_str()) {
            ("imap.gmail.com" | "imap.googlemail.com", _) => Self::Google,
            ("imap.mail.me.com", _) => Self::Icloud,
            (_, "gmail.com" | "googlemail.com") => Self::Google,
            (_, "icloud.com" | "me.com" | "mac.com") => Self::Icloud,
            _ => Self::Imap,
        }
    }
}

/// 設定の「アカウント」のカード 1 枚（アドレスごと）。
///
/// 中身（メール・Google 連携）はそれぞれの一覧（`AccountSummary` / `GoogleAccount`）にあり、
/// ここはその id で指すだけ。画面はこの id で突き合わせてカードを組む。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AccountProfile {
    pub id: i32,
    pub provider: AccountProvider,
    pub email: String,
    /// 利用者が付けた呼び名。None ならアドレスで出す。
    pub display_name: Option<String>,
    /// このアドレスのメールアカウント（`accounts.id`）。メールを使わないなら空。
    /// 同じアドレスの重複登録（以前は拒んでいなかった）があれば複数になる。
    pub mail_account_ids: Vec<i32>,
    /// このアドレスの Google 連携（`google_accounts.id`）。連携していなければ None。
    pub google_account_id: Option<i32>,
}
