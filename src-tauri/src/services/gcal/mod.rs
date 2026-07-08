//! Google カレンダー双方向同期（docs/CALENDAR_SYNC.md）。
//!
//! - `oauth`  : デスクトップ用 OAuth（ループバック + PKCE）。トークン取得・更新。
//! - `api`    : Google Calendar API v3 の薄い REST ラッパー。
//! - `convert`: Google の予定 ⇄ ローカル表現（EventSummary/events 行）の相互変換。
//! - `sync`   : 取り込み（pull）と送信（push）を束ねる同期エンジン。
//!
//! 資格情報（refresh_token / client_secret）は keyring に保存し、この層は素の文字列で受け取る
//! （keyring とアプリ識別子の扱いは commands 層に閉じる）。

pub mod api;
pub mod convert;
pub mod oauth;
pub mod sync;

/// 双方向同期に必要なスコープ。calendar（読み書き）＋ openid/email（連携アカウントの特定）。
pub const SCOPES: &str = "https://www.googleapis.com/auth/calendar openid email";

pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
pub const USERINFO_ENDPOINT: &str = "https://openidconnect.googleapis.com/v1/userinfo";
pub const CAL_API_BASE: &str = "https://www.googleapis.com/calendar/v3";

/// 共有 HTTP クライアント（タイムアウト付き）。TLS は他と揃えて native-tls。
pub fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP クライアントを初期化できません: {e}"))
}
