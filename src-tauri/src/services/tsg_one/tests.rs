//! TSG One のログインの試験。**tsg-core へは送らない**（送る口と金庫を差し替える）。

use super::identity::{AppIdentity, Channel};
use super::secret::MemoryStore;
use super::transport::{Response, Sent, Transport, TransportError};
use super::*;

const CORE: &str = "http://core.invalid";
const DEVICE_ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

type Responder = Box<dyn Fn(&HttpRequest) -> Option<Response> + Send + Sync>;

/// 送った要求を覚え、決めておいた応答を返す。`None` を返すと「つながらない」。
struct FakeCore {
    sent: Arc<Mutex<Vec<HttpRequest>>>,
    respond: Responder,
}

impl Transport for FakeCore {
    fn send<'a>(&'a self, request: &'a HttpRequest) -> Sent<'a> {
        self.sent.lock().expect("試験").push(request.clone());
        let response = (self.respond)(request).ok_or(TransportError::Simulated);
        Box::pin(async move { response })
    }
}

fn path(request: &HttpRequest) -> &str {
    request.url.strip_prefix(CORE).unwrap_or(&request.url)
}

fn body(request: &HttpRequest) -> serde_json::Value {
    serde_json::from_str(&request.body).unwrap_or_default()
}

fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn tokens_json(refresh: &str, status: &str, with_user: bool) -> String {
    let far = now_millis() + 15 * 60 * 1000;
    let mut data = serde_json::json!({
        "accessToken": format!("at-{refresh}"),
        "refreshToken": refresh,
        "accessTokenExpiresAt": far,
        "refreshTokenExpiresAt": far,
        "deviceKey": { "id": "k1", "status": status },
    });
    if with_user {
        data["user"] = serde_json::json!({
            "id": "u1", "email": "taro@example.com", "displayName": "Taro", "provider": "google"
        });
    }
    serde_json::json!({ "success": true, "data": data }).to_string()
}

fn ok_json() -> String {
    r#"{"success":true,"data":{}}"#.to_string()
}

fn error_json(code: &str) -> String {
    serde_json::json!({ "success": false, "error": { "code": code, "message": code } }).to_string()
}

/// 作り物の tsg-core。トークンの交換・更新・端末の登録・ログアウトに答える。
fn happy_core(request: &HttpRequest) -> Option<Response> {
    match path(request) {
        "/api/auth/token" if body(request)["grant_type"] == "authorization_code" => {
            Some((200, tokens_json("rt-1", "approved", true)))
        }
        "/api/auth/token" => Some((200, tokens_json("rt-2", "approved", false))),
        "/api/devices" | "/api/auth/logout" => Some((200, ok_json())),
        _ => Some((404, error_json("NOT_FOUND"))),
    }
}

struct Harness {
    tsg: TsgOne,
    sent: Arc<Mutex<Vec<HttpRequest>>>,
}

impl Harness {
    fn new(respond: impl Fn(&HttpRequest) -> Option<Response> + Send + Sync + 'static) -> Harness {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let identity = AppIdentity {
            identifier: "tesaguri.rondine.dev".into(),
            scheme: "rondine".into(),
            channel: Channel::Dev,
            tsg_client_id: "rondine-desktop".into(),
        };
        let client = tsg_auth::Client {
            base_url: CORE.into(),
            client_id: identity.tsg_client_id.clone(),
            redirect_uri: identity.redirect_uri(),
            device_id: DEVICE_ID.into(),
            platform: Some("linux".into()),
            device_name: Some("raytrek".into()),
        };
        let device = Device {
            key: Arc::new(tsg_auth::SoftwareKey::generate()),
            client: Arc::new(client),
        };
        let tsg = TsgOne {
            identity,
            core_url: CORE.into(),
            device_name: Some("raytrek".into()),
            key_source: KeySource::Fixed,
            device: Arc::new(Mutex::new(Some(device))),
            secrets: Box::new(MemoryStore::default()),
            transport: Box::new(FakeCore {
                sent: Arc::clone(&sent),
                respond: Box::new(respond),
            }),
            state: Mutex::default(),
            refreshing: tokio::sync::Mutex::new(()),
        };
        Harness { tsg, sent }
    }

    fn sent(&self) -> Vec<HttpRequest> {
        self.sent.lock().expect("試験").clone()
    }

    /// ログインを始め、戻りの URL（tsg-core が付ける state を写したもの）を返す。
    async fn start(&self) -> (String, String) {
        let url = self
            .tsg
            .start_login(TsgOneProvider::Google)
            .await
            .expect("始められる");
        let state = url
            .split('&')
            .find_map(|kv| kv.strip_prefix("state="))
            .expect("state が載る")
            .to_string();
        let callback = format!("rondine-dev://oauth/callback?code=c-1&state={state}");
        (url, callback)
    }
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tauri::async_runtime::block_on(f)
}

#[test]
fn ログインの開始は_client_と戻り先と鍵の拇印を載せ_何も送らない() {
    let h = Harness::new(happy_core);
    let (url, _) = block_on(h.start());
    assert!(url.starts_with(&format!("{CORE}/api/auth/google?client_id=rondine-desktop")));
    assert!(url.contains("redirect_uri=rondine-dev%3A%2F%2Foauth%2Fcallback"));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("dpop_jkt="));
    assert!(h.sent().is_empty());
    assert!(h.tsg.status().pending);
    assert_eq!(h.tsg.status().user, None);
}

#[test]
fn 戻りを受けると引換券を鍵の_proof_付きで替え_金庫に置き_端末を登録する() {
    let h = Harness::new(happy_core);
    block_on(async {
        let (_, callback) = h.start().await;
        h.tsg.finish_login(&callback).await.expect("ログインできる");
    });

    let sent = h.sent();
    assert_eq!(sent.len(), 2);
    let exchange = &sent[0];
    assert_eq!(path(exchange), "/api/auth/token");
    assert_eq!(header(exchange, "X-Device-ID"), Some(DEVICE_ID));
    assert!(header(exchange, "DPoP").is_some());
    let b = body(exchange);
    assert_eq!(b["grant_type"], "authorization_code");
    assert_eq!(b["code"], "c-1");
    assert_eq!(b["client_id"], "rondine-desktop");
    assert_eq!(b["redirect_uri"], "rondine-dev://oauth/callback");
    assert_eq!(b["deviceName"], "raytrek");
    assert!(b["code_verifier"].as_str().is_some_and(|v| v.len() >= 43));

    let register = &sent[1];
    assert_eq!((register.method, path(register)), ("POST", "/api/devices"));
    assert_eq!(header(register, "Authorization"), Some("DPoP at-rt-1"));
    assert_eq!(body(register)["deviceId"], DEVICE_ID);

    let status = h.tsg.status();
    let user = status.user.expect("ログインしている");
    assert_eq!(user.email.as_deref(), Some("taro@example.com"));
    assert!(!status.pending && !status.over_cap);
    assert_eq!(status.problem, None);
    let saved = h.tsg.secrets.load().expect("読める").expect("置いてある");
    assert_eq!(saved.refresh_token, "rt-1");
}

#[test]
fn state_の合わない戻りは断るが_進めているログインは続けられる() {
    let h = Harness::new(happy_core);
    block_on(async {
        let (_, callback) = h.start().await;
        let forged = "rondine-dev://oauth/callback?code=evil&state=forged";
        assert!(matches!(
            h.tsg.finish_login(forged).await,
            Err(TsgOneError::Callback(CallbackError::StateMismatch))
        ));
        assert!(h.sent().is_empty());
        assert!(h.tsg.status().pending);
        assert_eq!(h.tsg.status().problem, None);
        h.tsg
            .finish_login(&callback)
            .await
            .expect("本物の戻りは通る");
    });
    assert!(h.tsg.status().user.is_some());
}

#[test]
fn 始めていないログインの戻りは受けない() {
    let h = Harness::new(happy_core);
    let result = block_on(
        h.tsg
            .finish_login("rondine-dev://oauth/callback?code=c&state=s"),
    );
    assert!(matches!(result, Err(TsgOneError::NoPendingLogin)));
    assert_eq!(h.tsg.status().problem, Some(TsgOneProblem::Expired));
    assert!(h.sent().is_empty());
}

#[test]
fn 取りやめたログインの戻りも受けない() {
    let h = Harness::new(happy_core);
    block_on(async {
        let (_, callback) = h.start().await;
        h.tsg.cancel_login();
        assert!(matches!(
            h.tsg.finish_login(&callback).await,
            Err(TsgOneError::NoPendingLogin)
        ));
    });
    assert!(h.sent().is_empty());
}

#[test]
fn ブラウザで取りやめると理由を出し_待つのをやめる() {
    let h = Harness::new(happy_core);
    block_on(async {
        let (_, callback) = h.start().await;
        let denied = callback.replace("code=c-1", "error=access_denied");
        assert!(h.tsg.finish_login(&denied).await.is_err());
    });
    let status = h.tsg.status();
    assert_eq!(status.problem, Some(TsgOneProblem::Denied));
    assert_eq!(status.problem_detail.as_deref(), Some("access_denied"));
    assert!(!status.pending);
    assert!(h.sent().is_empty());
}

#[test]
fn 台数の枠を超えたら_ログインしたまま枠の印を立てる() {
    let h = Harness::new(|r| match path(r) {
        "/api/auth/token" => Some((200, tokens_json("rt-1", "over-cap", true))),
        "/api/devices" => Some((
            403,
            serde_json::json!({ "success": false, "error": {
                "code": "device/cap-exceeded", "kind": "desktop", "cap": 2, "message": "上限" } })
            .to_string(),
        )),
        _ => None,
    });
    block_on(async {
        let (_, callback) = h.start().await;
        h.tsg
            .finish_login(&callback)
            .await
            .expect("ログイン自体は通る");
    });
    let status = h.tsg.status();
    assert!(status.user.is_some());
    assert!(status.over_cap);
}

#[test]
fn 引換券を断られたら_ログインせず理由を出す() {
    let h = Harness::new(|r| match path(r) {
        "/api/auth/token" => Some((400, error_json("invalid_grant"))),
        _ => None,
    });
    block_on(async {
        let (_, callback) = h.start().await;
        assert!(h.tsg.finish_login(&callback).await.is_err());
    });
    let status = h.tsg.status();
    assert_eq!(status.user, None);
    assert_eq!(status.problem, Some(TsgOneProblem::Relogin));
    assert_eq!(
        status.problem_detail.as_deref(),
        Some("HTTP 400 invalid_grant")
    );
    assert!(h.tsg.secrets.load().expect("読める").is_none());
}

#[test]
fn つながらないときは_つながらないと出す() {
    let h = Harness::new(|_| None);
    block_on(async {
        let (_, callback) = h.start().await;
        assert!(h.tsg.finish_login(&callback).await.is_err());
    });
    assert_eq!(h.tsg.status().problem, Some(TsgOneProblem::Unreachable));
}

#[test]
fn 確かめ直しは期限の切れたトークンを更新し_替わった更新用のトークンを置き直す() {
    let h = Harness::new(happy_core);
    block_on(async {
        let (_, callback) = h.start().await;
        h.tsg.finish_login(&callback).await.expect("ログインできる");
        h.tsg.state().access.as_mut().expect("ある").expires_at = 0;
        h.tsg.state().over_cap = true;
        h.tsg.recheck_device().await.expect("確かめ直せる");
    });
    let sent = h.sent();
    let refresh = &sent[2];
    assert_eq!(path(refresh), "/api/auth/token");
    assert_eq!(body(refresh)["refresh_token"], "rt-1");
    assert_eq!(header(&sent[3], "Authorization"), Some("DPoP at-rt-2"));
    let saved = h.tsg.secrets.load().expect("読める").expect("ある");
    assert_eq!(saved.refresh_token, "rt-2");
    assert_eq!(saved.user.email.as_deref(), Some("taro@example.com"));
    assert!(!h.tsg.status().over_cap);
}

#[test]
fn 更新を断られたら手元のログインを消す() {
    let h = Harness::new(|r| match (path(r), body(r)["grant_type"].as_str()) {
        ("/api/auth/token", Some("authorization_code")) => {
            Some((200, tokens_json("rt-1", "approved", true)))
        }
        ("/api/auth/token", _) => Some((400, error_json("invalid_grant"))),
        _ => Some((200, ok_json())),
    });
    block_on(async {
        let (_, callback) = h.start().await;
        h.tsg.finish_login(&callback).await.expect("ログインできる");
        h.tsg.state().access = None;
        assert!(h.tsg.recheck_device().await.is_err());
    });
    let status = h.tsg.status();
    assert_eq!(status.user, None);
    assert_eq!(status.problem, Some(TsgOneProblem::Relogin));
}

#[test]
fn ログアウトはサーバーで取り消し_失敗しても手元を消す() {
    let h = Harness::new(happy_core);
    block_on(async {
        let (_, callback) = h.start().await;
        h.tsg.finish_login(&callback).await.expect("ログインできる");
        h.tsg.logout().await.expect("ログアウトできる");
    });
    let logout = h.sent().pop().expect("送った");
    assert_eq!(path(&logout), "/api/auth/logout");
    assert_eq!(body(&logout)["refreshToken"], "rt-1");
    assert_eq!(header(&logout, "Authorization"), Some("DPoP at-rt-1"));
    assert_eq!(h.tsg.status().user, None);

    // サーバーにつながらなくても、手元は消える
    let offline = Harness::new(|r| match path(r) {
        "/api/auth/logout" => None,
        _ => happy_core(r),
    });
    block_on(async {
        let (_, callback) = offline.start().await;
        offline
            .tsg
            .finish_login(&callback)
            .await
            .expect("ログインできる");
        offline.tsg.logout().await.expect("手元は消せる");
    });
    assert_eq!(offline.tsg.status().user, None);
    assert!(offline.tsg.secrets.load().expect("読める").is_none());
}

#[test]
fn 金庫の控えの_debug_にトークンを出さない() {
    let saved = SavedLogin {
        refresh_token: "secret-refresh".into(),
        refresh_token_expires_at: 1,
        user: TsgOneUser {
            id: "u".into(),
            email: None,
            display_name: None,
            provider: None,
        },
    };
    assert!(!format!("{saved:?}").contains("secret-refresh"));
}
