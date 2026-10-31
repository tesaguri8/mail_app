//! tsg-core へ要求を送る口。tsg-auth は要求を作って応答を読むだけなので、送るのはここ（reqwest）。
//!
//! 試験では [`Transport`] を差し替え、tsg-core へ本物の通信をしない。

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use tsg_auth::HttpRequest;

/// 送れなかった（つながらない・時間切れ等。tsg-core が断ったのではない）。
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("TSG One につながりません: {0}")]
    Http(#[from] reqwest::Error),
    /// 試験で「つながらない」を作る。
    #[cfg(test)]
    #[error("TSG One につながりません（試験）")]
    Simulated,
}

/// 応答（HTTP の状態と本文）。
pub type Response = (u16, String);

/// [`Transport::send`] の戻り。
pub type Sent<'a> = Pin<Box<dyn Future<Output = Result<Response, TransportError>> + Send + 'a>>;

/// tsg-core へ要求を送る。
pub trait Transport: Send + Sync {
    fn send<'a>(&'a self, request: &'a HttpRequest) -> Sent<'a>;
}

/// 1 回の要求を待つ長さ。ログインの画面を固めない。
const TIMEOUT: Duration = Duration::from_secs(20);

/// reqwest で送る。
pub struct HttpTransport {
    client: reqwest::Client,
}

impl HttpTransport {
    /// 時間切れを付けた接続を作る。
    ///
    /// # Errors
    /// TLS の初期化に失敗すると [`TransportError`]。
    pub fn new() -> Result<HttpTransport, TransportError> {
        let client = reqwest::Client::builder().timeout(TIMEOUT).build()?;
        Ok(HttpTransport { client })
    }
}

impl Transport for HttpTransport {
    fn send<'a>(&'a self, request: &'a HttpRequest) -> Sent<'a> {
        Box::pin(async move {
            let method = reqwest::Method::from_bytes(request.method.as_bytes())
                .unwrap_or(reqwest::Method::GET);
            let mut builder = self.client.request(method, &request.url);
            for (name, value) in &request.headers {
                builder = builder.header(*name, value);
            }
            if !request.body.is_empty() {
                builder = builder.body(request.body.clone());
            }
            let response = builder.send().await?;
            let status = response.status().as_u16();
            Ok((status, response.text().await?))
        })
    }
}
