//! Google コンタクトの CSV(「Google CSV 形式」)を取り込む。
//!
//! ファイル文法は通常の RFC 4180 CSV（UTF-8・カンマ区切り・`"` 引用・`""` エスケープ・
//! セル内改行可）。Google 固有なのは列スキーマで、ヘッダ名が固定（`First Name` 等）、
//! 1 セルに複数値を ` ::: ` で連結、`E-mail 1/2/3`・`Phone 1〜4` の番号付き列を持つ点。
//! UID 列は無いので external_id は付かない（重複整理は氏名＋メール/電話で扱う）。

use super::vcard::{ImportedContact, ParseResult};
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

fn build_contact(idx: &HashMap<String, usize>, row: &[String]) -> Option<ImportedContact> {
    let first = get(idx, row, "First Name");
    let middle = get(idx, row, "Middle Name");
    let last = get(idx, row, "Last Name");
    let org = get(idx, row, "Organization Name");

    // メール（E-mail 1..3 - Value、各セルは ::: 複数可）。pref 概念は無いので出現順。
    let mut emails: Vec<String> = Vec::new();
    for n in 1..=3 {
        for v in multi(get(idx, row, &format!("E-mail {n} - Value"))) {
            let v = v.to_lowercase();
            if !emails.contains(&v) {
                emails.push(v);
            }
        }
    }
    let email = emails.first().cloned();
    let emails_json = if emails.len() > 1 {
        serde_json::to_string(&emails[1..]).ok()
    } else {
        None
    };

    // 電話（Phone 1..4 - Value）。
    let mut phone = None;
    for n in 1..=4 {
        if let Some(p) = multi(get(idx, row, &format!("Phone {n} - Value")))
            .into_iter()
            .next()
        {
            phone = Some(p);
            break;
        }
    }

    // 住所（Address 1 - Formatted を優先。無ければ構成要素を連結）。
    let address = {
        let f = get(idx, row, "Address 1 - Formatted");
        if !f.is_empty() {
            Some(f.replace(['\n', '\r'], " ").trim().to_string())
        } else {
            let parts: Vec<&str> = [
                "Address 1 - Postal Code",
                "Address 1 - Region",
                "Address 1 - City",
                "Address 1 - Street",
                "Address 1 - Extended Address",
            ]
            .iter()
            .map(|k| get(idx, row, k))
            .filter(|s| !s.is_empty())
            .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        }
    };

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
        name_kana,
        email,
        emails_json,
        phone,
        organization: non_empty(org),
        address,
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
        assert_eq!(c.name_kana.as_deref(), Some("アイカワ"));
        assert_eq!(c.email.as_deref(), Some("rabbit@key.ocn.ne.jp"));
        assert_eq!(c.emails_json.as_deref(), Some("[\"second@x.jp\"]"));
        assert_eq!(c.phone.as_deref(), Some("0997-52-4187"));
        assert_eq!(c.organization.as_deref(), Some("有限会社愛建工業"));
        assert_eq!(c.birthday.as_deref(), Some("1987-10-06"));
        assert_eq!(c.source, "google");
        assert!(c.external_id.is_none());
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
