//! 会話の中で同じ「手元に無いメールの引用」（未受信の枠）を 1 回だけ出す（docs/THREADING.md §5）。
//!
//! 未受信の枠はメール 1 通ごとに本文から拾うので、返信が同じ元メールを引用し続けると、
//! 返信のたびに同じ枠が付いていた（2026-11-11 vaio: 16 通の会話の各返信の上に同じ 1 通が出て、
//! 日付のつまみが 9/24 → 10/5 → 9/24 → 10/6 → … と行き来した）。
//!
//! 同じとみなすのは、**差出人と日時の両方が読めた枠**どうしで、差出人（アドレス、無ければ名前）と
//! 読んだ日時が一致し、本文（正規化後）が一致するか一方が他方を含むものだけ。残すのは最初に出た
//! （いちばん古いメールの）枠で、時間の流れを崩さない。本文が長いほうの中身はその枠に移す
//! （後の返信の側で、続きの引用が足されていることがあるため）。
//!
//! 差出人を推定した枠（`guessed`）と日時の無い枠は、同じかを確かめる手がかりが足りないので、
//! まとめずにそのまま残す。

use super::quote_match::{normalize_body, MIN_KNOWN_CHARS};
use crate::models::{ThreadMessage, UnseenQuote};
use crate::services::quotes::sender::split_sender;

/// 枠が同じかを比べる鍵（差出人・読んだ日時）。比べられない枠は None。
#[derive(Debug, PartialEq, Eq)]
struct Key {
    sender: String,
    date: String,
}

fn key_of(q: &UnseenQuote) -> Option<Key> {
    if q.guessed {
        return None;
    }
    let date = q.date.clone()?;
    let sender = match split_sender(&q.from) {
        (Some(addr), _) => addr,
        (None, Some(name)) => name.split_whitespace().collect::<String>().to_lowercase(),
        (None, None) => return None,
    };
    Some(Key { sender, date })
}

/// 正規化した本文が同じ内容か（一致、または短いほうが十分長く、長いほうに含まれる）。
fn same_body(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    short.chars().count() >= MIN_KNOWN_CHARS && long.contains(short)
}

/// 既に出した枠（どのメールの何番目か・鍵・正規化した本文）。
struct Shown {
    message: usize,
    index: usize,
    key: Key,
    norm: String,
}

/// 会話のメール（古い順）の未受信の枠から、前のメールで既に出した枠と同じものを除く。
pub fn drop_repeated_unseen_quotes(messages: &mut [ThreadMessage]) {
    let mut lists: Vec<Vec<UnseenQuote>> = messages
        .iter_mut()
        .map(|m| std::mem::take(&mut m.unseen_quotes))
        .collect();
    dedupe(&mut lists);
    for (m, list) in messages.iter_mut().zip(lists) {
        m.unseen_quotes = list;
    }
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
            let norm = normalize_body(&q.body);
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

#[cfg(test)]
mod tests;
