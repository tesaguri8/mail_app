//! 送信: 連絡先の中身（[`ContactFields`]）→ People API の書き込み本文。docs/CONTACT_MODEL.md §3。
//!
//! Google の更新は項目ごと（`names` / `organizations` …）の丸ごと置き換え。Rondine が持たない
//! 部分（将来足される項目、Google の内部的な付帯情報）を消さないため、**送る直前に読み直した
//! Person を土台にし、Rondine が扱う部分だけを上書きする。**
//!
//! - 土台の各要素は、同じ値（メールは小文字・電話は数字・会社は正規化名…）の要素に引き継ぐ。
//!   Rondine が知らないキー（会社の `location` など）はそのまま残る
//! - 読み取り専用・メタ情報（`metadata` / `displayName` / `formattedType` …）は送らない
//! - Rondine 固有の属性（共有の印・組織カードへのつながり・取引先・外部画像許可）は送らない

use crate::models::ContactFields;
use crate::services::contact_fields::org_key;
use crate::services::contact_labels::google_type_for;
use crate::services::dedupe::{digits, fold, normalize_org};
use serde_json::{json, Map, Value};

/// 要素から取り除く読み取り専用・派生のキー（土台を引き継ぐときに落とす）。
const READ_ONLY_KEYS: &[&str] = &[
    "metadata",
    "formattedType",
    "formattedProtocol",
    "canonicalForm",
    // 住所の構造化した値から組み立てられる派生値。古いまま残すと構造化した値と食い違う。
    "formattedValue",
];

/// 名前で取り除くキー（姓名から組み立てられる表示用の値）。
const NAME_DERIVED_KEYS: &[&str] = &[
    "displayName",
    "displayNameLastFirst",
    "unstructuredName",
    "phoneticFullName",
];

/// 送る 1 要素: 照合キーと、Rondine が扱うキーの値（None はそのキーを消す）。
struct Entry {
    key: String,
    values: Vec<(&'static str, Option<Value>)>,
}

fn text(v: &Option<String>) -> Option<Value> {
    v.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| Value::String(s.to_string()))
}

fn s(v: &str) -> Option<Value> {
    text(&Some(v.to_string()))
}

fn kind(label: &Option<String>) -> Option<Value> {
    google_type_for(label.as_deref()).map(Value::String)
}

/// 土台の要素から読み取り専用のキー（と `extra`）を落とした写し。
fn cleaned(base: &Value, extra: &[&str]) -> Map<String, Value> {
    let mut m = base.as_object().cloned().unwrap_or_default();
    for k in READ_ONLY_KEYS.iter().chain(extra) {
        m.remove(*k);
    }
    m
}

/// 土台の配列（無ければ空）。
fn base_list(base: Option<&Map<String, Value>>, field: &str) -> Vec<Value> {
    base.and_then(|b| b.get(field))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// 土台の要素の照合キー（`field` の文字列値を `norm` で畳んだもの）。
fn base_key(v: &Value, field: &str, norm: fn(&str) -> String) -> String {
    v.get(field)
        .and_then(Value::as_str)
        .map(norm)
        .unwrap_or_default()
}

/// 送る要素の列を、土台の要素を引き継ぎながら組み立てる。
///
/// 照合キーが同じ土台の要素を引き継ぎ、`positional` なら見つからないとき同じ位置の（まだ
/// 使っていない）要素を引き継ぐ。どれにも当たらなければ新しい要素にする。
fn merge_list(
    base: &[Value],
    base_key_of: impl Fn(&Value) -> String,
    entries: Vec<Entry>,
    positional: bool,
) -> Value {
    let mut used = vec![false; base.len()];
    let keys: Vec<String> = base.iter().map(base_key_of).collect();
    let out = entries
        .into_iter()
        .enumerate()
        .map(|(i, e)| {
            let hit = keys
                .iter()
                .enumerate()
                .position(|(j, k)| !used[j] && !e.key.is_empty() && *k == e.key)
                .or_else(|| (positional && i < base.len() && !used[i]).then_some(i));
            let mut m = match hit {
                Some(j) => {
                    used[j] = true;
                    cleaned(&base[j], &[])
                }
                None => Map::new(),
            };
            for (k, v) in e.values {
                match v {
                    Some(v) => m.insert(k.to_string(), v),
                    None => m.remove(k),
                };
            }
            Value::Object(m)
        })
        .collect();
    Value::Array(out)
}

/// 1 つの値の項目（名前・ニックネーム・メモ・誕生日）。土台の先頭だけを置き換え、2 つ目以降は
/// 残す。`extra` は落とすキー（読み取り専用のキーに加えて）。2 つ目以降の要素からも同じキーを
/// 落とす（`metadata` などを付けたまま送り返さない）。
fn merge_single(
    base: &[Value],
    first: Option<Vec<(&'static str, Option<Value>)>>,
    extra: &[&str],
) -> Value {
    let rest = base
        .iter()
        .skip(1)
        .map(|b| Value::Object(cleaned(b, extra)));
    let head = first.map(|values| {
        let mut m = base.first().map(|b| cleaned(b, extra)).unwrap_or_default();
        for (k, v) in values {
            match v {
                Some(v) => m.insert(k.to_string(), v),
                None => m.remove(k),
            };
        }
        Value::Object(m)
    });
    Value::Array(head.into_iter().chain(rest).collect())
}

/// 住所録の日付表記（`YYYY-MM-DD` / 年なしの `--MM-DD`）を People API の Date へ。
fn date_value(raw: &str) -> Option<Value> {
    let b = raw.trim();
    let num = |s: &str| s.parse::<u32>().ok().filter(|n| *n > 0);
    if let Some(md) = b.strip_prefix("--") {
        let (m, d) = md.split_once('-')?;
        return Some(json!({ "month": num(m)?, "day": num(d)? }));
    }
    let mut parts = b.splitn(3, '-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    Some(json!({ "year": y.parse::<i32>().ok()?, "month": num(m)?, "day": num(d)? }))
}

/// 誕生日: 日付として読めれば `date`、読めなければ原文を `text` で送る。
fn birthday_values(raw: &str) -> Vec<(&'static str, Option<Value>)> {
    match date_value(raw) {
        Some(d) => vec![("date", Some(d)), ("text", None)],
        None => vec![("date", None), ("text", s(raw))],
    }
}

/// IM のサービス名を People API の protocol へ。既知の綴りは機械可読な名前にそろえる。
fn im_protocol(service: &str) -> String {
    const KNOWN: [&str; 9] = [
        "aim",
        "msn",
        "yahoo",
        "skype",
        "qq",
        "googleTalk",
        "icq",
        "jabber",
        "netMeeting",
    ];
    let t = service.trim();
    KNOWN
        .iter()
        .find(|k| k.eq_ignore_ascii_case(t))
        .map(|k| k.to_string())
        .unwrap_or_else(|| t.to_string())
}

/// 名前。People API の displayName は読み取り専用なので姓名に分けて送る。姓名を持たない
/// （表示名だけの）連絡先は表示名を姓に入れる（日本語の氏名は姓が先なので並びが崩れない）。
fn names(f: &ContactFields, base: &[Value]) -> Value {
    let (family, given) = match (text(&f.family_name), text(&f.given_name)) {
        (None, None) => (s(&f.display_name), None),
        pair => pair,
    };
    let values = vec![
        ("honorificPrefix", text(&f.name_prefix)),
        ("familyName", family),
        ("middleName", text(&f.middle_name)),
        ("givenName", given),
        ("honorificSuffix", text(&f.name_suffix)),
        ("phoneticFamilyName", text(&f.phonetic_family)),
        ("phoneticMiddleName", text(&f.phonetic_middle)),
        ("phoneticGivenName", text(&f.phonetic_given)),
    ];
    // 連絡先の names は 1 つだけ持てる。主の要素を土台にする。
    let primary = base
        .iter()
        .find(|n| n.pointer("/metadata/primary") == Some(&Value::Bool(true)))
        .or_else(|| base.first())
        .cloned();
    merge_single(
        &primary.into_iter().collect::<Vec<_>>(),
        Some(values),
        NAME_DERIVED_KEYS,
    )
}

/// 送ろうとしている内容が、Google から最後に読んだ内容（台帳の snapshot）と同じか。同じなら
/// 送る必要が無い（統合で「残した 1 件を更新」が大量に溜まるが、中身はほとんど同じ）。
///
/// 送信用に組み立てた形（[`person_body`]）どうしで比べるので、Google へ送らない Rondine 固有の
/// 項目（取引先フラグ・外部画像許可）は自然に比較から外れる。ラベル（タグ）とお気に入りは
/// 本文とは別の経路（`members:modify`）で送るので、それも比べる（タグは集合で）。
pub fn same_as_google(local: &ContactFields, snapshot: &ContactFields) -> bool {
    let tags = |f: &ContactFields| {
        f.tags
            .iter()
            .map(|t| t.trim().to_string())
            .collect::<std::collections::BTreeSet<_>>()
    };
    person_body(local, None) == person_body(snapshot, None)
        && local.is_favorite == snapshot.is_favorite
        && tags(local) == tags(snapshot)
}

/// 連絡先 1 件を People API の書き込み本文にする。
///
/// `base` は送る直前に読み直した Person（作成のときは None）。その etag を本文に入れる
/// （更新には読んだ版の etag が要る）。`updatePersonFields`（[`super::WRITE_PERSON_FIELDS`]）に
/// 挙げた項目は空でもキーを必ず入れる（ローカルで消した項目が Google 側でも消えるように）。
pub fn person_body(f: &ContactFields, base: Option<&Value>) -> Value {
    let b = base.and_then(Value::as_object);
    let list = |field: &str| base_list(b, field);
    let lower = |v: &str| fold(v).trim().to_string();
    let mut body = Map::new();
    body.insert("names".into(), names(f, &list("names")));
    body.insert(
        "nicknames".into(),
        merge_single(
            &list("nicknames"),
            text(&f.nickname).map(|v| vec![("value", Some(v))]),
            &[],
        ),
    );
    body.insert(
        "biographies".into(),
        merge_single(
            &list("biographies"),
            text(&f.note).map(|v| vec![("value", Some(v)), ("contentType", s("TEXT_PLAIN"))]),
            &[],
        ),
    );
    body.insert(
        "birthdays".into(),
        merge_single(
            &list("birthdays"),
            f.birthday
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .map(birthday_values),
            &[],
        ),
    );
    let typed = |values: &[crate::models::ContactValue], norm: fn(&str) -> String| -> Vec<Entry> {
        values
            .iter()
            .filter(|v| !v.value.trim().is_empty())
            .map(|v| Entry {
                key: norm(&v.value),
                values: vec![("value", s(&v.value)), ("type", kind(&v.label))],
            })
            .collect()
    };
    body.insert(
        "emailAddresses".into(),
        merge_list(
            &list("emailAddresses"),
            |v| base_key(v, "value", lower),
            typed(&f.emails, lower),
            false,
        ),
    );
    body.insert(
        "phoneNumbers".into(),
        merge_list(
            &list("phoneNumbers"),
            |v| base_key(v, "value", digits),
            typed(&f.phones, digits),
            false,
        ),
    );
    let addresses = f
        .addresses
        .iter()
        .map(|a| Entry {
            key: String::new(),
            values: vec![
                ("poBox", text(&a.po_box)),
                ("postalCode", text(&a.postal)),
                ("region", text(&a.region)),
                ("city", text(&a.city)),
                ("streetAddress", text(&a.street)),
                ("extendedAddress", text(&a.extended)),
                ("country", text(&a.country)),
                ("countryCode", text(&a.country_code)),
                ("type", kind(&a.label)),
            ],
        })
        .collect();
    body.insert(
        "addresses".into(),
        merge_list(&list("addresses"), |_| String::new(), addresses, true),
    );
    let orgs = f
        .organizations
        .iter()
        .map(|o| Entry {
            key: org_key(o),
            values: vec![
                ("name", text(&o.name)),
                ("phoneticName", text(&o.phonetic_name)),
                ("title", text(&o.title)),
                ("department", text(&o.department)),
            ],
        })
        .collect();
    body.insert(
        "organizations".into(),
        merge_list(
            &list("organizations"),
            |v| base_key(v, "name", normalize_org),
            orgs,
            true,
        ),
    );
    body.insert("urls".into(), other_lists::urls(f, &list("urls")));
    body.insert("events".into(), other_lists::events(f, &list("events")));
    body.insert(
        "relations".into(),
        other_lists::relations(f, &list("relations")),
    );
    body.insert(
        "imClients".into(),
        other_lists::im_clients(f, &list("imClients")),
    );
    body.insert(
        "userDefined".into(),
        other_lists::user_defined(f, &list("userDefined")),
    );
    if let Some(etag) = b.and_then(|b| b.get("etag")) {
        body.insert("etag".into(), etag.clone());
    }
    Value::Object(body)
}

/// URL・日付・関係・チャット・カスタム項目。
mod other_lists {
    use super::{base_key, date_value, im_protocol, kind, merge_list, s, text, Entry};
    use crate::models::{ContactFields, HandleKind};
    use crate::services::dedupe::fold;
    use serde_json::Value;

    fn same(v: &str) -> String {
        v.trim().to_string()
    }

    fn lower(v: &str) -> String {
        fold(v).trim().to_string()
    }

    pub(super) fn urls(f: &ContactFields, base: &[Value]) -> Value {
        let entries = f
            .urls
            .iter()
            .filter(|u| !u.value.trim().is_empty())
            .map(|u| Entry {
                key: same(&u.value),
                values: vec![("value", s(&u.value)), ("type", kind(&u.label))],
            })
            .collect();
        merge_list(base, |v| base_key(v, "value", same), entries, false)
    }

    pub(super) fn events(f: &ContactFields, base: &[Value]) -> Value {
        // 日付として読めないものは送れない（events は date が必須）。
        let entries = f
            .dates
            .iter()
            .filter_map(|d| {
                let date = date_value(&d.date)?;
                Some(Entry {
                    key: date.to_string(),
                    values: vec![("date", Some(date)), ("type", kind(&d.label))],
                })
            })
            .collect();
        merge_list(
            base,
            |v| v.get("date").map(|d| d.to_string()).unwrap_or_default(),
            entries,
            false,
        )
    }

    pub(super) fn relations(f: &ContactFields, base: &[Value]) -> Value {
        let entries = f
            .relations
            .iter()
            .filter(|r| !r.name.trim().is_empty())
            .map(|r| Entry {
                key: lower(&r.name),
                values: vec![("person", s(&r.name)), ("type", kind(&r.label))],
            })
            .collect();
        merge_list(base, |v| base_key(v, "person", lower), entries, false)
    }

    /// チャットだけを送る（SNS のハンドルは Google に対応する項目が無い）。
    pub(super) fn im_clients(f: &ContactFields, base: &[Value]) -> Value {
        let entries = f
            .handles
            .iter()
            .filter(|h| h.kind == HandleKind::Im && !h.value.trim().is_empty())
            .map(|h| Entry {
                key: lower(&h.value),
                values: vec![
                    ("username", s(&h.value)),
                    (
                        "protocol",
                        h.service.as_deref().map(im_protocol).and_then(|p| s(&p)),
                    ),
                    ("type", kind(&h.label)),
                ],
            })
            .collect();
        merge_list(base, |v| base_key(v, "username", lower), entries, false)
    }

    pub(super) fn user_defined(f: &ContactFields, base: &[Value]) -> Value {
        let entries = f
            .custom_fields
            .iter()
            .filter(|c| !c.key.trim().is_empty() && !c.value.trim().is_empty())
            .map(|c| Entry {
                key: same(&c.key),
                values: vec![("key", s(&c.key)), ("value", text(&Some(c.value.clone())))],
            })
            .collect();
        merge_list(base, |v| base_key(v, "key", same), entries, false)
    }
}

#[cfg(test)]
mod tests;
