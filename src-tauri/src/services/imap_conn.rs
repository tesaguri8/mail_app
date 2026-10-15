//! IMAP への接続（TLS・時間の上限・受け取ったバイト数）。
//!
//! 接続はすべてここを通す。サーバーが黙っても、少しずつしか送ってこなくても、
//! `spawn_blocking` の中で永久に待たないよう、次の 3 つの上限を持つ。
//!
//! - TCP の接続の上限（[`ImapTimeouts::connect`]）
//! - 読み書き 1 回ごとの上限（[`ImapTimeouts::io`]）— 黙ったサーバー
//! - 全体の締め切り（[`ImapTimeouts::total`]）— 少しずつ送り続けるサーバー

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// IMAP のセッション（接続の使い回し／プール用にも使う）。
pub type ImapSession = imap::Session<native_tls::TlsStream<GuardedStream>>;
/// ログイン前の IMAP クライアント。
pub type ImapClient = imap::Client<native_tls::TlsStream<GuardedStream>>;

/// IMAP 接続の時間の上限。
#[derive(Debug, Clone, Copy)]
pub struct ImapTimeouts {
    /// TCP の接続の上限。
    pub connect: Duration,
    /// 読み書き 1 回ごとの上限（TLS のハンドシェイク・挨拶・ログイン・取得のすべて）。
    pub io: Duration,
    /// 接続してからの全体の締め切り（None なら無し）。使い回す同期の接続には付けない。
    pub total: Option<Duration>,
}

impl ImapTimeouts {
    /// 既定の上限（接続 20 秒・読み書き 60 秒・締め切り無し）。同期の持続接続用。
    pub const DEFAULT: Self = Self {
        connect: Duration::from_secs(20),
        io: Duration::from_secs(60),
        total: None,
    };
    /// 添付の取得の上限（既定に、全体で 3 分の締め切りを足す）。
    pub const ATTACHMENT: Self = Self {
        total: Some(Duration::from_secs(180)),
        ..Self::DEFAULT
    };
}

/// IMAP の接続・操作の失敗。時間切れだけは利用者に分かる文言を出すため、別に分ける。
#[derive(Debug, thiserror::Error)]
pub enum ImapError {
    /// サーバーが上限の時間内に応答しなかった（または締め切りまでに終わらなかった）。
    #[error("サーバーが応答しませんでした")]
    Timeout,
    /// ホスト名を解決できなかった。
    #[error("名前解決に失敗: {0}")]
    Resolve(String),
    /// TCP で接続できなかった。
    #[error("接続できませんでした: {0}")]
    Connect(std::io::Error),
    /// TLS のハンドシェイクに失敗した。
    #[error("TLS ハンドシェイクに失敗: {0}")]
    Tls(String),
    /// IMAP のやり取りに失敗した（サーバーの NO/BAD・切断など）。
    #[error("{0}")]
    Imap(imap::Error),
    /// 応答の中身が想定と違った（メッセージ・パートが無い等）。
    #[error("{0}")]
    Other(String),
}

/// 読み書きの時間切れか（Unix は WouldBlock、Windows は TimedOut で返る）。
fn is_timeout(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

impl From<imap::Error> for ImapError {
    fn from(e: imap::Error) -> Self {
        match e {
            imap::Error::Io(io) if is_timeout(&io) => Self::Timeout,
            other => Self::Imap(other),
        }
    }
}

impl From<native_tls::HandshakeError<GuardedStream>> for ImapError {
    fn from(e: native_tls::HandshakeError<GuardedStream>) -> Self {
        match e {
            // 読み込みの上限に達すると、ハンドシェイクが途中のまま WouldBlock で戻る。
            native_tls::HandshakeError::WouldBlock(_) => Self::Timeout,
            native_tls::HandshakeError::Failure(f) => {
                let timed_out =
                    std::iter::successors(std::error::Error::source(&f), |s| s.source())
                        .any(|s| s.downcast_ref::<std::io::Error>().is_some_and(is_timeout));
                if timed_out {
                    Self::Timeout
                } else {
                    Self::Tls(f.to_string())
                }
            }
        }
    }
}

/// 読み書きの上限と全体の締め切りを持つ TCP の流れ。
///
/// 読み書き 1 回ごとに、残り時間と 1 回の上限の短い方をソケットの上限に入れ直す。
/// 締め切りを過ぎたら `TimedOut` を返すので、少しずつ届き続ける応答でも必ず終わる。
#[derive(Debug)]
pub struct GuardedStream {
    tcp: TcpStream,
    io: Duration,
    deadline: Option<Instant>,
}

impl GuardedStream {
    /// `deadline` を過ぎたら読み書きを `TimedOut` で断る（None なら 1 回ごとの上限だけ）。
    pub fn new(tcp: TcpStream, io: Duration, deadline: Option<Instant>) -> Self {
        Self { tcp, io, deadline }
    }

    /// 今回の読み書きに使える時間（締め切りを過ぎていたら `TimedOut`）。
    fn budget(&self) -> std::io::Result<Duration> {
        match self.deadline {
            None => Ok(self.io),
            Some(d) => {
                let left = d.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "締め切りを過ぎました",
                    ))
                } else {
                    Ok(left.min(self.io))
                }
            }
        }
    }
}

impl Read for GuardedStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let budget = self.budget()?;
        if self.deadline.is_some() {
            self.tcp.set_read_timeout(Some(budget))?;
        }
        self.tcp.read(buf)
    }
}

impl Write for GuardedStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let budget = self.budget()?;
        if self.deadline.is_some() {
            self.tcp.set_write_timeout(Some(budget))?;
        }
        self.tcp.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.tcp.flush()
    }
}

/// 上限付きで IMAP に接続し、挨拶まで読んだクライアントを返す（ログインはまだ）。
///
/// # Errors
/// 名前解決・接続・TLS・挨拶のいずれかに失敗したとき。上限を過ぎたら [`ImapError::Timeout`]。
pub fn connect_client(
    host: &str,
    port: u16,
    timeouts: ImapTimeouts,
) -> Result<ImapClient, ImapError> {
    let deadline = timeouts.total.map(|t| Instant::now() + t);
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| ImapError::Tls(e.to_string()))?;
    let addr = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|e| ImapError::Resolve(e.to_string()))?
        .next()
        .ok_or_else(|| ImapError::Resolve("アドレスを解決できませんでした".to_string()))?;
    let connect = timeouts
        .total
        .map_or(timeouts.connect, |t| t.min(timeouts.connect));
    let tcp = TcpStream::connect_timeout(&addr, connect).map_err(|e| {
        if is_timeout(&e) {
            ImapError::Timeout
        } else {
            ImapError::Connect(e)
        }
    })?;
    tcp.set_read_timeout(Some(timeouts.io))
        .map_err(ImapError::Connect)?;
    tcp.set_write_timeout(Some(timeouts.io))
        .map_err(ImapError::Connect)?;
    let stream = GuardedStream::new(tcp, timeouts.io, deadline);
    let ssl = tls.connect(host, stream)?;
    let mut client = imap::Client::new(ssl);
    client.read_greeting()?;
    Ok(client)
}

/// 上限付きで IMAP に接続＋ログインし、Session を返す（[`ImapTimeouts`] で上限を差し替えられる）。
///
/// # Errors
/// 名前解決・接続・TLS・挨拶・ログインのいずれかに失敗したとき。上限を過ぎたら [`ImapError::Timeout`]。
pub fn connect_login_with(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    timeouts: ImapTimeouts,
) -> Result<ImapSession, ImapError> {
    let client = connect_client(host, port, timeouts)?;
    client.login(user, password).map_err(|(e, _)| e.into())
}

/// 既定の上限で IMAP に接続＋ログインし、Session を返す（接続の使い回し／プール用）。
///
/// # Errors
/// [`connect_login_with`] と同じ（文字列にして返す）。
pub fn connect_login(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
) -> Result<ImapSession, String> {
    connect_login_with(host, port, user, password, ImapTimeouts::DEFAULT)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
