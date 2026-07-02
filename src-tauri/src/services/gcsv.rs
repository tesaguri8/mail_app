//! Google コンタクトの CSV(「Google CSV 形式」)を取り込む。
//!
//! ファイル文法は通常の RFC 4180 CSV（UTF-8・カンマ区切り・`"` 引用・`""` エスケープ・
//! セル内改行可）。Google 固有なのは列スキーマで、ヘッダ名が固定（`First Name` 等）、
//! 1 セルに複数値を ` ::: ` で連結、`E-mail 1/2/3`・`Phone 1〜4` の番号付き列を持つ点。
//! UID 列は無いので external_id は付かない（重複整理は氏名＋メール/電話で扱う）。

use super::vcard::{ImportedAddress, ImportedContact, ImportedValue, ParseResult};
use std::collections::HashMap;

const MULTI_SEP: &str = ":::"; // Google の複数値区切り（実際は " ::: "）

/// Google CSV テキストをパースする。
pub fn parse(text: &str) -> ParseResult {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut records = parse_records(text).into_iter();

    let header = match records.next() {
        Some(h) => h,
        None => return ParseResult::default(),
    };
    let idx: HashMap<String, usize> = header
        .iter()
        .enumerate()
        .map(|(i, name)| (name.trim().to_string(), i))
        .collect();

    let mut result = ParseResult::default();
    for row in records {
        // 全セル空の行（末尾の空行など）は無視。
        if row.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        result.total_cards += 1;
        if let Some(c) = build_contact(&idx, &row) {
            result.contacts.push(c);
        }
    }
    result
}

/// ヘッダ名でセルを引く（無い列や範囲外は空文字）。
fn get<'a>(idx: &HashMap<String, usize>, row: &'a [String], key: &str) -> &'a str {
    idx.get(key)
        .and_then(|&i| row.get(i))
        .map(|s| s.trim())
        .unwrap_or("")
}

/// ` ::: ` 連結された複数値を分解（空要素は除く）。
fn multi(value: &str) -> Vec<String> {
    value
        .split(MULTI_SEP)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// ` ::: ` で分解するが空要素も残す（住所の各サブ項目を位置で対応づけるため）。
/// 全体が空なら空 Vec を返す。
fn multi_positional(value: &str) -> Vec<String> {
    if value.trim().is_empty() {
        return Vec::new();
    }
    value
        .split(MULTI_SEP)
        .map(|s| s.trim().to_string())
        .collect()
}

fn build_contact(idx: &HashMap<String, usize>, row: &[String]) -> Option<ImportedContact> {
    let first = get(idx, row, "First Name");
    let middle = get(idx, row, "Middle Name");
    let last = get(idx, row, "Last Name");
    let org = get(idx, row, "Organization Name");

    // メール（E-mail 1..3、各セルは ::: で複数、対の Label 列あり）。
    let mut all_emails: Vec<ImportedValue> = Vec::new();
    for n in 1..=3 {
        let label = non_empty(get(idx, row, &format!("E-mail {n} - Label")));
        for v in multi(get(idx, row, &format!("E-mail {n} - Value"))) {
            let v = v.to_lowercase();
            if !all_emails.iter().any(|x| x.value == v) {
                all_emails.push(ImportedValue {
                    label: label.clone(),
                    value: v,
                    is_primary: all_emails.is_empty(),
                });
            }
        }
    }
    let email = all_emails.first().map(|v| v.value.clone());

    // 電話（Phone 1..4）。
    let mut all_phones: Vec<ImportedValue> = Vec::new();
    for n in 1..=4 {
        let label = non_empty(get(idx, row, &format!("Phone {n} - Label")));
        for v in multi(get(idx, row, &format!("Phone {n} - Value"))) {
            if !all_phones.iter().any(|x| x.value == v) {
                all_phones.push(ImportedValue {
                    label: label.clone(),
                    value: v,
                    is_primary: all_phones.is_empty(),
                });
            }
        }
    }
    let phone = all_phones.first().map(|v| v.value.clone());

    // 住所（Address 1..2）。各サブ項目が ` ::: ` で複数詰めなので位置で対応づけて分解。
    let mut all_addresses: Vec<ImportedAddress> = Vec::new();
    for n in 1..=2 {
        let labels = multi_positional(get(idx, row, &format!("Address {n} - Label")));
        let postals = multi_positional(get(idx, row, &format!("Address {n} - Postal Code")));
        let regions = multi_positional(get(idx, row, &format!("Address {n} - Region")));
        let cities = multi_positional(get(idx, row, &format!("Address {n} - City")));
        let streets = multi_positional(get(idx, row, &format!("Address {n} - Street")));
        let exts = multi_positional(get(idx, row, &format!("Address {n} - Extended Address")));
        let countries = multi_positional(get(idx, row, &format!("Address {n} - Country")));
        let count = [
            &labels, &postals, &regions, &cities, &streets, &exts, &countries,
        ]
        .iter()
        .map(|v| v.len())
        .max()
        .unwrap_or(0);
        let at = |v: &[String], i: usize| v.get(i).and_then(|s| non_empty(s));
        for i in 0..count {
            let a = ImportedAddress {
                label: at(&labels, i),
                postal: at(&postals, i),
                region: at(&regions, i),
                city: at(&cities, i),
                street: at(&streets, i),
                extended: at(&exts, i),
                country: at(&countries, i),
                is_primary: all_addresses.is_empty(),
            };
            if a.postal.is_some()
                || a.region.is_some()
                || a.city.is_some()
                || a.street.is_some()
                || a.extended.is_some()
                || a.country.is_some()
            {
                all_addresses.push(a);
            }
        }
    }
    let address = all_addresses
        .first()
        .map(crate::services::vcard::format_address);

    let name_kana = {
        let kl = get(idx, row, "Phonetic Last Name");
        let kf = get(idx, row, "Phonetic First Name");
        match (kl.is_empty(), kf.is_empty()) {
            (false, false) => Some(format!("{kl} {kf}")),
            (false, true) => Some(kl.to_string()),
            (true, false) => Some(kf.to_string()),
            (true, true) => None,
        }
    };

    // 表示名: 氏名 → File As → 組織 → メール → 電話。
    let display_name = build_display_name(last, middle, first)
        .or_else(|| non_empty(get(idx, row, "File As")))
        .or_else(|| non_empty(org))
        .or_else(|| email.clone())
        .or_else(|| phone.clone())?;

    Some(ImportedContact {
        display_name,
        family_name: non_empty(last),
        given_name: non_empty(first),
        phonetic_family: non_empty(get(idx, row, "Phonetic Last Name")),
        phonetic_given: non_empty(get(idx, row, "Phonetic First Name")),
        name_kana,
        email,
        phone,
        organization: non_empty(org),
        org_title: non_empty(get(idx, row, "Organization Title")),
        org_department: non_empty(get(idx, row, "Organization Department")),
        address,
        all_emails,
        all_phones,
        all_addresses,
        birthday: non_empty(get(idx, row, "Birthday")),
        note: non_empty(get(idx, row, "Notes")).map(|s| s.replace("\r\n", "\n")),
        source: "google".to_string(),
        external_id: None,
    })
}

/// 姓・ミドル・名から表示名を作る。CJK のみなら詰め、そうでなければ空白区切り。
fn build_display_name(last: &str, middle: &str, first: &str) -> Option<String> {
    let parts: Vec<&str> = [last, middle, first]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    match parts.len() {
        0 => None,
        1 => Some(parts[0].to_string()),
        _ => {
            if parts.iter().all(|s| is_cjk(s)) {
                Some(parts.concat())
            } else {
                Some(parts.join(" "))
            }
        }
    }
}

fn is_cjk(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| !c.is_ascii())
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// RFC 4180 CSV を行×セルに分解する（引用・`""`・セル内改行・CRLF/LF 対応）。
fn parse_records(text: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else {
            match c {
                '"' => in_quotes = true,
                ',' => row.push(std::mem::take(&mut field)),
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                _ => field.push(c),
            }
        }
    }
    // 末尾に改行が無い最終行を回収。
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "First Name,Middle Name,Last Name,Phonetic First Name,Phonetic Middle Name,Phonetic Last Name,Name Prefix,Name Suffix,Nickname,File As,Organization Name,Organization Title,Organization Department,Birthday,Notes,Photo,Labels,E-mail 1 - Label,E-mail 1 - Value,E-mail 2 - Label,E-mail 2 - Value,E-mail 3 - Label,E-mail 3 - Value,Phone 1 - Label,Phone 1 - Value";

    fn parse_one(data_row: &str) -> ImportedContact {
        let text = format!("{HEADER}\n{data_row}\n");
        parse(&text).contacts.into_iter().next().unwrap()
    }

    #[test]
    fn maps_google_columns() {
        // 愛川翼, 組織, 2 メール(1 セルに :::), 電話, よみ
        let row = "翼,,愛川,アイカワ,,,,,,,有限会社愛建工業,,,1987-10-06,memo,,* myContacts,,rabbit@key.ocn.ne.jp ::: second@x.jp,,,,,,0997-52-4187";
        let c = parse_one(row);
        assert_eq!(c.display_name, "愛川翼");
        assert_eq!(c.family_name.as_deref(), Some("愛川")); // Last Name
        assert_eq!(c.given_name.as_deref(), Some("翼")); // First Name
        assert_eq!(c.phonetic_given.as_deref(), Some("アイカワ")); // Phonetic First 列にある
        assert_eq!(c.name_kana.as_deref(), Some("アイカワ"));
        assert_eq!(c.email.as_deref(), Some("rabbit@key.ocn.ne.jp"));
        assert_eq!(c.all_emails.len(), 2); // 1セル ::: の2件を保持
        assert_eq!(c.all_emails[1].value, "second@x.jp");
        assert_eq!(c.phone.as_deref(), Some("0997-52-4187"));
        assert_eq!(c.organization.as_deref(), Some("有限会社愛建工業"));
        assert_eq!(c.birthday.as_deref(), Some("1987-10-06"));
        assert_eq!(c.source, "google");
        assert!(c.external_id.is_none());
    }

    #[test]
    fn google_multi_address_split_by_triple_colon() {
        // Google CSV は住所も1セルに ` ::: ` で複数詰める。位置対応で複数住所に分解する。
        let header = "First Name,Last Name,Address 1 - Label,Address 1 - Postal Code,\
            Address 1 - Region,Address 1 - City,Address 1 - Street";
        let row = "太郎,山田,自宅 ::: 自宅,9050018 ::: 9050207,沖縄県 ::: 沖縄県,\
            名護市 ::: 本部町,大西1-15-5 ::: 備瀬535";
        let text = format!("{header}\n{row}\n");
        let c = parse(&text).contacts.into_iter().next().unwrap();
        assert_eq!(c.all_addresses.len(), 2);
        assert_eq!(c.all_addresses[0].postal.as_deref(), Some("9050018"));
        assert_eq!(c.all_addresses[0].city.as_deref(), Some("名護市"));
        assert_eq!(c.all_addresses[1].postal.as_deref(), Some("9050207"));
        assert_eq!(c.all_addresses[1].city.as_deref(), Some("本部町"));
    }

    #[test]
    fn company_row_uses_org_as_name() {
        let row = ",,,,,,,,,,浦添設計研究所,,,,,,* myContacts,,,,,,,,(03) 5287-3625";
        let c = parse_one(row);
        assert_eq!(c.display_name, "浦添設計研究所");
        assert_eq!(c.phone.as_deref(), Some("(03) 5287-3625"));
    }

    #[test]
    fn quoted_fields_with_comma_and_newline() {
        // Notes に改行・カンマ、氏名に空白区切り（非 CJK）。
        let row = "John,,Smith,,,,,,,,\"Acme, Inc.\",,,,\"line1\nline2\",,,,john@x.com,,,,,,";
        let text = format!("{HEADER}\n{row}\n");
        let c = parse(&text).contacts.into_iter().next().unwrap();
        assert_eq!(c.display_name, "Smith John");
        assert_eq!(c.organization.as_deref(), Some("Acme, Inc."));
        assert_eq!(c.note.as_deref(), Some("line1\nline2"));
    }

    #[test]
    fn blank_rows_skipped_and_counted() {
        let text = format!("{HEADER}\n,,,,,,,,,,,,,,,,,,,,,,,,\n翼,,愛川,,,,,,,,,,,,,,,,,,,,,,\n");
        let r = parse(&text);
        // 空行は total にも含めない。実データ 1 行のみ。
        assert_eq!(r.total_cards, 1);
        assert_eq!(r.contacts.len(), 1);
    }
}
