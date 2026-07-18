//! SMTP 送信（lettre）。IMAP 同期と同様にブロッキング API を spawn_blocking で回す。
//! TLS は native-tls（Win=SChannel / mac=SecureTransport）で OpenSSL 依存を避ける。

use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
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
    /// 返信元の Message-ID（In-Reply-To。直近の親。山括弧つき/なしどちらでも可）。
    pub in_reply_to: Option<String>,
    /// References チェーン（祖先 Message-ID を空白区切り・古い順。相手メーラーで正しくスレッド
    /// 表示させるため。docs/THREADING.md）。None のときは in_reply_to 単体で代用する。
    pub references: Option<String>,
    /// 自メッセージの Message-ID を明示指定する（山括弧なしの中身）。None なら lettre が自動採番。
    /// 下書きをサーバー Drafts へ APPEND する際、後で同定・削除できるよう固定 ID を使う。
    pub message_id: Option<String>,
    /// 添付（表示名, バイト列, content-type）。空なら添付なし（本文のみ）。
    pub attachments: Vec<(String, Vec<u8>, String)>,
    /// 自分宛メールの検証マーク（X-Rondine-Self ヘッダ値。HMAC。docs/SPAM.md）。None なら付けない。
    pub self_mark: Option<String>,
}

/// 自分宛メールの検証ヘッダ `X-Rondine-Self`（値は Message-ID の HMAC。docs/SPAM.md）。
#[derive(Clone)]
struct XRondineSelf(String);

impl lettre::message::header::Header for XRondineSelf {
    fn name() -> lettre::message::header::HeaderName {
        lettre::message::header::HeaderName::new_from_ascii_str("X-Rondine-Self")
    }
    fn parse(s: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(XRondineSelf(s.to_string()))
    }
    fn display(&self) -> lettre::message::header::HeaderValue {
        lettre::message::header::HeaderValue::new(Self::name(), self.0.clone())
    }
}

/// "名前 <addr>" / "addr" のどちらでも Mailbox に解釈する。
///
/// lettre の `Mailbox` パーサは表示名（phrase）に `@` や `.` などの特殊文字が裸で
/// 含まれると RFC 5322 違反として弾く（例: 表示名がアドレスそのものの
/// `foo@example.com <foo@example.com>`）。そうしたトークンでも送れるよう、直接パースに
/// 失敗したら `<...>` 内をアドレス、その手前を表示名として組み直す。`Mailbox::new` は
/// 表示名を保持し、出力時に必要な引用・エンコードを自前で行うため安全に送れる。
fn parse_mailbox(s: &str) -> Result<Mailbox, String> {
    let trimmed = s.trim();
    let direct_err = match trimmed.parse::<Mailbox>() {
        Ok(mb) => return Ok(mb),
        Err(e) => e,
    };
    // フォールバック: "表示名 <addr>" を手動分解し、アドレス部だけ厳密に検証する。
    // アドレス部が妥当なら表示名を（特殊文字を含んでも）そのまま Mailbox に持たせる。
    if let (Some(open), Some(close)) = (trimmed.rfind('<'), trimmed.rfind('>')) {
        if open < close {
            let addr_part = trimmed[open + 1..close].trim();
            let name_part = trimmed[..open].trim().trim_matches('"').trim();
            if let Ok(addr) = addr_part.parse::<lettre::Address>() {
                let name = (!name_part.is_empty()).then(|| name_part.to_string());
                return Ok(Mailbox::new(name, addr));
            }
        }
    }
    // フォールバックでも解決できないときは元の解釈エラーで通知する。
    Err(format!("宛先を解釈できません（{s}）: {direct_err}"))
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

    // 自メッセージの Message-ID を固定したい場合（下書きのサーバー同期）だけ明示指定する。
    // lettre は山括弧なしの中身を受け取り自前で <...> を付ける。
    if let Some(mid) = msg.message_id.as_ref().filter(|s| !s.trim().is_empty()) {
        let inner = mid.trim().trim_start_matches('<').trim_end_matches('>');
        builder = builder.message_id(Some(inner.to_string()));
    }

    // 自分宛メールの検証マーク（本物の自分からを受信側で判定できるように。docs/SPAM.md）。
    if let Some(mark) = msg.self_mark.as_ref().filter(|s| !s.trim().is_empty()) {
        builder = builder.header(XRondineSelf(mark.clone()));
    }

    for a in &msg.to {
        builder = builder.to(parse_mailbox(a)?);
    }
    for a in &msg.cc {
        builder = builder.cc(parse_mailbox(a)?);
    }
    for a in &msg.bcc {
        builder = builder.bcc(parse_mailbox(a)?);
    }

    // 宛先が 1 件も無いと lettre の build() は Envelope を導出できず MissingTo で失敗する。
    // 下書きのサーバー保存（IMAP APPEND）では宛先ゼロも正当なので、その場合だけ差出人を
    // 封筒の宛先に用いた明示 envelope を与えて組み立てを通す。封筒は .formatted() の生バイトに
    // は現れず（To ヘッダは空のまま）、下書きは SMTP 送信もしないため無害。
    if msg.to.is_empty() && msg.cc.is_empty() && msg.bcc.is_empty() {
        let from_addr = msg
            .from_email
            .trim()
            .parse::<lettre::Address>()
            .map_err(|e| format!("差出人アドレスが不正です（{}）: {e}", msg.from_email))?;
        let envelope = lettre::address::Envelope::new(Some(from_addr.clone()), vec![from_addr])
            .map_err(|e| format!("下書きの封筒を作成できませんでした: {e}"))?;
        builder = builder.envelope(envelope);
    }

    // 返信のスレッド化:
    // - In-Reply-To は直近の親 1 件。
    // - References は祖先チェーン全部（古い順）。相手メーラーで正しく連なる。
    //   references が無ければ in_reply_to 単体で代用する（従来動作）。
    if let Some(id) = msg.in_reply_to.as_ref().filter(|s| !s.trim().is_empty()) {
        builder = builder.in_reply_to(angle_wrap(id));
    }
    let refs_src = msg
        .references
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .or(msg.in_reply_to.as_ref().filter(|s| !s.trim().is_empty()));
    if let Some(refs) = refs_src {
        let chain = refs
            .split_whitespace()
            .map(angle_wrap)
            .collect::<Vec<_>>()
            .join(" ");
        if !chain.is_empty() {
            builder = builder.references(chain);
        }
    }

    // 本文: HTML があれば plain + HTML の multipart/alternative、無ければ plain のみ。
    // 添付があるときは multipart/mixed で本文パートに添付を足す。
    let has_html = msg
        .body_html
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .cloned();
    if msg.attachments.is_empty() {
        match has_html {
            Some(html) => {
                builder.multipart(MultiPart::alternative_plain_html(msg.body_plain.clone(), html))
            }
            None => builder.body(msg.body_plain.clone()),
        }
        .map_err(|e| format!("メッセージの組み立てに失敗しました: {e}"))
    } else {
        // 本文パート（HTML ありは alternative、無しは plain 単体）。
        let mut mixed = match has_html {
            Some(html) => MultiPart::mixed()
                .multipart(MultiPart::alternative_plain_html(msg.body_plain.clone(), html)),
            None => MultiPart::mixed().singlepart(SinglePart::plain(msg.body_plain.clone())),
        };
        // 添付を順に足す。content-type が壊れていても octet-stream で送る。
        let octet = ContentType::parse("application/octet-stream").unwrap();
        for (name, bytes, ct) in &msg.attachments {
            let content_type = ContentType::parse(ct).unwrap_or_else(|_| octet.clone());
            mixed = mixed.singlepart(Attachment::new(name.clone()).body(bytes.clone(), content_type));
        }
        builder
            .multipart(mixed)
            .map_err(|e| format!("メッセージの組み立てに失敗しました: {e}"))
    }
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
    let mailer = builder
        .port(config.port)
        .credentials(creds)
        // 接続・応答の停滞で無限に待たないよう上限を設ける（大きめの添付の送出も見込んで 120 秒）。
        .timeout(Some(std::time::Duration::from_secs(120)))
        .build();

    mailer
        .send(email)
        .map(|_| ())
        .map_err(|e| format!("送信に失敗しました: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 素のアドレスを解釈できる() {
        let mb = parse_mailbox("foo@example.com").expect("素のアドレスは解釈できる");
        assert_eq!(mb.email.to_string(), "foo@example.com");
        assert!(mb.name.is_none());
    }

    #[test]
    fn 通常の表示名つきを解釈できる() {
        let mb = parse_mailbox("山田 太郎 <taro@example.com>").expect("表示名つきは解釈できる");
        assert_eq!(mb.email.to_string(), "taro@example.com");
        assert_eq!(mb.name.as_deref(), Some("山田 太郎"));
    }

    #[test]
    fn 表示名がアドレスそのものでも解釈できる() {
        // lettre の直接パースは弾くが、フォールバックでアドレス部を取り出して組み直す。
        let mb =
            parse_mailbox("foo@example.com <foo@example.com>").expect("フォールバックで解釈できる");
        assert_eq!(mb.email.to_string(), "foo@example.com");
        assert_eq!(mb.name.as_deref(), Some("foo@example.com"));
    }

    #[test]
    fn アドレス部が不正なら元のエラーを返す() {
        let err = parse_mailbox("名前 <not-an-address>").expect_err("不正アドレスはエラー");
        assert!(err.contains("宛先を解釈できません"));
    }
}
