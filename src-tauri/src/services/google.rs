//! Google 連携の共通基盤（OAuth・HTTP・スコープ）。
//!
//! - `oauth`    : デスクトップ用 OAuth（ループバック + PKCE）。トークン取得・更新。
//! - `calendar` : Google カレンダー双方向同期（docs/CALENDAR_SYNC.md）。
//!
//! カレンダーと連絡先は **同じ Google アカウント・同じ refresh_token** を共有する。
//! そのため要求スコープは固定文字列にせず、有効にするサービスから組み立てる（`scopes`）。
//! 資格情報（refresh_token / client_secret）は keyring に保存し、この層は素の文字列で
//! 受け取る（keyring とアプリ識別子の扱いは commands 層に閉じる）。

pub mod calendar;
pub mod oauth;

/// 連携アカウントの特定に常に要るスコープ（メールアドレスの取得）。
pub const SCOPE_IDENTITY: &str = "openid email";
/// カレンダーの読み書き。
pub const SCOPE_CALENDAR: &str = "https://www.googleapis.com/auth/calendar";
/// 連絡先の読み書き（People API）。
pub const SCOPE_CONTACTS: &str = "https://www.googleapis.com/auth/contacts";

/// 製品版に同梱する既定の OAuth クライアント（デスクトップ種別）。
///
/// Google の「デスクトップアプリ」種別は client_secret に秘匿性を求めない（配布物に含める
/// 前提の種別。docs/CALENDAR_SYNC.md）。製品化時にここへ実値を入れると、ユーザーが自分で
/// Google Cloud Console にクライアントを作る手順が不要になる。空の間は、アプリ設定に保存
/// された値または開発用の環境変数だけが使われる。
pub const BUILTIN_CLIENT_ID: &str = "";
pub const BUILTIN_CLIENT_SECRET: &str = "";

pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
pub const USERINFO_ENDPOINT: &str = "https://openidconnect.googleapis.com/v1/userinfo";

/// 要求スコープを組み立てる（`SCOPE_IDENTITY` は常に含める）。
///
/// 例: カレンダーのみなら `scopes(&[SCOPE_CALENDAR])`、両方なら
/// `scopes(&[SCOPE_CALENDAR, SCOPE_CONTACTS])`。
pub fn scopes(services: &[&str]) -> String {
    std::iter::once(SCOPE_IDENTITY)
        .chain(services.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}

/// 共有 HTTP クライアント（タイムアウト付き）。TLS は他と揃えて native-tls。
pub fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP クライアントを初期化できません: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_always_include_identity() {
        assert_eq!(scopes(&[]), SCOPE_IDENTITY);
        assert_eq!(
            scopes(&[SCOPE_CALENDAR, SCOPE_CONTACTS]),
            format!("{SCOPE_IDENTITY} {SCOPE_CALENDAR} {SCOPE_CONTACTS}")
        );
    }
}
