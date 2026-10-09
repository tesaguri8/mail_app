//! 郵便番号 ⇄ 住所（日本）。
//!
//! 日本郵便の郵便番号データをアプリに同梱し（`data/postal/ken_all.tsv.zst`。作り方は
//! `scripts/gen-postal-data.mjs`）、住所を外部へ送らずに引く。表は初回の検索で一度だけ展開し、
//! プロセスの終わりまで持つ。
//!
//! - 郵便番号 → 住所: [`lookup_by_code`]
//! - 住所 → 郵便番号: [`lookup_by_address`]（町域まで前方一致させ、丁目・注記で絞る）

use crate::models::PostalAddress;
use std::collections::HashMap;
use std::sync::OnceLock;

/// 同梱の郵便番号表（1 行 = 郵便番号7桁 \t 都道府県 \t 市区町村 \t 町域 \t 注記）。
static KEN_ALL: &[u8] = include_bytes!("../../data/postal/ken_all.tsv.zst");

/// 住所から引いた候補がこれより多ければ、絞れていないとみなして何も返さない
/// （大きなビルの階ごとの番号など。選ばせても役に立たない）。
const MAX_CANDIDATES: usize = 8;

/// 郵便番号表を読めないときのエラー（同梱データが壊れている場合のみ起きる）。
#[derive(Debug, Clone, thiserror::Error)]
pub enum PostalError {
    #[error("郵便番号データを展開できません: {0}")]
    Decompress(String),
    #[error("郵便番号データの文字コードが正しくありません")]
    Encoding,
}

/// 表の 1 行。
struct Record {
    code: &'static str,
    pref: &'static str,
    city: &'static str,
    /// 町域（市区町村の既定の番号は空）。
    town: &'static str,
    /// 町域の括弧書き（「１〜４丁目」「次のビルを除く」「その他」など。無ければ空）。
    note: &'static str,
}

/// 同じ都道府県・市区町村の行のまとまり（住所からの検索を、市区町村の一致で先に絞る）。
struct Area {
    /// 正規化した「都道府県＋市区町村」。
    pref_city: String,
    /// 正規化した「市区町村」（都道府県が空の入力用）。
    city: String,
    records: Vec<usize>,
}

struct PostalTable {
    records: Vec<Record>,
    by_code: HashMap<&'static str, Vec<usize>>,
    areas: Vec<Area>,
}

/// 郵便番号表（初回だけ展開する）。
fn table() -> Result<&'static PostalTable, PostalError> {
    static TABLE: OnceLock<Result<PostalTable, PostalError>> = OnceLock::new();
    TABLE.get_or_init(load).as_ref().map_err(Clone::clone)
}

fn load() -> Result<PostalTable, PostalError> {
    let raw = zstd::decode_all(KEN_ALL).map_err(|e| PostalError::Decompress(e.to_string()))?;
    let text = String::from_utf8(raw).map_err(|_| PostalError::Encoding)?;
    // 表はプロセスの終わりまで使うので、一度だけ確保したまま 'static にする
    // （12 万行を行ごとの String にせず、借用で持てる）。
    let text: &'static str = Box::leak(text.into_boxed_str());
    let records: Vec<Record> = text.lines().filter_map(parse_line).collect();

    let mut by_code: HashMap<&'static str, Vec<usize>> = HashMap::new();
    let mut area_of: HashMap<(&'static str, &'static str), usize> = HashMap::new();
    let mut areas: Vec<Area> = Vec::new();
    for (i, r) in records.iter().enumerate() {
        by_code.entry(r.code).or_default().push(i);
        let a = *area_of.entry((r.pref, r.city)).or_insert_with(|| {
            areas.push(Area {
                pref_city: norm(&format!("{}{}", r.pref, r.city)),
                city: norm(r.city),
                records: Vec::new(),
            });
            areas.len() - 1
        });
        areas[a].records.push(i);
    }
    Ok(PostalTable {
        records,
        by_code,
        areas,
    })
}

fn parse_line(line: &'static str) -> Option<Record> {
    let mut cols = line.split('\t');
    Some(Record {
        code: cols.next()?,
        pref: cols.next()?,
        city: cols.next()?,
        town: cols.next()?,
        note: cols.next()?,
    })
}

/// 突き合わせ用の正規化: 全角英数・記号を半角へ、空白を除き、表記ゆれ（ヶ/ケ、各種ハイフン）を揃える。
fn norm(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0),
            'ヶ' | 'ｹ' => Some('ケ'),
            'ー' | '‐' | '−' | '–' | '—' | '―' => Some('-'),
            c if c.is_whitespace() => None,
            c => Some(c),
        })
        .collect()
}

/// 7 桁を「NNN-NNNN」に。
fn format_code(code: &str) -> String {
    format!(
        "{}-{}",
        &code[..3.min(code.len())],
        &code[3.min(code.len())..]
    )
}

fn to_address(r: &Record) -> PostalAddress {
    PostalAddress {
        postal: format_code(r.code),
        region: r.pref.to_string(),
        city: r.city.to_string(),
        town: r.town.to_string(),
    }
}

/// 郵便番号から住所（都道府県・市区町村・町域）を引く。
///
/// `code` は全角数字・ハイフンの有無を問わない。7 桁でなければ空。1 つの郵便番号が複数の
/// 町域にまたがるときは、町域ごとに返す（呼び出し側で選ばせる）。
///
/// # Errors
/// 同梱の郵便番号表を展開できないとき。
pub fn lookup_by_code(code: &str) -> Result<Vec<PostalAddress>, PostalError> {
    let digits: String = norm(code).chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 7 {
        return Ok(Vec::new());
    }
    let t = table()?;
    let mut out: Vec<PostalAddress> = Vec::new();
    for a in t
        .by_code
        .get(digits.as_str())
        .into_iter()
        .flatten()
        .map(|&i| to_address(&t.records[i]))
    {
        if !out.contains(&a) {
            out.push(a);
        }
    }
    Ok(out)
}

/// 住所（都道府県・市区町村・町域以降）から郵便番号を引く。
///
/// 都道府県＋市区町村＋町域が入力の先頭に一致する行のうち、町域がいちばん長く一致したものを
/// 採り、丁目（「３丁目」「3-1-2」）や注記（「新町」など）で絞る。町域が一致しなければ、
/// 市区町村の既定の番号（「以下に掲載がない場合」）を返す。絞り切れなければ候補を郵便番号ごとに
/// 返し、多すぎれば空を返す。`region` が空なら市区町村から突き合わせる。
///
/// # Errors
/// 同梱の郵便番号表を展開できないとき。
pub fn lookup_by_address(
    region: &str,
    city: &str,
    street: &str,
) -> Result<Vec<PostalAddress>, PostalError> {
    let with_pref = !norm(region).is_empty();
    let full = format!("{}{}{}", norm(region), norm(city), norm(street));
    if full.is_empty() {
        return Ok(Vec::new());
    }
    let t = table()?;

    // (行, 町域より後ろの残り) のうち、町域がいちばん長く一致したもの。
    let mut best_len = 0;
    let mut best: Vec<(usize, String)> = Vec::new();
    for area in &t.areas {
        let key = if with_pref {
            &area.pref_city
        } else {
            &area.city
        };
        let Some(rest) = full.strip_prefix(key.as_str()) else {
            continue;
        };
        for &i in &area.records {
            let town = norm(t.records[i].town);
            let Some(remainder) = rest.strip_prefix(town.as_str()) else {
                continue;
            };
            let len = key.len() + town.len();
            if len > best_len {
                best_len = len;
                best.clear();
            }
            if len == best_len {
                best.push((i, remainder.to_string()));
            }
        }
    }

    let picked = narrow(&t.records, &best);
    let mut out: Vec<PostalAddress> = Vec::new();
    for a in picked.into_iter().map(|i| to_address(&t.records[i])) {
        if !out.iter().any(|o| o.postal == a.postal) {
            out.push(a);
        }
    }
    Ok(if out.len() > MAX_CANDIDATES {
        Vec::new()
    } else {
        out
    })
}

/// 同じ町域に番号が複数あるとき、残りの住所（丁目・注記）で絞る。
fn narrow(records: &[Record], best: &[(usize, String)]) -> Vec<usize> {
    let codes = |v: &[usize]| {
        let mut c: Vec<&str> = v.iter().map(|&i| records[i].code).collect();
        c.sort_unstable();
        c.dedup();
        c.len()
    };
    let all: Vec<usize> = best.iter().map(|(i, _)| *i).collect();
    if codes(&all) <= 1 {
        return all;
    }
    // 注記が残りの住所に合うもの（丁目の範囲・地名）。
    let matched: Vec<usize> = best
        .iter()
        .filter(|(i, rem)| note_matches(records[*i].note, rem))
        .map(|(i, _)| *i)
        .collect();
    if !matched.is_empty() {
        return matched;
    }
    // 合う注記が無ければ、注記なし・「その他」（その町域の残り全部）。
    let rest: Vec<usize> = best
        .iter()
        .filter(|(i, _)| matches!(records[*i].note, "" | "その他"))
        .map(|(i, _)| *i)
        .collect();
    if rest.is_empty() {
        all
    } else {
        rest
    }
}

/// 注記が、町域より後ろの住所 `remainder` に合うか。
fn note_matches(note: &str, remainder: &str) -> bool {
    let note = norm(note);
    if note.is_empty() || note == "その他" {
        return false;
    }
    match chome_ranges(&note) {
        Some(ranges) => {
            chome_of(remainder).is_some_and(|n| ranges.iter().any(|&(a, b)| (a..=b).contains(&n)))
        }
        None => remainder.starts_with(note.as_str()),
    }
}

/// 「1〜4丁目」「1、2丁目」「2丁目」「18〜25丁目」を範囲の並びへ。丁目の注記でなければ None。
fn chome_ranges(note: &str) -> Option<Vec<(u32, u32)>> {
    let body = note.strip_suffix("丁目")?;
    body.split('、')
        .map(|part| {
            let part = part.strip_suffix("丁目").unwrap_or(part);
            match part.split_once(['〜', '~']) {
                Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
                None => part.parse().ok().map(|n| (n, n)),
            }
        })
        .collect()
}

/// 町域より後ろの住所の先頭から丁目の番号を読む（「3丁目」「三丁目」「3-1-2」「3」）。
fn chome_of(remainder: &str) -> Option<u32> {
    let digits: String = remainder.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() {
        let after = &remainder[digits.len()..];
        return (after.is_empty() || after.starts_with("丁目") || after.starts_with('-'))
            .then(|| digits.parse().ok())
            .flatten();
    }
    let kanji: String = remainder
        .chars()
        .take_while(|c| "〇一二三四五六七八九十".contains(*c))
        .collect();
    remainder[kanji.len()..]
        .starts_with("丁目")
        .then(|| kanji_number(&kanji))
        .flatten()
}

/// 漢数字（〜九十九）を数へ。「十二」「二十」「二十三」「三」。
fn kanji_number(s: &str) -> Option<u32> {
    let digit = |c: char| {
        "〇一二三四五六七八九"
            .chars()
            .position(|d| d == c)
            .map(|p| p as u32)
    };
    match s.split_once('十') {
        Some((tens, ones)) => {
            let t = if tens.is_empty() {
                1
            } else {
                digit(tens.chars().next()?)?
            };
            let o = if ones.is_empty() {
                0
            } else {
                digit(ones.chars().next()?)?
            };
            Some(t * 10 + o)
        }
        None if s.chars().count() == 1 => digit(s.chars().next()?),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(v: &[PostalAddress]) -> Vec<&str> {
        v.iter().map(|a| a.postal.as_str()).collect()
    }

    #[test]
    fn code_to_address() {
        let got = lookup_by_code("0600042").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].region, "北海道");
        assert_eq!(got[0].city, "札幌市中央区");
        assert_eq!(got[0].town, "大通西");
        assert_eq!(got[0].postal, "060-0042");
        // 全角・ハイフン付きでも引ける。
        assert_eq!(lookup_by_code("０６０－００４２").unwrap(), got);
        // 7 桁でなければ空。
        assert!(lookup_by_code("06000").unwrap().is_empty());
    }

    #[test]
    fn default_code_of_city_has_empty_town() {
        let got = lookup_by_code("060-0000").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].town, "");
    }

    #[test]
    fn address_to_code_uses_chome_range() {
        let a = lookup_by_address("北海道", "札幌市中央区", "大通西3丁目").unwrap();
        assert_eq!(codes(&a), ["060-0042"]);
        let b = lookup_by_address("北海道", "札幌市中央区", "大通西２２丁目").unwrap();
        assert_eq!(codes(&b), ["064-0820"]);
        let c = lookup_by_address("北海道", "札幌市中央区", "大通西二十二丁目").unwrap();
        assert_eq!(codes(&c), ["064-0820"]);
        let d = lookup_by_address("北海道", "札幌市中央区", "大通西22-1-3").unwrap();
        assert_eq!(codes(&d), ["064-0820"]);
    }

    #[test]
    fn address_to_code_ignores_building_notes() {
        // 「丸の内（次のビルを除く）」に落ちる（ビル名の行は町域が一致しない）。
        let a = lookup_by_address("東京都", "千代田区", "丸の内1-1-1").unwrap();
        assert_eq!(codes(&a), ["100-0005"]);
    }

    #[test]
    fn unknown_town_falls_back_to_city_default() {
        let a = lookup_by_address("北海道", "札幌市中央区", "どこにもない町1-2").unwrap();
        assert_eq!(codes(&a), ["060-0000"]);
    }

    #[test]
    fn region_may_be_empty_and_spacing_is_ignored() {
        let a = lookup_by_address("", "札幌市中央区", " 大通西 3丁目").unwrap();
        assert_eq!(codes(&a), ["060-0042"]);
    }

    #[test]
    fn nothing_for_unknown_city() {
        assert!(lookup_by_address("東京都", "存在しない市", "本町1")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn chome_parsing() {
        assert_eq!(chome_ranges("1〜4丁目"), Some(vec![(1, 4)]));
        assert_eq!(chome_ranges("1、2丁目"), Some(vec![(1, 1), (2, 2)]));
        assert_eq!(chome_ranges("新町"), None);
        assert_eq!(chome_of("3丁目5"), Some(3));
        assert_eq!(chome_of("12-3"), Some(12));
        assert_eq!(chome_of("三丁目"), Some(3));
        assert_eq!(chome_of("123番地"), None);
        assert_eq!(kanji_number("二十三"), Some(23));
        assert_eq!(kanji_number("十"), Some(10));
    }
}
