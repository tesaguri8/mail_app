//! TSG One のアカウントでのログイン（docs/TSG_ONE.md）。Google の接続（`services/google`）とは別のもの。
//!
//! 組み立てと読み取りは tsg-kit の部品（`tsg_auth`）。Primadoc と同じ形にする:
//! 1. ログインを始める（[`TsgOne::start_login`]）: 端末の鍵を開き、鍵の拇印を名乗った認可の URL を作る。
//!    `state` と `code_verifier` は**メモリにだけ**置く
//! 2. ブラウザから deep link（`rondine-dev://oauth/callback?code=…&state=…`）で戻る（[`TsgOne::finish_login`]）。
//!    `state` が合わない戻りは断るが、進めているログインは捨てない（攻撃者のリンクを踏まされても続けられる）
//! 3. 引換券を端末の鍵の proof と `X-Device-ID` を添えてトークンに替える（トークンは鍵に結ばれる）
//! 4. 更新用のトークンは OS 金庫へ、アクセストークン（15 分）はメモリへ。平文のファイルには置かない
//! 5. 端末を登録する（`POST /api/devices`）。**台数の枠を超えた 403 は握り潰さず**、画面に出す
//!
//! 利用権の判定・匿名の利用状況は、tsg-kit の部品を待つ（ここでは作らない）。
//! **秘密鍵・トークン・proof・引換券をログに出さない。**

pub mod identity;
pub mod secret;
pub mod transport;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use tsg_auth::{Action, Approval, AuthError, CallbackError, DeviceKey, HttpRequest, KeyError};

use crate::models::{TsgOneProblem, TsgOneProvider, TsgOneStatus, TsgOneUser};
use identity::{AppIdentity, IdentityError};
use secret::{KeyringStore, SavedLogin, SecretError, SecretStore};
use transport::{HttpTransport, Transport, TransportError};

/// ログインを始めてから戻ってくるまでの猶予。tsg-core がログインの途中の状態を持つ時間
/// （`authorize_service._STATE_MINUTES`）に合わせる。過ぎるとアプリへは戻らない。
pub const LOGIN_TTL: Duration = Duration::from_secs(10 * 60);

/// アクセストークンの期限の手前で更新に回す余裕（ミリ秒）。送っている途中で切れないように。
const ACCESS_MARGIN_MS: i64 = 60_000;

/// "tsg-one:changed" イベント（[`TsgOneStatus`] を載せる）。
pub const CHANGED_EVENT: &str = "tsg-one:changed";

/// TSG One のログインで起きたこと。
#[derive(Debug, thiserror::Error)]
pub enum TsgOneError {
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error("端末の鍵を使えません: {0}")]
    Key(#[from] KeyError),
    #[error(transparent)]
    Secret(#[from] SecretError),
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error(transparent)]
    Rejected(#[from] AuthError),
    #[error(transparent)]
    Callback(#[from] CallbackError),
    #[error("ログインを始めていないか、始めてから時間が経ちすぎました")]
    NoPendingLogin,
    #[error("TSG One にログインしていません")]
    NotSignedIn,
    #[error("トークンの応答にログインした人の情報がありません")]
    NoUser,
    #[error("端末の鍵の処理が止まりました: {0}")]
    Join(#[from] tauri::Error),
}

impl TsgOneError {
    /// 画面に出す種類と、翻訳しない補足。
    pub fn problem(&self) -> (TsgOneProblem, Option<String>) {
        match self {
            TsgOneError::Callback(CallbackError::Denied(code)) => {
                (TsgOneProblem::Denied, Some(code.clone()))
            }
            TsgOneError::NoPendingLogin => (TsgOneProblem::Expired, None),
            TsgOneError::Transport(e) => (TsgOneProblem::Unreachable, Some(e.to_string())),
            TsgOneError::Rejected(e) => {
                let detail = format!("HTTP {} {}", e.status, e.dpop.as_deref().unwrap_or(&e.code));
                let problem = match e.action() {
                    Action::FixClock => TsgOneProblem::FixClock,
                    Action::UpdateApp => TsgOneProblem::UpdateApp,
                    Action::Relogin | Action::NewKeyAndRelogin => TsgOneProblem::Relogin,
                    _ => TsgOneProblem::Other,
                };
                (problem, Some(detail))
            }
            TsgOneError::NotSignedIn => (TsgOneProblem::Relogin, None),
            TsgOneError::Key(_) | TsgOneError::Secret(_) | TsgOneError::Join(_) => {
                (TsgOneProblem::Storage, Some(self.to_string()))
            }
            TsgOneError::Identity(_) | TsgOneError::Callback(_) | TsgOneError::NoUser => {
                (TsgOneProblem::Other, Some(self.to_string()))
            }
        }
    }
}

/// 端末の鍵と、それで組んだ tsg-core とのつなぎ（端末の ID が決まってから組む）。
#[derive(Clone)]
struct Device {
    key: Arc<dyn DeviceKey>,
    client: Arc<tsg_auth::Client>,
}

/// 端末の鍵の開き方。
enum KeySource {
    /// OS の鍵の置き場（Windows は TPM、ほかは `dir/device-key.p8`）。端末の ID も `dir` を使う。
    Os { app: String, dir: PathBuf },
    /// 試験用（鍵と端末の ID を外から渡す）。
    #[cfg(test)]
    Fixed,
}

/// 進めているログイン（メモリにだけ置く）。
struct Pending {
    login: tsg_auth::Login,
    started: Instant,
}

/// メモリにだけ持つアクセストークン。**`Debug` を付けない**（トークンを出さない）。
#[derive(Clone)]
struct Access {
    token: String,
    /// 期限（UNIX 時刻のミリ秒）。
    expires_at: i64,
}

/// メモリにだけ持つ状態。
#[derive(Default)]
struct State {
    pending: Option<Pending>,
    access: Option<Access>,
    over_cap: bool,
    problem: Option<(TsgOneProblem, Option<String>)>,
}

/// [`TsgOne`] の組み立てに要るもの。
pub struct TsgOneParts {
    pub identity: AppIdentity,
    /// tsg-core の接続先（[`identity::effective_core_url`]）。
    pub core_url: String,
    /// 端末の鍵と、機械 ID が読めないときの退避の ID を置くフォルダー。
    /// **データの置き場（移動できる）ではなく、この機械のアプリのフォルダーにする**（鍵を別の機械へ写さない）。
    pub device_dir: PathBuf,
    /// 端末の一覧に出す名前（OS のコンピューター名）。
    pub device_name: Option<String>,
    pub secrets: Box<dyn SecretStore>,
    pub transport: Box<dyn Transport>,
}

/// 本物の部品（OS 金庫・reqwest・OS の鍵の置き場）で組み立てる。
///
/// `app_dir` はこの機械のアプリのフォルダー（`app_data_dir`）。端末の鍵は `app_dir/tsg-one` に置く。
/// `identifier` は OS 金庫のサービス名（メールのアカウントの資格情報と同じ）。
///
/// # Errors
/// 単一ソースの形が違う・TLS を初期化できないと [`TsgOneError`]。
pub fn build(app_dir: &Path, identifier: &str) -> Result<TsgOne, TsgOneError> {
    let identity = AppIdentity::load()?;
    Ok(TsgOne::new(TsgOneParts {
        core_url: identity::effective_core_url(&identity),
        identity,
        device_dir: app_dir.join("tsg-one"),
        device_name: hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .filter(|h| !h.is_empty()),
        secrets: Box::new(KeyringStore::new(identifier)),
        transport: Box::new(HttpTransport::new()?),
    }))
}

/// TSG One のログイン（Tauri の managed state）。
pub struct TsgOne {
    identity: AppIdentity,
    core_url: String,
    device_name: Option<String>,
    key_source: KeySource,
    device: Arc<Mutex<Option<Device>>>,
    secrets: Box<dyn SecretStore>,
    transport: Box<dyn Transport>,
    state: Mutex<State>,
    /// 更新用のトークンは使い捨てなので、更新を 1 本ずつにする（2 本が同じ券で更新すると、
    /// 2 本目が `invalid_grant` になり、ログインごと失う）。
    refreshing: tokio::sync::Mutex<()>,
}

impl TsgOne {
    /// 組み立てる（鍵はまだ開かない。最初に要ったときに開く）。
    pub fn new(parts: TsgOneParts) -> TsgOne {
        let app = parts.identity.device_key_app();
        TsgOne {
            key_source: KeySource::Os {
                app,
                dir: parts.device_dir,
            },
            identity: parts.identity,
            core_url: parts.core_url,
            device_name: parts.device_name,
            device: Arc::default(),
            secrets: parts.secrets,
            transport: parts.transport,
            state: Mutex::default(),
            refreshing: tokio::sync::Mutex::new(()),
        }
    }

    /// ログインの戻り先のスキーム（deep link の受け口で、自分宛てかを見分ける）。
    pub fn redirect_scheme(&self) -> String {
        self.identity.redirect_scheme()
    }

    /// 会員サイト（端末の解除など、重い操作はブラウザのここで行う）。
    pub fn member_site_url(&self) -> &'static str {
        self.identity.member_site_url()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        // 状態はトークンの控えと表示の印だけ。途中で落ちた書き込みがあっても続けてよい
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 設定の画面に出す状態。金庫を読めなければ、ログインしていない扱いで理由を載せる。
    pub fn status(&self) -> TsgOneStatus {
        let loaded = self.secrets.load();
        let state = self.state();
        let (user, storage_problem) = match loaded {
            Ok(saved) => (saved.map(|s| s.user), None),
            Err(e) => {
                log::warn!("[TsgOne] ログインの控えを読めません: {e}");
                (None, Some((TsgOneProblem::Storage, Some(e.to_string()))))
            }
        };
        let (problem, problem_detail) = storage_problem
            .or_else(|| state.problem.clone())
            .map_or((None, None), |(p, d)| (Some(p), d));
        TsgOneStatus {
            over_cap: user.is_some() && state.over_cap,
            user,
            pending: state
                .pending
                .as_ref()
                .is_some_and(|p| p.started.elapsed() < LOGIN_TTL),
            problem,
            problem_detail,
        }
    }

    /// 結果の失敗を、画面に出す理由として覚える（成功なら前の理由を消す）。
    fn remember<T>(&self, result: Result<T, TsgOneError>) -> Result<T, TsgOneError> {
        let mut state = self.state();
        match &result {
            Ok(_) => state.problem = None,
            // state の合わない戻りは、進めているログインを続けるので理由にしない
            Err(TsgOneError::Callback(CallbackError::StateMismatch)) => {}
            Err(e) => state.problem = Some(e.problem()),
        }
        result
    }

    /// 端末の鍵とつなぎを返す（まだ開いていなければ開く。鍵が無ければ作る）。
    ///
    /// **TPM の操作は待つことがある**ので、非同期の実行環境を塞がないよう別スレッドで開く。
    /// 開くあいだは握るので、2 本が同時に鍵を作りに行くことはない
    async fn device(&self) -> Result<Device, TsgOneError> {
        let (app, dir) = match &self.key_source {
            KeySource::Os { app, dir } => (app.clone(), dir.clone()),
            // 試験の鍵は組み立てのときに入れてある
            #[cfg(test)]
            KeySource::Fixed => {
                return self
                    .device
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
                    .ok_or(TsgOneError::NotSignedIn)
            }
        };
        let slot = Arc::clone(&self.device);
        let client = tsg_auth::Client {
            base_url: self.core_url.clone(),
            client_id: self.identity.tsg_client_id.clone(),
            redirect_uri: self.identity.redirect_uri(),
            device_id: String::new(),
            platform: tsg_auth::platform().map(str::to_string),
            device_name: self.device_name.clone(),
        };
        tauri::async_runtime::spawn_blocking(move || {
            let mut slot = slot.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(device) = slot.as_ref() {
                return Ok(device.clone());
            }
            std::fs::create_dir_all(&dir).map_err(KeyError::from)?;
            let key: Arc<dyn DeviceKey> = Arc::from(tsg_auth::open_device_key(&app, &dir)?);
            // 機械 1 台の単位・全アプリで同じ値（tsg-core はこれで台数の枠を数える）
            let device_id = tsg_auth::device_id(&dir)?;
            log::info!(
                "[TsgOne] 端末の鍵を開きました（置き場: {}）",
                key.storage().as_str()
            );
            let device = Device {
                key,
                client: Arc::new(tsg_auth::Client {
                    device_id,
                    ..client
                }),
            };
            *slot = Some(device.clone());
            Ok(device)
        })
        .await?
    }

    /// 端末の鍵を消す（端末が解除された）。次に要ったときに作り直す。
    async fn reset_device_key(&self) -> Result<(), TsgOneError> {
        *self.device.lock().unwrap_or_else(PoisonError::into_inner) = None;
        let (app, dir) = match &self.key_source {
            KeySource::Os { app, dir } => (app.clone(), dir.clone()),
            #[cfg(test)]
            KeySource::Fixed => return Ok(()),
        };
        tauri::async_runtime::spawn_blocking(move || tsg_auth::delete_device_key(&app, &dir))
            .await??;
        log::warn!("[TsgOne] 端末の鍵を消しました（次のログインで作り直します）");
        Ok(())
    }

    /// 鍵で署名して要求を作る（TPM の署名は待つことがあるので別スレッドで）。
    async fn signed<F>(device: &Device, build: F) -> Result<HttpRequest, TsgOneError>
    where
        F: FnOnce(&tsg_auth::Client, &dyn DeviceKey) -> Result<HttpRequest, KeyError>
            + Send
            + 'static,
    {
        let device = device.clone();
        Ok(
            tauri::async_runtime::spawn_blocking(move || {
                build(&device.client, device.key.as_ref())
            })
            .await??,
        )
    }

    /// ログインを始め、ブラウザで開く URL を返す。前に進めていたログインは捨てる。
    ///
    /// **鍵を先に開く。**開けない（TPM が断った等）なら、ブラウザでログインさせてから
    /// 引換券を替えられずに終わるより、ここで理由を返すほうがよい
    pub async fn start_login(&self, provider: TsgOneProvider) -> Result<String, TsgOneError> {
        let result = async {
            let device = self.device().await?;
            let login = tsg_auth::Login::start(
                &device.client.base_url,
                provider.as_str(),
                &device.client.client_id,
                &device.client.redirect_uri,
                device.key.as_ref(),
            );
            let url = login.url().to_string();
            self.state().pending = Some(Pending {
                login,
                started: Instant::now(),
            });
            Ok(url)
        }
        .await;
        self.remember(result)
    }

    /// 進めているログインを取りやめる。取りやめたあとに届いた戻りは断る。
    pub fn cancel_login(&self) {
        let mut state = self.state();
        state.pending = None;
        state.problem = None;
    }

    /// ログインの戻り（deep link の URL）を受け、引換券をトークンに替えて保存する。
    pub async fn finish_login(&self, callback_url: &str) -> Result<(), TsgOneError> {
        let result = self.finish_login_inner(callback_url).await;
        self.remember(result)
    }

    async fn finish_login_inner(&self, callback_url: &str) -> Result<(), TsgOneError> {
        let (code, verifier) = {
            let mut state = self.state();
            let pending = state
                .pending
                .take_if(|p| p.started.elapsed() < LOGIN_TTL)
                .ok_or(TsgOneError::NoPendingLogin)?;
            match pending.login.finish(callback_url) {
                Ok(pair) => pair,
                Err(CallbackError::StateMismatch) => {
                    // このアプリが始めたログインの戻りではない。本物のログインは待ち続ける
                    state.pending = Some(pending);
                    return Err(CallbackError::StateMismatch.into());
                }
                Err(e) => return Err(e.into()),
            }
        };

        let device = self.device().await?;
        let request = Self::signed(&device, move |client, key| {
            client.exchange_code(key, &code, &verifier, tsg_auth::now_secs())
        })
        .await?;
        let (status, body) = self.transport.send(&request).await?;
        let tokens = tsg_auth::read_tokens(status, &body)?;
        let user = tokens.user.clone().ok_or(TsgOneError::NoUser)?;
        self.secrets.save(&SavedLogin {
            refresh_token: tokens.refresh_token.clone(),
            refresh_token_expires_at: tokens.refresh_token_expires_at,
            user: TsgOneUser {
                id: user.id,
                email: user.email,
                display_name: user.display_name,
                provider: user.provider,
            },
        })?;
        let over_cap = tokens
            .device_key
            .as_ref()
            .is_some_and(|k| k.status == Approval::OverCap);
        {
            let mut state = self.state();
            state.access = Some(Access {
                token: tokens.access_token.clone(),
                expires_at: tokens.access_token_expires_at,
            });
            state.over_cap = over_cap;
        }
        log::info!("[TsgOne] ログインしました（トークンは端末の鍵に結んだもの）");

        // 枠の中なら `/token` が登録まで済ませているので、これは名前と最終接続の更新になる。
        // **枠を超えた 403 は黙って流さない**（画面が会員サイトでの解除へ案内する）。
        // ほかの失敗はログインを止めない（次の確かめ直しで送り直す）
        match self.register_device(&device, &tokens.access_token).await {
            Ok(registered_over_cap) => self.state().over_cap = registered_over_cap,
            Err(e) => log::warn!("[TsgOne] 端末の登録に失敗しました（ログインは続けます）: {e}"),
        }
        Ok(())
    }

    /// この端末を登録する（`POST /api/devices`。冪等）。枠を超えていれば `Ok(true)`。
    async fn register_device(
        &self,
        device: &Device,
        access_token: &str,
    ) -> Result<bool, TsgOneError> {
        let token = access_token.to_string();
        let request = Self::signed(device, move |client, key| {
            client.register_device(key, &token, tsg_auth::now_secs())
        })
        .await?;
        let (status, body) = self.transport.send(&request).await?;
        match tsg_auth::read_ok(status, &body) {
            Ok(()) => Ok(false),
            Err(e) if e.action() == Action::CapExceeded => {
                log::warn!(
                    "[TsgOne] 端末の枠を超えています（種別 {} / 上限 {}）",
                    e.kind.as_deref().unwrap_or("?"),
                    e.cap.map_or_else(|| "?".to_string(), |c| c.to_string())
                );
                Ok(true)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// 使えるアクセストークン（期限が近ければ更新する）。
    async fn access_token(&self, device: &Device) -> Result<String, TsgOneError> {
        let fresh = |state: &State| {
            state
                .access
                .as_ref()
                .filter(|a| a.expires_at - ACCESS_MARGIN_MS > now_millis())
                .map(|a| a.token.clone())
        };
        if let Some(token) = fresh(&self.state()) {
            return Ok(token);
        }
        let _one_at_a_time = self.refreshing.lock().await;
        // 待っているあいだに、前の 1 本が更新を済ませたかもしれない
        if let Some(token) = fresh(&self.state()) {
            return Ok(token);
        }
        let saved = self.secrets.load()?.ok_or(TsgOneError::NotSignedIn)?;
        let refresh_token = saved.refresh_token.clone();
        let request = Self::signed(device, move |client, key| {
            client.refresh(key, &refresh_token, tsg_auth::now_secs())
        })
        .await?;
        let (status, body) = self.transport.send(&request).await?;
        let tokens = match tsg_auth::read_tokens(status, &body) {
            Ok(tokens) => tokens,
            Err(e) => {
                log::warn!(
                    "[TsgOne] トークンの更新を断られました: HTTP {} {} → {:?}",
                    e.status,
                    e.dpop.as_deref().unwrap_or(&e.code),
                    e.action()
                );
                match e.action() {
                    Action::Relogin => self.forget_local_login()?,
                    Action::NewKeyAndRelogin => {
                        self.forget_local_login()?;
                        self.reset_device_key().await?;
                    }
                    _ => {}
                }
                return Err(e.into());
            }
        };
        // 更新用のトークンは使い捨て。替わったものを置き直す（人の情報は前の控えのまま）
        self.secrets.save(&SavedLogin {
            refresh_token: tokens.refresh_token.clone(),
            refresh_token_expires_at: tokens.refresh_token_expires_at,
            user: saved.user,
        })?;
        let mut state = self.state();
        state.access = Some(Access {
            token: tokens.access_token.clone(),
            expires_at: tokens.access_token_expires_at,
        });
        if let Some(k) = &tokens.device_key {
            state.over_cap = k.status == Approval::OverCap;
        }
        Ok(tokens.access_token)
    }

    /// この端末を登録し直して、台数の枠を確かめ直す（会員サイトでほかの端末を解除したあと）。
    pub async fn recheck_device(&self) -> Result<(), TsgOneError> {
        let result = async {
            let device = self.device().await?;
            let token = self.access_token(&device).await?;
            let over_cap = self.register_device(&device, &token).await?;
            self.state().over_cap = over_cap;
            Ok(())
        }
        .await;
        self.remember(result)
    }

    /// ログアウトする。サーバー側のトークンの取り消しは試みるだけで、失敗しても手元は必ず消す。
    pub async fn logout(&self) -> Result<(), TsgOneError> {
        let refresh_token = match self.secrets.load() {
            Ok(saved) => saved.map(|s| s.refresh_token),
            Err(e) => {
                log::warn!("[TsgOne] ログアウト: ログインの控えを読めません: {e}");
                None
            }
        };
        let access = self.state().access.clone();
        if refresh_token.is_some() || access.is_some() {
            match self.revoke_on_server(refresh_token, access).await {
                Ok(()) => log::info!("[TsgOne] ログアウト: サーバー側のトークンを取り消しました"),
                Err(e) => log::warn!(
                    "[TsgOne] ログアウト: サーバー側の取り消しに失敗しました（手元は消します）: {e}"
                ),
            }
        }
        self.forget_local_login()?;
        self.state().problem = None;
        Ok(())
    }

    /// `POST /api/auth/logout`（本文に更新用のトークン。アクセストークンがあれば鍵の proof も添える）。
    async fn revoke_on_server(
        &self,
        refresh_token: Option<String>,
        access: Option<Access>,
    ) -> Result<(), TsgOneError> {
        let url = format!("{}/api/auth/logout", self.core_url.trim_end_matches('/'));
        let body = refresh_token.map_or_else(
            || serde_json::json!({}),
            |rt| serde_json::json!({ "refreshToken": rt }),
        );
        let mut headers = vec![("Content-Type", "application/json".to_string())];
        if let Some(access) = access {
            // 鍵を開けなくてもログアウトは止めない（本文の更新用のトークンだけで取り消せる）
            match self.device().await {
                Ok(device) => {
                    let url = url.clone();
                    let auth = tauri::async_runtime::spawn_blocking(move || {
                        device.client.api_headers(
                            device.key.as_ref(),
                            "POST",
                            &url,
                            &access.token,
                            tsg_auth::now_secs(),
                        )
                    })
                    .await??;
                    headers.extend(auth);
                }
                Err(e) => log::warn!("[TsgOne] ログアウト: 端末の鍵を開けないため、鍵の proof を付けずに送ります: {e}"),
            }
        }
        let request = HttpRequest {
            method: "POST",
            url,
            headers,
            body: body.to_string(),
        };
        let (status, body) = self.transport.send(&request).await?;
        Ok(tsg_auth::read_ok(status, &body)?)
    }

    /// 手元のログイン（金庫の控え・メモリのトークン・枠の印）を消す。
    fn forget_local_login(&self) -> Result<(), TsgOneError> {
        {
            let mut state = self.state();
            state.access = None;
            state.over_cap = false;
            state.pending = None;
        }
        self.secrets.clear()?;
        Ok(())
    }
}

/// 手元の時計（UNIX 時刻のミリ秒）。
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests;
