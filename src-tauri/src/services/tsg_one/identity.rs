//! TSG One とつなぐときの名乗り（client_id・戻り先・接続先・端末の鍵の名前）。
//!
//! **単一ソースは `config/app-identity.json`**（CLAUDE.md のハードコード排除）。ビルド時に取り込み、
//! channel から戻り先のスキームと接続先を求める。スキームの規則は `scripts/sync-app-identity.mjs` と同じで、
//! 試験で `tauri.conf.json`（deep link の登録）と突き合わせる。

use serde::Deserialize;

/// ビルド時に取り込んだ `config/app-identity.json`。
const APP_IDENTITY_JSON: &str = include_str!("../../../../config/app-identity.json");

/// 配布の段（`config/app-identity.json` の `channel`。組織の語彙は app / preview / dev）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// 本番。
    App,
    /// 配る版（staging の tsg-core につなぐ）。
    Preview,
    /// 手元の開発版（raytrek の dev の tsg-core につなぐ）。
    Dev,
}

/// `config/app-identity.json` のうち、TSG One とつなぐのに使う欄。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIdentity {
    /// アプリの識別子（`tesaguri.rondine.dev`。tauri.conf.json にも同じ値が書かれる）。
    pub identifier: String,
    pub scheme: String,
    pub channel: Channel,
    /// tsg-core の `clients` 表に載った名前（`rondine-desktop`）。
    pub tsg_client_id: String,
}

/// 識別子の単一ソースを読めない（取り込んだ JSON の形が違う）。
#[derive(Debug, thiserror::Error)]
#[error("config/app-identity.json の形が違います: {0}")]
pub struct IdentityError(#[from] serde_json::Error);

/// 開発版だけ、接続先を差し替える環境変数（試験で作り物の tsg-core へ向ける）。本番の版は見ない。
pub const CORE_URL_OVERRIDE_ENV: &str = "RONDINE_TSG_CORE_URL";

impl AppIdentity {
    /// ビルド時に取り込んだ単一ソースを読む。
    ///
    /// # Errors
    /// JSON の形が違えば [`IdentityError`]。
    pub fn load() -> Result<AppIdentity, IdentityError> {
        Ok(serde_json::from_str(APP_IDENTITY_JSON)?)
    }

    /// ログインの戻り先のスキーム。本番は `scheme` のまま、ほかは `<scheme>-<channel>`。
    pub fn redirect_scheme(&self) -> String {
        match self.channel {
            Channel::App => self.scheme.clone(),
            Channel::Preview => format!("{}-preview", self.scheme),
            Channel::Dev => format!("{}-dev", self.scheme),
        }
    }

    /// ログインの戻り先（tsg-core の `clients` 表の `redirect_uris` に載ったもの）。
    pub fn redirect_uri(&self) -> String {
        format!("{}://oauth/callback", self.redirect_scheme())
    }

    /// 端末の鍵の名前（tsg-auth の `app`）。**段ごとに分ける**。
    ///
    /// Windows の鍵は TPM の中で名前だけで決まり、置き場のフォルダーを見ない（tsg-auth の README）。
    /// 分けないと本番・配る版・開発版が同じ鍵を使い、開発版で鍵を作り直すと本番のログインまで切れる
    /// （Primadoc の `device_key_app` と同じ考え）。
    pub fn device_key_app(&self) -> String {
        match self.channel {
            Channel::App => self.tsg_client_id.clone(),
            Channel::Preview => format!("{}.preview", self.tsg_client_id),
            Channel::Dev => format!("{}.dev", self.tsg_client_id),
        }
    }

    /// tsg-core の接続先（dev は raytrek の 8109。tesaguri が規約のポート 8104 を使うため）。
    pub fn core_url(&self) -> &'static str {
        match self.channel {
            Channel::App => "https://accounts.tesaguri.tech",
            Channel::Preview => "https://staging-accounts.tesaguri.tech",
            Channel::Dev => "http://localhost:8109",
        }
    }

    /// 会員サイト（TSG One のブラウザ）。端末の解除など、重い操作はここで行う。
    pub fn member_site_url(&self) -> &'static str {
        match self.channel {
            Channel::App => "https://one.tesaguri.tech",
            Channel::Preview => "https://staging-one.tesaguri.tech",
            Channel::Dev => "http://localhost:3108",
        }
    }
}

/// 実際につなぐ先。開発版（debug の版）だけ [`CORE_URL_OVERRIDE_ENV`] で差し替えられる。
pub fn effective_core_url(identity: &AppIdentity) -> String {
    let overridden = cfg!(debug_assertions)
        .then(|| std::env::var(CORE_URL_OVERRIDE_ENV).ok())
        .flatten()
        .filter(|u| !u.trim().is_empty());
    overridden.unwrap_or_else(|| identity.core_url().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(channel: Channel) -> AppIdentity {
        AppIdentity {
            identifier: "tesaguri.rondine.dev".into(),
            scheme: "rondine".into(),
            channel,
            tsg_client_id: "rondine-desktop".into(),
        }
    }

    #[test]
    fn 戻り先は本番が_rondine_でほかは段を付ける() {
        assert_eq!(
            identity(Channel::App).redirect_uri(),
            "rondine://oauth/callback"
        );
        assert_eq!(
            identity(Channel::Dev).redirect_uri(),
            "rondine-dev://oauth/callback"
        );
        assert_eq!(
            identity(Channel::Preview).redirect_uri(),
            "rondine-preview://oauth/callback"
        );
    }

    #[test]
    fn 端末の鍵の名前は段ごとに分かれ_client_id_は変えない() {
        assert_eq!(identity(Channel::App).device_key_app(), "rondine-desktop");
        assert_eq!(
            identity(Channel::Dev).device_key_app(),
            "rondine-desktop.dev"
        );
        assert_eq!(identity(Channel::Dev).tsg_client_id, "rondine-desktop");
    }

    #[test]
    fn 単一ソースを読める() {
        let id = AppIdentity::load().expect("config/app-identity.json は取り込み時に存在する");
        assert_eq!(id.tsg_client_id, "rondine-desktop");
    }

    /// deep link の登録（sync-app-identity.mjs が tauri.conf.json に書く）と、戻り先のスキームが一致する。
    /// ずれると、ブラウザから戻っても OS がこのアプリに渡さない。
    #[test]
    fn tauri_conf_の_deep_link_と戻り先のスキームが一致する() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../../tauri.conf.json")).expect("JSON");
        let schemes = &conf["plugins"]["deep-link"]["desktop"]["schemes"];
        let id = AppIdentity::load().expect("取り込み時に存在する");
        assert_eq!(
            schemes,
            &serde_json::json!([id.redirect_scheme()]),
            "npm run sync:identity を流してください"
        );
    }
}
