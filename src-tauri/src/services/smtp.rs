//! SMTP 送信（lettre）。IMAP 同期と同様にブロッキング API を spawn_blocking で回す。
//! TLS は native-tls（Win=SChannel / mac=SecureTransport）で OpenSSL 依存を避ける。

use lettre::message::{Mailbox, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

/// 送信サーバーの接続・認証情報。
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    /// 'ssl'（実装 TLS・通常465）| 'starttls'（通常587）| その他（平文・非推奨）。
    pub security: String,
    pub user: String,
    pub password: String,
}

/// 送信する 1 通の内容。
pub struct OutgoingMessage {
    /// 差出人の表示名（任意）。
    pub from_name: Option<String>,
    /// 差出人アドレス（From:／エンベロープ）。
    pub from_email: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    /// プレーン本文（必須。作成はプレーンで行う）。
    pub body_plain: String,
    /// HTML 本文（あれば multipart/alternative で同梱）。
    pub body_html: Option<String>,
    /// 返信元の Message-ID（In-Reply-To／References。スレッド用。山括弧つき/なしどちらでも可）。
    pub in_reply_to: Option<String>,
}

/// "名前 <addr>" / "addr" のどちらでも Mailbox に解釈する。
fn parse_mailbox(s: &str) -> Result<Mailbox, String> {
    s.trim()
        .parse::<Mailbox>()
        .map_err(|e| format!("宛先を解釈できません（{s}）: {e}"))
}

/// Message-ID を山括弧つきの形（<id@host>）へ正規化する。
fn angle_wrap(id: &str) -> String {
    let t = id.trim();
    if t.starts_with('<') && t.ends_with('>') {
        t.to_string()
    } else {
        format!("<{t}>")
    }
}

/// OutgoingMessage から lettre の Message を組み立てる（SMTP 送信と Sent 保存で共有）。
pub fn build_message(msg: &OutgoingMessage) -> Result<Message, String> {
    let from = {
        let addr = msg
            .from_email
            .trim()
            .parse::<lettre::Address>()
            .map_err(|e| format!("差出人アドレスが不正です（{}）: {e}", msg.from_email))?;
        Mailbox::new(msg.from_name.clone().filter(|s| !s.trim().is_empty()), addr)
    };

    let mut builder = Message::builder().from(from).subject(msg.subject.clone());

    for a in &msg.to {
        builder = builder.to(parse_mailbox(a)?);
    }
    for a in &msg.cc {
        builder = builder.cc(parse_mailbox(a)?);
    }
    for a in &msg.bcc {
        builder = builder.bcc(parse_mailbox(a)?);
    }

    // 返信のスレッド化: In-Reply-To と References に元メッセージの Message-ID を入れる。
    if let Some(id) = msg.in_reply_to.as_ref().filter(|s| !s.trim().is_empty()) {
        let wrapped = angle_wrap(id);
        builder = builder.in_reply_to(wrapped.clone()).references(wrapped);
    }

    // 本文: HTML があれば plain + HTML の multipart/alternative、無ければ plain のみ。
    match msg.body_html.as_ref().filter(|s| !s.trim().is_empty()) {
        Some(html) => builder.multipart(MultiPart::alternative_plain_html(
            msg.body_plain.clone(),
            html.clone(),
        )),
        None => builder.body(msg.body_plain.clone()),
    }
    .map_err(|e| format!("メッセージの組み立てに失敗しました: {e}"))
}

/// 組み立て済みメッセージを SMTP で送信する。成功なら Ok(())。
pub fn send(config: &SmtpConfig, email: &Message) -> Result<(), String> {
    let creds = Credentials::new(config.user.clone(), config.password.clone());
    let host = config.host.as_str();
    let builder = match config.security.as_str() {
        // 実装 TLS（接続直後から TLS。通常 465）。
        "ssl" | "tls" | "ssl/tls" => SmtpTransport::relay(host).map_err(|e| e.to_string())?,
        // STARTTLS（平文で接続後に TLS へ昇格。通常 587）。
        "starttls" => SmtpTransport::starttls_relay(host).map_err(|e| e.to_string())?,
        // 平文（非推奨。テスト用途など）。
        _ => SmtpTransport::builder_dangerous(host),
    };
    let mailer = builder.port(config.port).credentials(creds).build();

    mailer
        .send(email)
        .map(|_| ())
        .map_err(|e| format!("送信に失敗しました: {e}"))
}
