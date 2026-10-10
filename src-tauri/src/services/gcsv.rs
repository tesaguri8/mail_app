//! Google コンタクトの CSV(「Google CSV 形式」)を取り込む。
//!
//! ファイル文法は通常の RFC 4180 CSV（UTF-8・カンマ区切り・`"` 引用・`""` エスケープ・
//! セル内改行可）。Google 固有なのは列スキーマで、ヘッダ名が固定（`First Name` 等）、
//! 1 セルに複数値を ` ::: ` で連結、`E-mail 1/2/3`・`Phone 1〜4` の番号付き列を持つ点。
//! UID 列は無いので、取り込んだ連絡先はどのサービスにもつながらない（重複整理は氏名＋
//! メール/電話で扱う）。

use super::contact_fields::address_is_empty;
use super::contact_labels::label_from_term;
use super::vcard::ParseResult;
use crate::models::{
    ContactAddress, ContactCustomField, ContactDate, ContactFields, ContactHandle,
    ContactOrganization, ContactRelation, ContactUrl, ContactValue, HandleKind,
};
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
            // Google CSV には uid が無い（照合はメール・電話と表示名で行う）。
            result.contacts.push(c.into());
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

fn build_contact(idx: &HashMap<String, usize>, row: &[String]) -> Option<ContactFields> {
    let cell = |key: &str| get(idx, row, key);
    let first = cell("First Name");
    let middle = cell("Middle Name");
    let last = cell("Last Name");

    let emails = labeled_values(idx, row, "E-mail", 3, |v| v.to_lowercase());
    let phones = labeled_values(idx, row, "Phone", 4, |v| v.to_string());
    let organization = ContactOrganization {
        org_id: None,
        name: non_empty(cell("Organization Name")),
        phonetic_name: non_empty(cell("Organization Phonetic Name")),
        title: non_empty(cell("Organization Title")),
        department: non_empty(cell("Organization Department")),
    };
    let organizations: Vec<ContactOrganization> = (organization != ContactOrganization::default())
        .then_some(organization)
        .into_iter()
        .collect();

    // 表示名: 氏名 → File As → 組織 → メール → 電話。
    let display_name = build_display_name(last, middle, first)
        .or_else(|| non_empty(cell("File As")))
        .or_else(|| organizations.first().and_then(|o| o.name.clone()))
        .or_else(|| emails.first().map(|e| e.value.clone()))
        .or_else(|| phones.first().map(|p| p.value.clone()))?;

    Some(ContactFields {
        display_name,
        name_prefix: non_empty(cell("Name Prefix")),
        family_name: non_empty(last),
        middle_name: non_empty(middle),
        given_name: non_empty(first),
        name_suffix: non_empty(cell("Name Suffix")),
        phonetic_family: non_empty(cell("Phonetic Last Name")),
        phonetic_middle: non_empty(cell("Phonetic Middle Name")),
        phonetic_given: non_empty(cell("Phonetic First Name")),
        nickname: non_empty(cell("Nickname")),
        birthday: non_empty(cell("Birthday")),
        note: non_empty(cell("Notes")).map(|s| s.replace("\r\n", "\n")),
        organizations,
        emails,
        phones,
        addresses: addresses(idx, row),
        urls: pairs(idx, row, "Website", 3)
            .into_iter()
            .map(|(label, value)| ContactUrl { label, value })
            .collect(),
        dates: pairs(idx, row, "Event", 3)
            .into_iter()
            .map(|(label, date)| ContactDate { label, date })
            .collect(),
        relations: pairs(idx, row, "Relation", 3)
            .into_iter()
            .map(|(label, name)| ContactRelation { label, name })
            .collect(),
        handles: handles(idx, row),
        custom_fields: (1..=5)
            .filter_map(|n| {
                let key = non_empty(cell(&format!("Custom Field {n} - Label")))?;
                let value = non_empty(cell(&format!("Custom Field {n} - Value")))?;
                Some(ContactCustomField { key, value })
            })
            .collect(),
        // Labels は ` ::: ` 区切り。Google のシステムラベル（"* myContacts" 等）は除外。
        tags: multi(cell("Labels"))
            .into_iter()
            .filter(|l| !l.starts_with('*'))
            .collect(),
        ..Default::default()
    })
}

/// Google CSV の見出し（"* Work" の `*` は主値の印）を Rondine の表記へ。
fn csv_label(raw: &str) -> Option<String> {
    label_from_term(raw.trim_start_matches('*').trim())
}

/// `{prefix} n - Label` / `{prefix} n - Value` の番号付き列から (見出し, 値) を集める
/// （値のセルは ` ::: ` で複数。同じ値は 1 つにする）。
fn pairs(
    idx: &HashMap<String, usize>,
    row: &[String],
    prefix: &str,
    count: usize,
) -> Vec<(Option<String>, String)> {
    let mut out: Vec<(Option<String>, String)> = Vec::new();
    for n in 1..=count {
        let label = csv_label(get(idx, row, &format!("{prefix} {n} - Label")));
        for v in multi(get(idx, row, &format!("{prefix} {n} - Value"))) {
            if !out.iter().any(|(_, x)| x == &v) {
                out.push((label.clone(), v));
            }
        }
    }
    out
}

/// メール・電話（番号付き列）。`norm` で値をそろえる（メールは小文字）。
fn labeled_values(
    idx: &HashMap<String, usize>,
    row: &[String],
    prefix: &str,
    count: usize,
    norm: fn(&str) -> String,
) -> Vec<ContactValue> {
    let mut out: Vec<ContactValue> = Vec::new();
    for (label, raw) in pairs(idx, row, prefix, count) {
        let value = norm(&raw);
        if !out.iter().any(|x| x.value == value) {
            out.push(ContactValue {
                label,
                value,
                is_shared: false,
            });
        }
    }
    out
}

/// 住所（Address 1..2）。各サブ項目が ` ::: ` で複数詰めなので位置で対応づけて分解する。
fn addresses(idx: &HashMap<String, usize>, row: &[String]) -> Vec<ContactAddress> {
    let mut out: Vec<ContactAddress> = Vec::new();
    for n in 1..=2 {
        let col = |sub: &str| multi_positional(get(idx, row, &format!("Address {n} - {sub}")));
        let labels = col("Label");
        let po_boxes = col("PO Box");
        let postals = col("Postal Code");
        let regions = col("Region");
        let cities = col("City");
        let streets = col("Street");
        let exts = col("Extended Address");
        let countries = col("Country");
        let cols = [
            &labels, &po_boxes, &postals, &regions, &cities, &streets, &exts, &countries,
        ];
        let count = cols.iter().map(|v| v.len()).max().unwrap_or(0);
        let at = |v: &[String], i: usize| v.get(i).and_then(|s| non_empty(s));
        for i in 0..count {
            let a = ContactAddress {
                label: labels.get(i).and_then(|l| csv_label(l)),
                po_box: at(&po_boxes, i),
                postal: at(&postals, i),
                region: at(&regions, i),
                city: at(&cities, i),
                street: at(&streets, i),
                extended: at(&exts, i),
                country: at(&countries, i),
                country_code: None,
            };
            if !address_is_empty(&a) {
                out.push(a);
            }
        }
    }
    out
}

/// チャット（IM n - Label / Service / Value）。
fn handles(idx: &HashMap<String, usize>, row: &[String]) -> Vec<ContactHandle> {
    (1..=3)
        .flat_map(|n| {
            let label = csv_label(get(idx, row, &format!("IM {n} - Label")));
            let service = non_empty(get(idx, row, &format!("IM {n} - Service")));
            multi(get(idx, row, &format!("IM {n} - Value")))
                .into_iter()
                .map(move |value| ContactHandle {
                    kind: HandleKind::Im,
                    service: service.clone(),
                    value,
                    label: label.clone(),
                })
        })
        .collect()
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

    fn parse_one(data_row: &str) -> ContactFields {
        let text = format!("{HEADER}\n{data_row}\n");
        parse(&text).contacts.into_iter().next().unwrap().fields
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
        assert_eq!(c.emails[0].value, "rabbit@key.ocn.ne.jp");
        assert_eq!(c.emails.len(), 2); // 1セル ::: の2件を保持
        assert_eq!(c.emails[1].value, "second@x.jp");
        assert_eq!(c.phones[0].value, "0997-52-4187");
        assert_eq!(c.organizations[0].name.as_deref(), Some("有限会社愛建工業"));
        assert_eq!(c.birthday.as_deref(), Some("1987-10-06"));
    }

    #[test]
    fn google_multi_address_split_by_triple_colon() {
        // Google CSV は住所も1セルに ` ::: ` で複数詰める。位置対応で複数住所に分解する。
        let header = "First Name,Last Name,Address 1 - Label,Address 1 - Postal Code,\
            Address 1 - Region,Address 1 - City,Address 1 - Street";
        let row = "太郎,山田,自宅 ::: 自宅,9050018 ::: 9050207,沖縄県 ::: 沖縄県,\
            名護市 ::: 本部町,大西1-15-5 ::: 備瀬535";
        let text = format!("{header}\n{row}\n");
        let c = parse(&text).contacts.into_iter().next().unwrap().fields;
        assert_eq!(c.addresses.len(), 2);
        assert_eq!(c.addresses[0].postal.as_deref(), Some("9050018"));
        assert_eq!(c.addresses[0].city.as_deref(), Some("名護市"));
        assert_eq!(c.addresses[1].postal.as_deref(), Some("9050207"));
        assert_eq!(c.addresses[1].city.as_deref(), Some("本部町"));
    }

    #[test]
    fn company_row_uses_org_as_name() {
        let row = ",,,,,,,,,,浦添設計研究所,,,,,,* myContacts,,,,,,,,(03) 5287-3625";
        let c = parse_one(row);
        assert_eq!(c.display_name, "浦添設計研究所");
        assert_eq!(c.phones[0].value, "(03) 5287-3625");
    }

    #[test]
    fn quoted_fields_with_comma_and_newline() {
        // Notes に改行・カンマ、氏名に空白区切り（非 CJK）。
        let row = "John,,Smith,,,,,,,,\"Acme, Inc.\",,,,\"line1\nline2\",,,,john@x.com,,,,,,";
        let text = format!("{HEADER}\n{row}\n");
        let c = parse(&text).contacts.into_iter().next().unwrap().fields;
        assert_eq!(c.display_name, "Smith John");
        assert_eq!(c.organizations[0].name.as_deref(), Some("Acme, Inc."));
        assert_eq!(c.note.as_deref(), Some("line1\nline2"));
    }

    #[test]
    fn reads_the_union_columns() {
        let header = "First Name,Last Name,Middle Name,Name Prefix,Nickname,\
            Website 1 - Label,Website 1 - Value,Event 1 - Label,Event 1 - Value,\
            Relation 1 - Label,Relation 1 - Value,IM 1 - Label,IM 1 - Service,IM 1 - Value,\
            Custom Field 1 - Label,Custom Field 1 - Value,Address 1 - PO Box,Address 1 - City,\
            Phone 1 - Label,Phone 1 - Value";
        let row = "太郎,山田,一,Dr.,たろ,Work,https://example.com,Anniversary,2010-06-01,\
            Spouse,山田花子,Home,Skype,taro,社員番号,123,私書箱1,那覇市,* Mobile,090-1111-2222";
        let c = parse(&format!("{header}\n{row}\n"))
            .contacts
            .remove(0)
            .fields;
        assert_eq!(c.middle_name.as_deref(), Some("一"));
        assert_eq!(c.name_prefix.as_deref(), Some("Dr."));
        assert_eq!(c.nickname.as_deref(), Some("たろ"));
        assert_eq!(c.urls[0].label.as_deref(), Some("職場"));
        assert_eq!(c.dates[0].label.as_deref(), Some("記念日"));
        assert_eq!(c.relations[0].label.as_deref(), Some("配偶者"));
        assert_eq!(c.handles[0].service.as_deref(), Some("Skype"));
        assert_eq!(c.custom_fields[0].value, "123");
        assert_eq!(c.addresses[0].po_box.as_deref(), Some("私書箱1"));
        assert_eq!(c.phones[0].label.as_deref(), Some("携帯"));
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
