//! 引用の領域（引用開始〜末尾）を、連なった「元のメッセージ」・属性行・見出しで 1 通ずつに分ける。

use super::attribution::{line_has_date_or_email, parse_attribution, wrapped_attribution};
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
/// 区切り・罫線の直後の "From:" や、罫線の直後の区切り（"=====" の次の "Begin forwarded message:"）は同じ 1 通。
fn starts_next_block(region: &[&str], j: usize) -> bool {
    let prev = region[..j].iter().rev().find(|l| !l.trim().is_empty());
    let after_rule = prev.is_some_and(|l| header::is_rule_line(l));
    if (header::is_separator(region[j]) && !after_rule)
        || header::starts_rule_separator(region, j)
        || inner_attribution(region[j]).is_some()
    {
        return true;
    }
    if after_rule || prev.is_some_and(|l| header::is_separator(l)) {
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

/// 行頭の「>」を 1 段外す（"> From: …" → "From: …"）。「>」が無ければそのまま。
fn strip_one_quote_level(line: &str) -> &str {
    let t = line.trim_start_matches([' ', '\t']);
    match t.strip_prefix('>') {
        Some(rest) => rest.strip_prefix(' ').unwrap_or(rest),
        None => line,
    }
}

/// ブロックの見出しを読む（[`header::parse_header_after`]）。最初の空でない行が「>」で始まる
/// ブロック（引用の中に入った "> Begin forwarded message:" / "> 差出人: …"）は、「>」を 1 段
/// 外した形で読む（行の数は変わらないので、本文の始まりの添字はそのまま使える）。
fn header_of(block: &[&str], skip: usize) -> header::Header {
    let quoted = block
        .iter()
        .find(|l| !l.trim().is_empty())
        .is_some_and(|l| l.trim_start().starts_with('>'));
    if !quoted {
        return header::parse_header_after(block, skip);
    }
    let view: Vec<&str> = block.iter().map(|l| strip_one_quote_level(l)).collect();
    header::parse_header_after(&view, skip)
}

/// 1 通の始まり。
struct Start {
    /// 領域の中の行の添字。
    line: usize,
    /// 見出しとして飛ばす属性行の行数（属性行が無ければ 0、改行で割れていれば 2）。
    skip: usize,
    /// 属性行から読んだ (差出人, 時刻)。
    attribution: Option<(Option<String>, Option<String>)>,
}

/// 引用の領域で `j` の行から次の 1 通が始まるなら、その始まり。属性行が改行で割れていれば
/// 前の行（`floor` より後ろのときだけ）から始める。
fn next_start(region: &[&str], j: usize, floor: usize) -> Option<Start> {
    let wrapped = (j > floor + 1 && !region[j - 1].trim_start().starts_with('>'))
        .then(|| wrapped_attribution(region[j - 1], region[j]))
        .flatten();
    if let Some(found) = wrapped {
        return Some(Start {
            line: j - 1,
            skip: 2,
            attribution: Some(found),
        });
    }
    if !starts_next_block(region, j) {
        return None;
    }
    let attribution = inner_attribution(region[j]);
    Some(Start {
        line: j,
        skip: usize::from(attribution.is_some()),
        attribution,
    })
}

/// 引用の領域（引用開始〜末尾）を 1 通ずつのブロックに分ける。先頭のブロックは、本文を切った
/// 属性行（`attribution`。`>` 行で切ったときは None。`attribution_lines` はその行数）の差出人・
/// 時刻を、見出しから取れなかった分の代わりに使う。
pub(super) fn quote_blocks(
    region: &[&str],
    attribution: Option<(Option<String>, Option<String>)>,
    attribution_lines: usize,
) -> Vec<QuoteBlock> {
    // 先頭のブロックが属性行（"On … wrote:" 等）から始まるなら、その行は見出しとして飛ばす。
    // 区切り行・罫線から始まるなら、続く区切りごと `parse_header_after` が飛ばす。
    let skip_attribution_line = attribution.is_some()
        && !header::is_header_line(region[0])
        && !header::is_separator(region[0])
        && !header::is_rule_line(region[0]);
    let mut starts = vec![Start {
        line: 0,
        skip: if skip_attribution_line {
            attribution_lines
        } else {
            0
        },
        attribution: Some(attribution.unwrap_or((None, None))),
    }];
    // 次の 1 通の始まりは、いまの 1 通の見出しより後ろでだけ探す（項目名と値が別の行にある
    // 見出しでは "From:" の後に "To:" 等が続き、区切りの無い見出しと見分けられないため）。
    let mut header_end = header_of(region, starts[0].skip).body_start;
    for j in 1..region.len() {
        if j < header_end {
            continue;
        }
        let Some(start) = next_start(region, j, header_end.saturating_sub(1)) else {
            continue;
        };
        header_end = start.line + header_of(&region[start.line..], start.skip).body_start;
        starts.push(start);
    }
    let ends: Vec<usize> = starts
        .iter()
        .skip(1)
        .map(|s| s.line)
        .chain([region.len()])
        .collect();
    starts
        .into_iter()
        .zip(ends)
        .enumerate()
        .map(|(k, (start, e))| {
            let block = &region[start.line..e];
            // 属性行で始まるなら、差出人・時刻はその行から取り、行は見出しとして飛ばす。
            let h = header_of(block, start.skip);
            let (attr_from, attr_at) = start.attribution.unwrap_or((None, None));
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
