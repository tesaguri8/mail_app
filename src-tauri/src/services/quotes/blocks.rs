//! 引用の領域（引用開始〜末尾）を、連なった「元のメッセージ」・属性行・見出しで 1 通ずつに分ける。

use super::attribution::{line_has_date_or_email, parse_attribution};
use super::{fingerprint, header, QuoteBlock};

/// 引用の領域の途中にある属性行（"On … wrote:" / "〜さんは書きました:" 等）なら、その (from, at)。
/// 区切り行と "From:" の見出しは別に扱うので含めない。`>` の付いた行（引用の中の引用）と、
/// 日付もアドレスも無い行（本文の「…と彼は書きました:」）は属性行とみなさない。
fn inner_attribution(line: &str) -> Option<(Option<String>, Option<String>)> {
    let l = line.trim();
    if l.starts_with('>')
        || header::is_separator(l)
        || header::is_from_line(l)
        || !line_has_date_or_email(l)
    {
        return None;
    }
    parse_attribution(l)
}

/// 引用の領域で、次の 1 通が始まる行か。`>` の付かない「元のメッセージ」の区切り、属性行
/// （"On … wrote:" 等）、または区切りの無い見出し（"From:" の後に "Sent:" 等が続く）。
/// 区切りの直後の "From:" は同じ 1 通。
fn starts_next_block(region: &[&str], j: usize) -> bool {
    if header::is_separator(region[j]) || inner_attribution(region[j]).is_some() {
        return true;
    }
    let prev = region[..j].iter().rev().find(|l| !l.trim().is_empty());
    if prev.is_some_and(|l| header::is_separator(l)) {
        return false;
    }
    // 見出しらしさの裏付け: 続く数行に日時の項目がある。
    let known = header::is_from_line(region[j])
        && region[j + 1..]
            .iter()
            .take(4)
            .any(|l| header::is_date_line(l));
    // 既知の形で読めなければ、見出しが近くに固まっているかで見る（予備の判定）。
    known || header::cluster::starts_header_cluster(region, j)
}

/// 行の先頭の共通の字下げを外して 1 つの文字列にする（字下げされた転送の中の 1 通を、
/// 左に寄せて見せるため）。前後の空行は落とす。
fn dedent(lines: &[&str]) -> String {
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        // 外すのは半角の空白とタブだけ（全角の空白は本文の字下げとして残す）。
        .map(|l| l.len() - l.trim_start_matches([' ', '\t']).len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        // 空白だけの行は indent より短いことがあるので空にする。
        .map(|l| {
            if l.trim().is_empty() {
                ""
            } else {
                l.get(indent..).unwrap_or(l)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_matches('\n')
        .trim_end()
        .to_string()
}

/// 引用の領域（引用開始〜末尾）を 1 通ずつのブロックに分ける。先頭のブロックは、本文を切った
/// 属性行（`attribution`。`>` 行で切ったときは None）の差出人・時刻を、見出しから取れなかった
/// 分の代わりに使う。
pub(super) fn quote_blocks(
    region: &[&str],
    attribution: Option<(Option<String>, Option<String>)>,
) -> Vec<QuoteBlock> {
    // 先頭のブロックが属性行（"On … wrote:" 等）から始まるなら、その行は見出しとして飛ばす。
    let skip_attribution_line = attribution.is_some() && !header::is_header_line(region[0]);
    // 次の 1 通の始まりは、いまの 1 通の見出しより後ろでだけ探す（項目名と値が別の行にある
    // 見出しでは "From:" の後に "To:" 等が続き、区切りの無い見出しと見分けられないため）。
    let mut starts: Vec<usize> = vec![0];
    let mut header_end = header::parse_header(region, skip_attribution_line).body_start;
    for j in 1..region.len() {
        if j < header_end || !starts_next_block(region, j) {
            continue;
        }
        starts.push(j);
        let skip = inner_attribution(region[j]).is_some();
        header_end = j + header::parse_header(&region[j..], skip).body_start;
    }
    let mut first = Some(attribution.unwrap_or((None, None)));
    starts
        .iter()
        .enumerate()
        .map(|(k, &s)| {
            let e = starts.get(k + 1).copied().unwrap_or(region.len());
            let block = &region[s..e];
            // 途中のブロックが属性行で始まるなら、差出人・時刻はその行から取り、行は見出しとして飛ばす。
            let inner = (k > 0).then(|| inner_attribution(block[0])).flatten();
            let skip_first = (k == 0 && skip_attribution_line) || inner.is_some();
            let h = header::parse_header(block, skip_first);
            let (attr_from, attr_at) = first.take().or(inner).unwrap_or((None, None));
            // フィンガープリントは引用本文（`>` と空白を正規化）から作る。
            let quoted_text: String = block
                .iter()
                .map(|l| l.trim_start().trim_start_matches('>').trim())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            QuoteBlock {
                order: k as i64,
                quoted_from: h.from.or(attr_from),
                quoted_at: h.at.or(attr_at),
                subject: h.subject,
                body: dedent(&block[h.body_start.min(block.len())..]),
                text: dedent(block),
                fingerprint: fingerprint(&quoted_text),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
