//! デスクトップ用 OAuth 2.0（ループバック + PKCE）。
//!
//! フロー: ローカルの 127.0.0.1:任意ポートで待ち受け → ブラウザで同意 → リダイレクトで
//! 受け取った認可コードを PKCE の code_verifier とともにトークンへ交換する。
//! Google の「デスクトップアプリ」種別クライアントはループバックリダイレクトを許可する。

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

/// 取得したトークン一式。
#[derive(Debug, Clone)]
pub struct TokenSet {
    pub access_token: String,
    /// 初回同意時のみ返る。再取得できないため keyring に保存して使い回す。
    pub refresh_token: Option<String>,
    /// 実際に許可されたスコープ（スペース区切り）。ユーザーが一部だけ許可することがあるため
    /// 要求と一致するとは限らない。後から別サービスを有効化する際の再同意判定に使う。
    pub granted_scopes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    sub: Option<String>,
}

/// URL セーフな乱数文字列（PKCE verifier / state 用）。
fn random_urlsafe(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).map_err(|e| format!("乱数を生成できません: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(&buf))
}

/// PKCE の (code_verifier, code_challenge[S256]) を作る。
fn make_pkce() -> Result<(String, String), String> {
    let verifier = random_urlsafe(48)?; // 64 文字程度（RFC 43〜128）
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());
    Ok((verifier, challenge))
}

/// 認可フローを実行し、トークンと連携アカウントのメールアドレスを返す。
///
/// `scope` は `super::scopes()` で組み立てたスペース区切りの要求スコープ。既に別サービスで
/// 連携済みのアカウントを追加同意させる場合も、この関数に新しいスコープ集合を渡せばよい
/// （`include_granted_scopes` により既存の許可は失われない）。
///
/// `login_hint` はアカウント選択画面で先に選んでおくアドレス（カードのアドレス）。利用者は
/// 別のアカウントも選べるので、戻ったアドレスを正とする。
pub async fn run_flow(
    app: &AppHandle,
    client_id: &str,
    client_secret: &str,
    scope: &str,
    login_hint: Option<&str>,
) -> Result<(TokenSet, String), String> {
    // 1) ループバックの待受を確保（ポートは OS 任せ）。
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("ローカルポートを開けません: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}");

    // 2) PKCE と state を用意し、認可 URL を組み立てる。
    let (verifier, challenge) = make_pkce()?;
    let state = random_urlsafe(24)?;
    let mut auth_url = reqwest::Url::parse_with_params(
        super::AUTH_ENDPOINT,
        &[
            ("client_id", client_id),
            ("redirect_uri", &redirect_uri),
            ("response_type", "code"),
            ("scope", scope),
            ("access_type", "offline"),
            ("prompt", "consent"),
            // 追加同意でも既存の許可を失わない（カレンダー連携済みに連絡先を足す場合など）。
            ("include_granted_scopes", "true"),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
        ],
    )
    .map_err(|e| format!("認可 URL を作成できません: {e}"))?;
    if let Some(hint) = login_hint.map(str::trim).filter(|h| !h.is_empty()) {
        auth_url.query_pairs_mut().append_pair("login_hint", hint);
    }

    // 3) 既定ブラウザで同意画面を開く。
    app.opener()
        .open_url(auth_url.to_string(), None::<&str>)
        .map_err(|e| format!("ブラウザを開けません: {e}"))?;

    // 4) リダイレクト（認可コード）をブロッキングで待つ（5 分でタイムアウト）。
    let expected_state = state.clone();
    let code = tauri::async_runtime::spawn_blocking(move || wait_for_code(listener, &expected_state))
        .await
        .map_err(|e| format!("待受タスクに失敗: {e}"))??;

    // 5) 認可コード → トークン交換。
    let client = super::http_client()?;
    let tokens = exchange_code(&client, client_id, client_secret, &code, &redirect_uri, &verifier)
        .await?;

    // 6) 連携アカウントのメールアドレスを取得。
    let email = fetch_email(&client, &tokens.access_token).await?;
    Ok((tokens, email))
}

/// refresh_token でアクセストークンを更新する（同期のたびに呼ぶ）。
pub async fn refresh_access_token(
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<String, String> {
    let client = super::http_client()?;
    let params = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];
    let resp = client
        .post(super::TOKEN_ENDPOINT)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("トークン更新に失敗: {e}"))?;
    let status = resp.status();
    let body: TokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("トークン応答を解析できません: {e}"))?;
    if !status.is_success() || body.access_token.is_none() {
        let msg = body
            .error_description
            .or(body.error)
            .unwrap_or_else(|| "不明なエラー".into());
        // testing 公開状態の refresh_token は約 7 日で失効する。再連携を促す。
        return Err(format!(
            "アクセストークンを更新できませんでした（再連携が必要な場合があります）: {msg}"
        ));
    }
    Ok(body.access_token.unwrap())
}

/// Google 側の許可を取り消す（refresh token を送ると、その連携の許可が無効になる）。
///
/// 「完全に解除」で使う。取り消すと、同じ Google アカウント・同じ OAuth クライアントで
/// 発行したほかのトークン（別の端末の連携など）も無効になりうる。
///
/// # Errors
/// 送れなかった、または Google が拒んだとき（理由の文言つき）。
pub async fn revoke_token(token: &str) -> Result<(), String> {
    revoke_token_at(&super::http_client()?, super::REVOKE_ENDPOINT, token).await
}

/// [`revoke_token`] の本体（送り先を差し替えて試験するため分けてある）。
async fn revoke_token_at(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
) -> Result<(), String> {
    let resp = client
        .post(endpoint)
        .form(&[("token", token)])
        .send()
        .await
        .map_err(|e| format!("許可の取り消しを送れませんでした: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    // 失効済みのトークンは 400 invalid_token が返る。理由はそのまま見せる。
    let body = resp.text().await.unwrap_or_default();
    Err(format!(
        "Google が許可の取り消しを受け付けませんでした（{status}）: {body}"
    ))
}

async fn exchange_code(
    client: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<TokenSet, String> {
    let params = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("code", code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
    ];
    let resp = client
        .post(super::TOKEN_ENDPOINT)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("トークン交換に失敗: {e}"))?;
    let status = resp.status();
    let body: TokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("トークン応答を解析できません: {e}"))?;
    if !status.is_success() || body.access_token.is_none() {
        let msg = body
            .error_description
            .or(body.error)
            .unwrap_or_else(|| "不明なエラー".into());
        return Err(format!("トークンを取得できませんでした: {msg}"));
    }
    Ok(TokenSet {
        access_token: body.access_token.unwrap(),
        refresh_token: body.refresh_token,
        granted_scopes: body.scope,
    })
}

async fn fetch_email(client: &reqwest::Client, access_token: &str) -> Result<String, String> {
    let resp = client
        .get(super::USERINFO_ENDPOINT)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("アカウント情報の取得に失敗: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "アカウント情報を取得できませんでした（HTTP {}）",
            resp.status().as_u16()
        ));
    }
    let info: UserInfo = resp
        .json()
        .await
        .map_err(|e| format!("アカウント情報を解析できません: {e}"))?;
    info.email
        .or(info.sub)
        .ok_or_else(|| "メールアドレスを取得できませんでした".into())
}

/// ループバックへのリダイレクトを 1 件受け取り、認可コードを取り出す。
/// state を検証し、ブラウザには「閉じてよい」旨の簡易ページを返す。
fn wait_for_code(listener: TcpListener, expected_state: &str) -> Result<String, String> {
    listener
        .set_nonblocking(true)
        .map_err(|e| e.to_string())?;
    // 同意（未確認アプリの警告クリック等）に時間がかかっても取りこぼさないよう長めに。
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .ok();
                // リクエスト行（ヘッダ終端まで）を読む。
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            data.extend_from_slice(&buf[..n]);
                            if data.windows(4).any(|w| w == b"\r\n\r\n") || data.len() > 16384 {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let req = String::from_utf8_lossy(&data);
                let path = req
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("/");
                // パス解析に失敗しても中止せず、空リクエスト扱いにして待受を続ける。
                let url = reqwest::Url::parse(&format!("http://127.0.0.1{path}"))
                    .unwrap_or_else(|_| reqwest::Url::parse("http://127.0.0.1/").unwrap());
                let (mut code, mut got_state, mut err) = (None, None, None);
                for (k, v) in url.query_pairs() {
                    match k.as_ref() {
                        "code" => code = Some(v.into_owned()),
                        "state" => got_state = Some(v.into_owned()),
                        "error" => err = Some(v.into_owned()),
                        _ => {}
                    }
                }

                // code も error も無い接続（ブラウザの先読み・favicon 等）は認可リダイレクトでは
                // ないので、204 を返して閉じ、待受は継続する（return しない）。これをしないと
                // 先読み接続を誤判定して state 不一致で中止し、本命が接続拒否になる。
                if code.is_none() && err.is_none() {
                    let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
                    let _ = stream.flush();
                    continue;
                }

                let page = "<!doctype html><html lang=\"ja\"><meta charset=\"utf-8\">\
                    <title>Rondine</title><body style=\"font-family:sans-serif;text-align:center;\
                    padding-top:3rem;color:#333\"><h2>認証が完了しました</h2>\
                    <p>この画面を閉じて Rondine に戻ってください。</p></body></html>";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    page.len(),
                    page
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();

                if let Some(e) = err {
                    return Err(format!("認証が拒否されました: {e}"));
                }
                if got_state.as_deref() != Some(expected_state) {
                    return Err("state が一致しません（安全のため中止しました）".into());
                }
                return code.ok_or_else(|| "認可コードを受け取れませんでした".into());
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Err("認証がタイムアウトしました（10 分）。もう一度お試しください".into());
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            Err(e) => return Err(format!("待受でエラー: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// 1 回だけ応える試験用の HTTP サーバー。受け取った要求の全文を返す。
    fn one_shot_server(
        status_line: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/revoke", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 4096];
            let mut req = String::new();
            // 頭と本文（form）が揃うまで読む。
            while !req.contains("token=") {
                let n = sock.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                req.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
            let resp = format!(
                "{status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(resp.as_bytes()).unwrap();
            req
        });
        (url, handle)
    }

    #[test]
    fn revoke_posts_the_token_as_a_form() {
        let (url, server) = one_shot_server("HTTP/1.1 200 OK", "");
        let client = reqwest::Client::new();
        let r = tauri::async_runtime::block_on(revoke_token_at(&client, &url, "1//refresh-abc"));
        assert!(r.is_ok(), "{r:?}");
        let req = server.join().unwrap();
        assert!(req.starts_with("POST /revoke"));
        assert!(req.contains("application/x-www-form-urlencoded"));
        assert!(req.contains("token=1%2F%2Frefresh-abc"));
    }

    #[test]
    fn revoke_reports_a_refusal() {
        let (url, server) =
            one_shot_server("HTTP/1.1 400 Bad Request", r#"{"error":"invalid_token"}"#);
        let client = reqwest::Client::new();
        let r = tauri::async_runtime::block_on(revoke_token_at(&client, &url, "expired"));
        server.join().unwrap();
        let e = r.unwrap_err();
        assert!(e.contains("400") && e.contains("invalid_token"), "{e}");
    }
}
