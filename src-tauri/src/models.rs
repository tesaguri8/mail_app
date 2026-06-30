use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 境界型の例。ts-rs により `src/bindings/AppInfo.ts` を生成する。
/// 生成: `npm run gen:bindings`（= cargo test --lib export_bindings）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub identifier: String,
}

/// データベースの状態（スキーマバージョン・パス）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct DbInfo {
    pub schema_version: i32,
    pub path: String,
}

/// プロバイダ自動判定の結果（docs/ONBOARDING.md）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AutoconfigResult {
    pub email: String,
    pub display_name: Option<String>,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_security: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: String,
    pub source: String, // "builtin" | "guess"
    pub note: Option<String>,
}

/// アカウント追加の入力（フロントから受け取る）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AccountInput {
    pub email: String,
    pub display_name: Option<String>,
    /// ログイン用サーバーユーザー名（メールアドレスと別にできる）。未指定なら email を使う。
    pub username: Option<String>,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
}

/// アカウント一覧表示用（資格情報は含めない）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AccountSummary {
    pub id: i32,
    pub email: String,
    pub display_name: Option<String>,
    pub imap_host: String,
    pub smtp_host: String,
}

/// メールサーバーアカウント設定（接続＋ログイン）。再利用・紐づけ用。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ServerAccountSummary {
    pub id: i32,
    pub name: Option<String>,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
}

/// メール一覧表示用（軽量）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct MailSummary {
    pub id: i32,
    pub subject: Option<String>,
    pub from_address: Option<String>,
    pub date: Option<String>,
    pub preview: String,
    pub is_read: bool,
    pub has_attachments: bool,
}

/// メール詳細（本文表示用）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct MailDetail {
    pub id: i32,
    pub subject: Option<String>,
    pub from_address: Option<String>,
    pub to_addresses: Option<String>,
    pub date: Option<String>,
    pub clean_body: Option<String>,
    pub body_plain: Option<String>,
    pub has_attachments: bool,
}

/// 同期結果。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct SyncResult {
    pub fetched: i32,
    pub stored: i32,
}
