//! 差出人の行が無い「>」だけの引用の差出人を、返信の向きから推定する（docs/THREADING.md §2）。
//!
//! 規則（2026-10-21 利用者と決めた）:
//! - 返信かどうかは返信の印で決める（In-Reply-To / References）。印が無ければ件名の返信の印
//!   （"Re:" / "RE：" / "Re[2]:" / "返信:" / "AW:" / "SV:" / "回复:" 等）で決める。
//! - 件名が転送（"Fwd:" / "FW:" / "転送:" 等）で始まるものは推定しない（「>」は第三者の文のため）。
//! - 差出人を 1 人に決められるときだけ推定する。自分が送った返信なら、宛先（To）の自分以外が
//!   1 人だけのときその人（引用は相手の文）。相手から届いた返信なら、自分（引用は自分が前に送った文）。
//!   ただし、宛先・Cc に自分と差出人以外がいるとき、メーリングリストの印（List-Id）があるときは推定しない。

use super::sender::split_sender;

/// 推定に使うメールの情報（保存済みの列のまま）。
pub struct ReplyContext<'a> {
    pub subject: Option<&'a str>,
    pub in_reply_to: Option<&'a str>,
    pub references: Option<&'a str>,
    pub from_address: Option<&'a str>,
    pub from_name: Option<&'a str>,
    /// 宛先（"名前 <addr>, addr" の形）。
    pub to: Option<&'a str>,
    pub cc: Option<&'a str>,
    pub list_id: Option<&'a str>,
}

/// 件名の返信の印（小文字・半角にそろえて照らす）。
const REPLY_PREFIXES: &[&str] = &["re", "返信", "aw", "sv", "vs", "回复", "答复", "回覆"];

/// 件名の転送の印。
const FORWARD_PREFIXES: &[&str] = &["fwd", "fw", "転送", "转发", "轉寄", "wg", "tr", "rv"];

/// 件名の先頭の印（"Re:" / "RE：" / "Re[2]:" / "Re(2):"）を、小文字・半角にそろえて返す。
/// 先頭のメーリングリストの札（"[ml:123]"）は飛ばす。印が無ければ None。
fn subject_prefix(subject: &str) -> Option<String> {
    let mut s = subject.trim();
    while let Some(rest) = s.strip_prefix('[') {
        s = rest.split_once(']').map_or("", |(_, r)| r).trim_start();
    }
    let (head, _) = s.split_once([':', '：'])?;
    let head: String = head
        .trim()
        .chars()
        .map(|c| match c {
            'Ａ'..='Ｚ' | 'ａ'..='ｚ' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .collect::<String>()
        .to_lowercase();
    // "re[2]" / "re(2)" / "re*2" の回数を外す。
    let head = head
        .split(['[', '(', '*'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    (!head.is_empty() && head.chars().count() <= 4).then_some(head)
}

/// 件名が返信の印で始まるか。
pub fn is_reply_subject(subject: &str) -> bool {
    subject_prefix(subject).is_some_and(|p| REPLY_PREFIXES.contains(&p.as_str()))
}

/// 件名が転送の印で始まるか。
pub fn is_forward_subject(subject: &str) -> bool {
    subject_prefix(subject).is_some_and(|p| FORWARD_PREFIXES.contains(&p.as_str()))
}

/// 宛先の欄（"名前 <addr>, addr"）を、(アドレス, 元の書き方) に分ける。アドレスの無いかけら
/// （名前のカンマで割れた "Kinjo"）は落とす。
fn recipients(list: Option<&str>) -> Vec<(String, &str)> {
    list.into_iter()
        .flat_map(|l| l.split([',', ';']))
        .filter_map(|part| {
            let part = part.trim();
            split_sender(part).0.map(|addr| (addr, part))
        })
        .collect()
}

/// そのメールを受け取った自分のアカウント（受信した返信の推定に使う）。
pub struct OwnAccount<'a> {
    /// アドレス。
    pub address: &'a str,
    /// 表示名（あれば）。
    pub name: Option<&'a str>,
}

/// 差出人の行が無い「>」だけの引用の差出人を推定する。決められなければ None。
/// `self_addrs` は自分のアドレス（小文字。そのアカウントと自分の別アカウント）。
///
/// - 自分が送った返信: 引用は相手の文。宛先（To）の自分以外が 1 人だけなら、その人（宛先の書き方のまま）。
/// - 相手から届いた返信: 引用は自分が前に送った文。推定は自分（宛先に書かれた自分の書き方。無ければ
///   `own` の表示名とアドレス）。宛先・Cc に自分と差出人以外がいるとき、List-Id があるときは推定しない。
pub fn guess_quote_sender(
    ctx: &ReplyContext,
    self_addrs: &[String],
    own: Option<&OwnAccount>,
) -> Option<String> {
    let subject = ctx.subject.unwrap_or_default();
    if is_forward_subject(subject) {
        return None;
    }
    let has_marker = [ctx.in_reply_to, ctx.references]
        .iter()
        .any(|v| v.is_some_and(|s| !s.trim().is_empty()));
    if !has_marker && !is_reply_subject(subject) {
        return None;
    }
    let is_self = |a: &str| self_addrs.iter().any(|s| s.eq_ignore_ascii_case(a));
    let from = ctx
        .from_address
        .map(str::trim)
        .filter(|a| a.contains('@'))?;
    if is_self(from) {
        // 自分の返信: 宛先の自分以外が 1 人だけなら、その人。
        let mut others: Vec<(String, &str)> = Vec::new();
        for (a, written) in recipients(ctx.to) {
            if !is_self(&a) && !others.iter().any(|(o, _)| *o == a) {
                others.push((a, written));
            }
        }
        return match others.as_slice() {
            [(_, written)] => Some((*written).to_string()),
            _ => None,
        };
    }
    // 相手の返信: 引用は自分の前の文。メーリングリスト、ほかの人にも届いているものは推定しない。
    if ctx.list_id.is_some_and(|l| !l.trim().is_empty()) {
        return None;
    }
    let to_cc: Vec<(String, &str)> = recipients(ctx.to)
        .into_iter()
        .chain(recipients(ctx.cc))
        .collect();
    if to_cc
        .iter()
        .any(|(a, _)| !is_self(a) && !a.eq_ignore_ascii_case(from))
    {
        return None;
    }
    // 宛先に書かれた自分の書き方（受けたアカウントのアドレスを優先）。無ければアカウントの表示名とアドレス。
    let own_addr = own.map(|o| o.address.trim());
    let written = to_cc
        .iter()
        .filter(|(a, _)| is_self(a))
        .min_by_key(|(a, _)| own_addr.map_or(true, |o| !a.eq_ignore_ascii_case(o)))
        .map(|(_, w)| (*w).to_string());
    written.or_else(|| {
        let o = own?;
        let addr = o.address.trim();
        addr.contains('@')
            .then(|| match o.name.map(str::trim).filter(|n| !n.is_empty()) {
                Some(name) => format!("{name} <{addr}>"),
                None => addr.to_string(),
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> Vec<String> {
        vec!["me@example.jp".to_string(), "info@example.jp".to_string()]
    }

    fn own() -> OwnAccount<'static> {
        OwnAccount {
            address: "me@example.jp",
            name: Some("末松"),
        }
    }

    fn ctx<'a>(subject: &'a str, from: &'a str, to: &'a str) -> ReplyContext<'a> {
        ReplyContext {
            subject: Some(subject),
            in_reply_to: None,
            references: None,
            from_address: Some(from),
            from_name: None,
            to: Some(to),
            cc: None,
            list_id: None,
        }
    }

    #[test]
    fn reads_reply_and_forward_prefixes() {
        for s in [
            "Re: 件名",
            "RE：件名",
            "Re[2]: x",
            "Re(3): x",
            "返信: x",
            "AW: x",
            "SV: x",
            "回复: x",
            "[ml:12] Re: x",
            "ＲＥ: x",
        ] {
            assert!(is_reply_subject(s), "{s}");
        }
        for s in ["Fwd: x", "FW: x", "転送: x", "[ml] Fwd: x"] {
            assert!(is_forward_subject(s), "{s}");
            assert!(!is_reply_subject(s), "{s}");
        }
        for s in ["件名", "Report: 10月", "お知らせ：休業日"] {
            assert!(!is_reply_subject(s), "{s}");
        }
    }

    /// 自分の返信で、宛先が 1 人ならその人。
    #[test]
    fn own_reply_to_one_person() {
        let c = ctx("Re: 図面", "me@example.jp", "佐藤 一郎 <sato@example.jp>");
        assert_eq!(
            guess_quote_sender(&c, &me(), Some(&own())).as_deref(),
            Some("佐藤 一郎 <sato@example.jp>")
        );
        // 自分の別アカウントが宛先に入っていても数えない。
        let c = ctx(
            "Re: 図面",
            "me@example.jp",
            "sato@example.jp, info@example.jp",
        );
        assert_eq!(
            guess_quote_sender(&c, &me(), Some(&own())).as_deref(),
            Some("sato@example.jp")
        );
    }

    /// 相手から届いた返信の「>」は自分の前の文なので、推定は自分。宛先に書かれた自分の書き方を使い、
    /// 宛先にアドレスだけなら、アカウントの表示名とアドレスを使う。
    #[test]
    fn reply_from_the_other_person_quotes_me() {
        let mut c = ctx("Re: 図面", "sato@example.jp", "末松 慎吾 <Me@example.jp>");
        c.from_name = Some("佐藤 一郎");
        assert_eq!(
            guess_quote_sender(&c, &me(), Some(&own())).as_deref(),
            Some("末松 慎吾 <Me@example.jp>")
        );
        // 宛先がメーリングリストの別名など、自分の書き方が無ければアカウントから。
        let c = ctx("Re: 図面", "sato@example.jp", "");
        assert_eq!(
            guess_quote_sender(&c, &me(), Some(&own())).as_deref(),
            Some("末松 <me@example.jp>")
        );
        // 受けたアカウントの書き方を優先する（自分の別アカウントも宛先にいるとき）。
        let c = ctx(
            "Re: 図面",
            "sato@example.jp",
            "info@example.jp, me@example.jp",
        );
        assert_eq!(
            guess_quote_sender(&c, &me(), Some(&own())).as_deref(),
            Some("me@example.jp")
        );
        // Cc にほかの人がいれば推定しない（相手が 1 人とは限らない）。
        let mut c = ctx("Re: 図面", "sato@example.jp", "me@example.jp");
        c.cc = Some("suzuki@example.jp");
        assert_eq!(guess_quote_sender(&c, &me(), Some(&own())), None);
    }

    /// 件名から "Re:" が消えていても、In-Reply-To があれば返信とみなす。
    #[test]
    fn reply_marker_without_re_in_subject() {
        let mut c = ctx("図面の件", "sato@example.jp", "me@example.jp");
        assert_eq!(guess_quote_sender(&c, &me(), Some(&own())), None);
        c.in_reply_to = Some("<orig@example.jp>");
        assert_eq!(
            guess_quote_sender(&c, &me(), Some(&own())).as_deref(),
            Some("me@example.jp")
        );
    }

    /// 転送は、返信の印があっても推定しない。
    #[test]
    fn forward_is_not_guessed() {
        let mut c = ctx("Fwd: 図面", "sato@example.jp", "me@example.jp");
        c.in_reply_to = Some("<orig@example.jp>");
        assert_eq!(guess_quote_sender(&c, &me(), Some(&own())), None);
    }

    /// 自分の返信で、宛先が複数なら推定しない。
    #[test]
    fn own_reply_to_many_is_not_guessed() {
        let c = ctx(
            "Re: 図面",
            "me@example.jp",
            "sato@example.jp, suzuki@example.jp",
        );
        assert_eq!(guess_quote_sender(&c, &me(), Some(&own())), None);
    }

    /// メーリングリストで、ほかの人にも届いている返信は推定しない。
    #[test]
    fn mailing_list_reply_is_not_guessed() {
        let mut c = ctx("Re: [dev] 質問", "sato@example.jp", "dev@lists.example.org");
        c.list_id = Some("<dev.lists.example.org>");
        assert_eq!(guess_quote_sender(&c, &me(), Some(&own())), None);
    }

    /// 返信の印も件名の返信の印も無い（メールマガジンの中の「>」）は推定しない。
    #[test]
    fn newsletter_quote_is_not_guessed() {
        let c = ctx("木の家だより 10 月号", "news@example.jp", "me@example.jp");
        assert_eq!(guess_quote_sender(&c, &me(), Some(&own())), None);
    }
}
