//! プレーン本文から「新しく書かれた部分（clean_body）」と「引用ブロック」を分離する。
//! docs/THREADING.md §7（引用・署名の分離）の最小実装。regex 依存を避け手書きで判定する。
//!
//! 方針:
//! - トップポスト（本文の下に引用が続く）を主対象に、最初の引用開始位置で本文と引用を切る。
//! - 引用開始の目印: 属性行（"On … wrote:" / "…が書きました:" / "-----元のメッセージ-----" ほか）、
//!   または連続する `>` 引用行。属性行があれば、その直前までを本文とする。
//! - 署名（`-- ` 区切り／"Sent from my iPhone" 等）は本文からも引用からも落とす。
//! - 100% は狙わない。取りこぼしは手動分割/引用表示で救済する（THREADING.md §4）。

pub mod anchor;
mod attribution;
mod blocks;
pub mod header;
pub mod sender;
mod signature;
pub mod when;

use attribution::{parse_attribution, wrapped_attribution};
use blocks::quote_blocks;
use signature::strip_signature;

/// 1 つの引用ブロック（属性行から差出人・時刻を、本文からフィンガープリントを取る）。
pub struct QuoteBlock {
    pub order: i64,
    /// 引用元の差出人（属性行から抽出。"名前 <addr>" や addr 単体。取れなければ None）。
    pub quoted_from: Option<String>,
    /// 引用元の送信時刻（属性行から抽出。生テキストのまま。取れなければ None）。
    pub quoted_at: Option<String>,
    /// 引用元の件名（Outlook 等の見出しにあれば。表示用で、保存はしない）。
    pub subject: Option<String>,
    /// 見出しを除いた引用の本文（表示用で、保存はしない）。
    pub body: String,
    /// 見出しも含めた、このブロックの全文（1 通として読めなかったとき、直前の枠に残す。表示用）。
    pub text: String,
    /// 引用本文の正規化ハッシュ（同じ履歴を引いているかの照合キー）。
    pub fingerprint: String,
}

/// 分離結果。
pub struct Split {
    /// 引用・署名を除いた新規本文。
    pub clean: String,
    /// 引用ブロック。末尾のまとまりを 1 ブロックとし、Outlook 等の「元のメッセージ」が
    /// 連なっていれば 1 通ずつに分ける（新しい順）。
    pub quotes: Vec<QuoteBlock>,
}

/// 本文（プレーン）を新規部分と引用に分離する。
pub fn split_reply(plain: &str) -> Split {
    // 1 行に詰まった見出し（"From：…To：…日時：…"）は、項目ごとの行に分けてから読む。
    let plain = header::packed::expand(plain);
    let lines: Vec<&str> = plain.lines().collect();

    // 1) 引用開始行を探す。
    //    a) まず属性行（"On … wrote:" / "…が書きました:" / "-----元のメッセージ-----" 等）。
    //       これは末尾の返信引用の確実な目印なので、本文途中のインライン `>` 引用より優先する
    //       （インライン引用の後ろに書いた新規テキストを clean から落とさないため）。
    //       属性行が改行で 2 行に割れていれば、前の行から始める（`wrapped_attribution`）。
    //       罫線は、続く行に見出しが来るときだけ区切りとみなす（`header::starts_rule_separator`）。
    //       アドレスの無い "From: 名前" / "送信者：名前" は、続く行に見出しが来るときだけ
    //       （通知メールの本文の「送信者：○○」で切らないため）。
    let mut quote_start: Option<usize> = None;
    let mut attribution: Option<(Option<String>, Option<String>)> = None;
    let mut attribution_lines = 1;
    for (i, line) in lines.iter().enumerate() {
        if let Some(found) = i
            .checked_sub(1)
            .and_then(|p| wrapped_attribution(lines[p], line))
        {
            quote_start = Some(i - 1);
            attribution = Some(found);
            attribution_lines = 2;
            break;
        }
        let found = parse_attribution(line)
            .or_else(|| header::starts_rule_separator(&lines, i).then_some((None, None)));
        if let Some(found) = found.filter(|_| header::from_line_backed(&lines, i)) {
            quote_start = Some(i);
            attribution = Some(found);
            break;
        }
    }
    //       "From:" の見出しで切ったときは、その上に続く見出しの行（"Date:" が先に来る書き方）も含める。
    if let Some(qs) = quote_start.filter(|&qs| header::is_from_line(lines[qs])) {
        let head = lines[..qs]
            .iter()
            .rev()
            .take_while(|l| header::is_header_line(l))
            .count();
        quote_start = Some(qs - head);
    }
    //    b) 属性行が無ければ、見出しが近くに固まって始まる行（予備の判定。
    //       `header::cluster::starts_header_cluster`）。
    if quote_start.is_none() {
        quote_start = (0..lines.len()).find(|&i| header::cluster::starts_header_cluster(&lines, i));
    }
    //    c) どちらも無いときだけ、最初の `>` 引用行を引用開始とする（属性行なしで下部に
    //       いきなり `>` 引用が来る返信のため。従来動作）。
    if quote_start.is_none() {
        for (i, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with('>') {
                quote_start = Some(i);
                break;
            }
        }
    }

    // 2) 本文の終端＝引用開始。署名は後段の strip_signature で落とす（原文 body_plain は温存）。
    // ※ メール末尾（引用より後ろ）に付く署名は、引用ごと落ちるのでここで扱う必要はない。
    let body_end = quote_start.unwrap_or(lines.len());
    let clean = strip_signature(lines[..body_end].join("\n").trim());

    // 3) 引用ブロック（引用開始〜末尾）。「元のメッセージ」が連なっていれば 1 通ずつに分ける。
    let quotes = match quote_start {
        Some(qs) => quote_blocks(&lines[qs..], attribution, attribution_lines),
        None => Vec::new(),
    };

    Split { clean, quotes }
}

/// 後方互換の薄いラッパ（旧 strip_quotes 相当）。新規本文だけ欲しいとき用。
pub fn clean_body(plain: &str) -> String {
    split_reply(plain).clean
}

/// 正規化テキストの安定ハッシュ（FNV-1a 64bit → 16 桁 hex）。外部依存を足さない軽量版。
pub fn fingerprint(text: &str) -> String {
    // 空白・改行を 1 つに畳んで正規化してからハッシュする。
    let norm: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in norm.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod diag;
#[cfg(test)]
mod tests;
