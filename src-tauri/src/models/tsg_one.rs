//! TSG One のアカウント（ログイン）の境界型。Google の接続（`account.rs`）とは別のもの。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// ログインに使う提供元（tsg-core の `/api/auth/{provider}`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "snake_case")]
pub enum TsgOneProvider {
    Google,
    Github,
    Apple,
}

impl TsgOneProvider {
    /// tsg-core の URL に載せる綴り。
    pub fn as_str(self) -> &'static str {
        match self {
            TsgOneProvider::Google => "google",
            TsgOneProvider::Github => "github",
            TsgOneProvider::Apple => "apple",
        }
    }
}

/// ログインしている人（`/api/auth/token` の応答の `user`）。**トークンは載せない。**
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct TsgOneUser {
    /// tsg-core の利用者 ID。
    pub id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    /// ログインに使った提供元（`google` / `github` / `apple`）。
    pub provider: Option<String>,
}

/// 設定の画面に出す TSG One の状態（`tsg_one_status` と "tsg-one:changed" イベントで渡す）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct TsgOneStatus {
    /// ログインしている人（していなければ None）。
    pub user: Option<TsgOneUser>,
    /// ブラウザでのログインを待っている。
    pub pending: bool,
    /// 台数の枠を超えて、この端末が止まっている（ほかの端末を会員サイトで解除してもらう）。
    pub over_cap: bool,
    /// 直前のログイン・確かめ直しが失敗した種類（文言は画面が ja / en で出す）。
    pub problem: Option<TsgOneProblem>,
    /// 失敗の補足（HTTP の状態と tsg-core の code 等。翻訳しない技術的な情報。トークンは含まない）。
    pub problem_detail: Option<String>,
}

/// ログイン・確かめ直しが失敗した種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "snake_case")]
pub enum TsgOneProblem {
    /// ブラウザで取りやめた・提供元が断った。
    Denied,
    /// ログインを始めてから時間が経ちすぎた（もう一度始めてもらう）。
    Expired,
    /// TSG One につながらない。
    Unreachable,
    /// この機械の時計がずれている。
    FixClock,
    /// このアプリの版ではログインできない（更新が要る）。
    UpdateApp,
    /// ログインが切れた（もう一度ログインしてもらう）。
    Relogin,
    /// OS の資格情報ストア・端末の鍵を使えない。
    Storage,
    /// ほか（tsg-core が断った等。補足に理由が入る）。
    Other,
}
