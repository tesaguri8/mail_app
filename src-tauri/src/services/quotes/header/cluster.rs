//! 既知の書式で読めなかったときの予備の判定: 見出しが近くに固まっていれば 1 通の区切りとみなす
//! （2026-10-20 利用者の発案。docs/THREADING.md §2「引用ブロックの分け方と照合」）。

use super::{header_field, is_header_line, next_non_blank, Field};
use crate::services::quotes::when;

/// 見出しのかたまりとみなす範囲の行数（空行は数えない）。
const CLUSTER_LINES: usize = 12;

/// 見出しのかたまりに要る、見出しの種類の数（差出人・日付・宛先・件名・Cc のうち）。
const CLUSTER_KINDS: usize = 3;

/// 項目の値。同じ行に無ければ、空行を挟んだ次の行（それも項目なら値は無い）。
fn field_value<'a>(block: &[&'a str], j: usize, inline: &'a str) -> &'a str {
    if !inline.is_empty() {
        return inline;
    }
    next_non_blank(block, j)
        .filter(|&k| header_field(block[k]).is_none())
        .map_or("", |k| block[k].trim())
}

/// 既知の書式で読めなかったときの予備の判定: `j` の行から、見出しがかたまって始まるか。
///
/// 空行を除いた [`CLUSTER_LINES`] 行の中に、差出人・日付・宛先・件名・Cc の見出しが
/// [`CLUSTER_KINDS`] 種類以上あり、そのうちに**差出人と日付**があり、値に**アドレス**（`@`）と
/// **日時として読めるもの**が 1 つ以上ある。値は同じ行でも、空行を挟んだ次の行でもよい。
/// To と Subject は必須にしない（Bcc で受けたメール・"On … wrote:" の形には無い）。
/// 本文の申込書や見積の「件名:」「日付:」だけの段落は、差出人・アドレスが無いので区切らない。
/// かたまりの途中の行では始まらない（直前の空でない行が見出しなら false）。
pub fn starts_header_cluster(block: &[&str], j: usize) -> bool {
    let is_field = is_header_line;
    if !is_field(block[j]) {
        return false;
    }
    if block[..j]
        .iter()
        .rev()
        .find(|l| !l.trim().is_empty())
        .is_some_and(|l| is_field(l))
    {
        return false;
    }
    let mut kinds: Vec<Field> = Vec::new();
    let (mut addr, mut date) = (false, false);
    let window = (j..block.len())
        .filter(|&k| !block[k].trim().is_empty())
        .take(CLUSTER_LINES);
    for k in window {
        let Some((field, inline)) = header_field(block[k]).filter(|_| is_field(block[k])) else {
            continue;
        };
        let value = field_value(block, k, inline);
        if field != Field::Other && !kinds.contains(&field) {
            kinds.push(field);
        }
        addr |= value.contains('@');
        date |= field == Field::Date && when::parse_quoted_time(value).is_some();
    }
    kinds.len() >= CLUSTER_KINDS && kinds.contains(&Field::From) && addr && date
}
