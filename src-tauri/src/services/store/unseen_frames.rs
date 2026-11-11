//! 会話の「手元に無いメールの引用」（未受信の枠）を、会話全体で 1 つの集合として並べ直す
//! （docs/THREADING.md §5）。
//!
//! 未受信の枠はメール 1 通ごとに本文から拾うので、返信が同じ元メールを引用し続けると、
//! 返信のたびに同じ枠が付いていた（2026-11-11 vaio: 16 通の会話の各返信の上に同じ 1 通が出て、
//! 日付のつまみが 9/24 → 10/5 → 9/24 → 10/6 → … と行き来した）。ここでは次の 2 つをする。
//!
//! 1. **同じ枠を 1 つにまとめる**: 差出人と日時の両方が読めた枠どうしで、差出人（アドレス、無ければ
//!    名前）・読んだ日時（分単位）が一致し、本文（正規化・全角半角をそろえたもの）が一致するか、十分長い
//!    一方が他方を含むものだけ。迷うもの（時差が片方にだけ書かれている など）は別のものにする
//!    （誤ってまとめると内容が消えるが、まとめ損ねても重なって見えるだけ）。本文が長いほうの中身を残す
//!    （後の返信の側で、続きの引用が足されていることがあるため）。
//! 2. **日時のある枠を、自分の日時の位置に置く**: 会話（日時順）の中で、枠の日時以後の最初のメールの
//!    上に移す。含んでいたメールは枠より新しいので、移す先はそのメールか、それより前になる。
//!    遡った返信や枝分かれがあっても、会話の並びが日時順なので崩れない。
//!
//! 差出人を推定した枠（`guessed`）と日時の無い枠は、時系列に置けず、同じかを確かめる手がかりも
//! 足りないので、まとめずに含んでいたメールの上に残す。

use chrono::{DateTime, NaiveDateTime, Timelike};

use super::quote_match::{normalize_body, MIN_KNOWN_CHARS};
use crate::models::{ThreadMessage, UnseenQuote};
use crate::services::quotes::sender::split_sender;

/// 枠や会話のメールの日時（分単位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum At {
    /// 時差が書かれていた: UTC の分（UNIX 時刻 / 60）。
    Utc(i64),
    /// 時差が書かれていなかった: 書かれたままの現地時刻（秒は切り捨て）。
    Local(NaiveDateTime),
}

/// 枠の日時（`UnseenQuote.date`。`QuotedTime::to_iso` の形）を読む。
fn frame_at(date: &str) -> Option<At> {
    if let Ok(t) = DateTime::parse_from_rfc3339(date) {
        return Some(At::Utc(t.timestamp().div_euclid(60)));
    }
    NaiveDateTime::parse_from_str(date, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .and_then(|t| t.with_second(0))
        .map(At::Local)
}

/// 会話のメールの日時（RFC 3339。差出人の時差付き）。比べる枠の形に合わせて両方を持つ。
#[derive(Debug, Clone, Copy)]
struct MessageAt {
    utc_minute: i64,
    local: NaiveDateTime,
}

fn message_at(date: Option<&str>) -> Option<MessageAt> {
    let t = DateTime::parse_from_rfc3339(date?).ok()?;
    Some(MessageAt {
        utc_minute: t.timestamp().div_euclid(60),
        local: t.naive_local().with_second(0)?,
    })
}

impl MessageAt {
    /// このメールが、枠の日時と同じ分か、それより後か。時差の無い枠は、メールの差出人の現地時刻と比べる
    /// （引用の見出しの時刻は、たいてい引用された側の現地時刻で、会話の相手どうしは同じ時差のことが多い。
    /// `quote_match::time_gap` と同じ考え方）。
    fn is_at_or_after(&self, at: At) -> bool {
        match at {
            At::Utc(m) => self.utc_minute >= m,
            At::Local(t) => self.local >= t,
        }
    }
}

/// 枠が同じかを比べる鍵（差出人・読んだ日時）。比べられない枠は None。
#[derive(Debug, PartialEq, Eq)]
struct Key {
    sender: String,
    at: At,
}

fn key_of(q: &UnseenQuote) -> Option<Key> {
    if q.guessed {
        return None;
    }
    let at = frame_at(q.date.as_deref()?)?;
    let sender = match split_sender(&q.from) {
        (Some(addr), _) => addr,
        (None, Some(name)) => fold_width(&name)
            .split_whitespace()
            .collect::<String>()
            .to_lowercase(),
        (None, None) => return None,
    };
    Some(Key { sender, at })
}

/// 全角の英数・記号・空白を半角にそろえる（引用の段ごとに、メールソフトが書き換えることがある）。
fn fold_width(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{3000}' => ' ',
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(u32::from(c) - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// 比べる形の本文（行頭の `>`・改行・空白を落とし、全角半角をそろえる）。
fn comparable_body(body: &str) -> String {
    normalize_body(&fold_width(body))
}

/// 正規化した本文が同じ内容か（一致、または短いほうが十分長く、長いほうに含まれる）。
fn same_body(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    short.chars().count() >= MIN_KNOWN_CHARS && long.contains(short)
}

/// 会話のメール（日時順）の未受信の枠を並べ直す: 同じ枠を 1 つにまとめ、日時のある枠を自分の日時の
/// 位置へ移す。
pub fn arrange(messages: &mut [ThreadMessage]) {
    let times: Vec<Option<MessageAt>> = messages
        .iter()
        .map(|m| message_at(m.date.as_deref()))
        .collect();
    let mut lists: Vec<Vec<UnseenQuote>> = messages
        .iter_mut()
        .map(|m| std::mem::take(&mut m.unseen_quotes))
        .collect();
    dedupe(&mut lists);
    place(&mut lists, &times);
    for (m, list) in messages.iter_mut().zip(lists) {
        m.unseen_quotes = list;
    }
}

/// 既に出した枠（どのメールの何番目か・鍵・比べる形の本文）。
struct Shown {
    message: usize,
    index: usize,
    key: Key,
    norm: String,
}

/// メールごとの枠の並び（古いメールから順）から、前に出した枠と同じものを除く。
fn dedupe(lists: &mut [Vec<UnseenQuote>]) {
    let mut shown: Vec<Shown> = Vec::new();
    for i in 0..lists.len() {
        let quotes = std::mem::take(&mut lists[i]);
        let mut kept: Vec<UnseenQuote> = Vec::with_capacity(quotes.len());
        for q in quotes {
            let Some(key) = key_of(&q) else {
                kept.push(q);
                continue;
            };
            let norm = comparable_body(&q.body);
            match shown
                .iter_mut()
                .find(|s| s.key == key && same_body(&s.norm, &norm))
            {
                Some(s) => {
                    // 後の枠のほうが中身が多ければ、残す枠の本文を差し替える。
                    if norm.len() > s.norm.len() {
                        let target = if s.message == i {
                            kept.get_mut(s.index)
                        } else {
                            lists[s.message].get_mut(s.index)
                        };
                        if let Some(t) = target {
                            t.body = q.body;
                            s.norm = norm;
                        }
                    }
                }
                None => {
                    shown.push(Shown {
                        message: i,
                        index: kept.len(),
                        key,
                        norm,
                    });
                    kept.push(q);
                }
            }
        }
        lists[i] = kept;
    }
}

/// 日時のある枠を、その日時以後の最初のメール（含んでいたメールまで）の上へ移す。
/// 移した枠は、移した先の枠の後ろに足す（並びは画面側で日時順にする）。
fn place(lists: &mut [Vec<UnseenQuote>], times: &[Option<MessageAt>]) {
    for i in 0..lists.len() {
        let quotes = std::mem::take(&mut lists[i]);
        let mut stay: Vec<UnseenQuote> = Vec::with_capacity(quotes.len());
        for q in quotes {
            let target = (!q.guessed)
                .then(|| q.date.as_deref().and_then(frame_at))
                .flatten()
                .and_then(|at| (0..i).find(|&j| times[j].is_some_and(|t| t.is_at_or_after(at))));
            match target {
                Some(j) => lists[j].push(q),
                None => stay.push(q),
            }
        }
        // 移す先は前のメール（j < i）だけなので、このメールの並びは残る枠だけになる。
        lists[i] = stay;
    }
}

#[cfg(test)]
mod tests;
