//! 連絡先の書き込み（本体・子テーブル・組織カードとのつながり）。docs/CONTACT_MODEL.md §1・§4。
//!
//! 編集画面・ファイル取り込み・Google 同期・重複整理の統合は、どれも [`write_contact`] を通る。
//! 違うのは「会社名から組織カードをどう引くか」（[`OrgLinking`]）と「未送信の印を立てるか」だけ。

use crate::models::{ContactFields, ContactOrganization};
use crate::services::contact_fields::{is_blank, org_key, sort_name};
use crate::services::dedupe::normalize_org;
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use std::collections::HashSet;

/// 会社名から組織カードを引くときの振る舞い（docs/CONTACT_MODEL.md §1-5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OrgLinking {
    /// 編集画面から: 人が新しく入れた会社名は、カードが無ければ作る。
    Editor,
    /// 取り込み・同期・統合から: 既存のカードと正規化名が一致したときだけつなぐ（作らない）。
    ExistingOnly,
}

/// [`write_contact`] の振る舞い。
#[derive(Debug, Clone, Copy)]
pub(super) struct WriteOptions {
    /// 利用者の変更として「未送信」の印を立てる（つながっている全サービスへ送る）。
    pub mark_dirty: bool,
    pub org_linking: OrgLinking,
}

/// 連絡先の中身を書き込み、その ID を返す（`id` が None なら新規）。
///
/// 子テーブルは渡した配列で置き換える。タグは扱わない（呼び出し側が場面ごとに付け外しする）。
/// 会社は [`OrgLinking`] に従って組織カードへつなぎ、つながった会社の名前はカードの名前にそろえる。
pub(super) fn write_contact(
    conn: &Connection,
    id: Option<i64>,
    f: &ContactFields,
    opts: WriteOptions,
) -> rusqlite::Result<i64> {
    let orgs = resolve_orgs(conn, id, &f.organizations, opts.org_linking)?;
    let id = write_row(conn, id, f, opts.mark_dirty)?;
    replace_children(conn, id, f, &orgs)?;
    if opts.mark_dirty {
        mark_dirty(conn, id)?;
    }
    Ok(id)
}

/// 本体の行を書く。
fn write_row(
    conn: &Connection,
    id: Option<i64>,
    f: &ContactFields,
    mark_dirty: bool,
) -> rusqlite::Result<i64> {
    let values: Vec<Value> = vec![
        f.display_name.trim().to_string().into(),
        f.name_prefix.clone().into(),
        f.family_name.clone().into(),
        f.middle_name.clone().into(),
        f.given_name.clone().into(),
        f.name_suffix.clone().into(),
        f.phonetic_family.clone().into(),
        f.phonetic_middle.clone().into(),
        f.phonetic_given.clone().into(),
        f.nickname.clone().into(),
        f.maiden_name.clone().into(),
        sort_name(f).into(),
        f.birthday.clone().into(),
        f.note.clone().into(),
        (f.show_as_company as i64).into(),
        (f.is_favorite as i64).into(),
        (f.is_business as i64).into(),
        (f.allow_remote_images as i64).into(),
        (mark_dirty as i64).into(),
    ];
    const COLS: &str = "display_name, name_prefix, family_name, middle_name, given_name, \
         name_suffix, phonetic_family, phonetic_middle, phonetic_given, nickname, maiden_name, \
         sort_name, birthday, note, show_as_company, is_favorite, is_business, \
         allow_remote_images";
    match id {
        Some(id) => {
            let sets = COLS
                .split(',')
                .enumerate()
                .map(|(i, c)| format!("{} = ?{}", c.trim(), i + 1))
                .collect::<Vec<_>>()
                .join(", ");
            // 取り込み（mark_dirty = false）は既存の未送信の印を消さない。
            let sql = format!(
                "UPDATE contacts SET {sets}, dirty = max(dirty, ?19), \
                 updated_at = CURRENT_TIMESTAMP WHERE id = ?20"
            );
            let mut binds = values;
            binds.push(id.into());
            conn.execute(&sql, params_from_iter(binds))?;
            Ok(id)
        }
        None => {
            let holders = (1..=values.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            conn.execute(
                &format!("INSERT INTO contacts ({COLS}, dirty) VALUES ({holders})"),
                params_from_iter(values),
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// 子テーブル 1 つを、渡した行（列の値の並び）で置き換える。
fn replace_child(
    conn: &Connection,
    table: &str,
    cols: &[&str],
    id: i64,
    rows: Vec<Vec<Value>>,
) -> rusqlite::Result<()> {
    conn.execute(
        &format!("DELETE FROM {table} WHERE contact_id = ?1"),
        params![id],
    )?;
    let holders = (0..cols.len() + 2)
        .map(|i| format!("?{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT INTO {table} (contact_id, position, {}) VALUES ({holders})",
        cols.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    for (pos, row) in rows.into_iter().enumerate() {
        let mut binds: Vec<Value> = vec![id.into(), (pos as i64).into()];
        binds.extend(row);
        stmt.execute(params_from_iter(binds))?;
    }
    Ok(())
}

/// 文字列の値を trim し、空なら NULL にする。
fn text(v: &Option<String>) -> Value {
    v.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| Value::Text(s.to_string()))
        .unwrap_or(Value::Null)
}

/// 必須の文字列の値（trim 済み）。空なら None（その行は書かない）。
fn required(v: &str) -> Option<Value> {
    let t = v.trim();
    (!t.is_empty()).then(|| Value::Text(t.to_string()))
}

/// すべての子テーブルを置き換える（空の値の行は書かない）。
fn replace_children(
    conn: &Connection,
    id: i64,
    f: &ContactFields,
    orgs: &[ContactOrganization],
) -> rusqlite::Result<()> {
    let org_rows = orgs
        .iter()
        .map(|o| {
            vec![
                o.org_id.map(i64::from).into(),
                text(&o.name),
                text(&o.phonetic_name),
                text(&o.title),
                text(&o.department),
            ]
        })
        .collect();
    replace_child(
        conn,
        "contact_organizations",
        &["org_id", "name", "phonetic_name", "title", "department"],
        id,
        org_rows,
    )?;
    for (table, values) in [("contact_emails", &f.emails), ("contact_phones", &f.phones)] {
        let rows = values
            .iter()
            .filter_map(|v| {
                Some(vec![
                    text(&v.label),
                    required(&v.value)?,
                    (v.is_shared as i64).into(),
                ])
            })
            .collect();
        replace_child(conn, table, &["label", "value", "is_shared"], id, rows)?;
    }
    let addr_rows = f
        .addresses
        .iter()
        .map(|a| {
            [
                &a.label,
                &a.po_box,
                &a.postal,
                &a.region,
                &a.city,
                &a.street,
                &a.extended,
                &a.country,
                &a.country_code,
            ]
            .map(text)
            .to_vec()
        })
        // 見出し以外が全部空の住所は書かない。
        .filter(|row: &Vec<Value>| row[1..].iter().any(|v| *v != Value::Null))
        .collect();
    replace_child(
        conn,
        "contact_addresses",
        &[
            "label",
            "po_box",
            "postal",
            "region",
            "city",
            "street",
            "extended",
            "country",
            "country_code",
        ],
        id,
        addr_rows,
    )?;
    let labeled = |items: Vec<(&Option<String>, &str)>| -> Vec<Vec<Value>> {
        items
            .into_iter()
            .filter_map(|(label, v)| Some(vec![text(label), required(v)?]))
            .collect()
    };
    replace_child(
        conn,
        "contact_urls",
        &["label", "value"],
        id,
        labeled(
            f.urls
                .iter()
                .map(|u| (&u.label, u.value.as_str()))
                .collect(),
        ),
    )?;
    replace_child(
        conn,
        "contact_dates",
        &["label", "date"],
        id,
        labeled(
            f.dates
                .iter()
                .map(|d| (&d.label, d.date.as_str()))
                .collect(),
        ),
    )?;
    replace_child(
        conn,
        "contact_relations",
        &["label", "name"],
        id,
        labeled(
            f.relations
                .iter()
                .map(|r| (&r.label, r.name.as_str()))
                .collect(),
        ),
    )?;
    let handle_rows = f
        .handles
        .iter()
        .filter_map(|h| {
            Some(vec![
                h.kind.as_str().to_string().into(),
                text(&h.service),
                required(&h.value)?,
                text(&h.label),
            ])
        })
        .collect();
    replace_child(
        conn,
        "contact_handles",
        &["kind", "service", "value", "label"],
        id,
        handle_rows,
    )?;
    let custom_rows = f
        .custom_fields
        .iter()
        .filter_map(|c| Some(vec![required(&c.key)?, required(&c.value)?]))
        .collect();
    replace_child(
        conn,
        "contact_custom_fields",
        &["key", "value"],
        id,
        custom_rows,
    )
}

/// 未送信の印を立てる（本体と、つながっている全サービスのつながり）。
pub(super) fn mark_dirty(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE contacts SET dirty = 1 WHERE id = ?1", params![id])?;
    conn.execute(
        "UPDATE contact_identities SET dirty = 1 WHERE contact_id = ?1",
        params![id],
    )?;
    Ok(())
}

/// 会社を組織カードへつなぐ。空の会社は落とし、つながった会社の名前はカードの名前にそろえる。
///
/// `Editor` のときでも、すでに保存されていた（人が今回入れたのではない）会社名からはカードを
/// 作らない — 同期で入った会社名が、編集画面で保存しただけでカードになるのを防ぐため。
fn resolve_orgs(
    conn: &Connection,
    id: Option<i64>,
    orgs: &[ContactOrganization],
    linking: OrgLinking,
) -> rusqlite::Result<Vec<ContactOrganization>> {
    let stored: HashSet<String> = match (linking, id) {
        (OrgLinking::Editor, Some(id)) => stored_org_keys(conn, id)?,
        _ => HashSet::new(),
    };
    let mut out = Vec::with_capacity(orgs.len());
    for o in orgs {
        if [&o.name, &o.phonetic_name, &o.title, &o.department]
            .into_iter()
            .all(is_blank)
        {
            continue;
        }
        let mut o = o.clone();
        let card = match o.org_id {
            Some(oid) => card_by_id(conn, i64::from(oid))?,
            None => None,
        };
        let card = match card {
            Some(c) => Some(c),
            None => match o.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
                Some(name) => {
                    let create =
                        linking == OrgLinking::Editor && !stored.contains(&normalize_org(name));
                    match find_org_by_key(conn, &normalize_org(name))? {
                        Some(c) => Some(c),
                        None if create => Some(find_or_create_org(conn, name)?),
                        None => None,
                    }
                }
                None => None,
            },
        };
        match card {
            Some((oid, name)) => {
                o.org_id = Some(oid as i32);
                o.name = Some(name);
            }
            None => o.org_id = None,
        }
        out.push(o);
    }
    Ok(out)
}

/// 保存済みの会社名（正規化名）。
fn stored_org_keys(conn: &Connection, id: i64) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT name FROM contact_organizations WHERE contact_id = ?1")?;
    let rows = stmt.query_map(params![id], |r| r.get::<_, Option<String>>(0))?;
    let mut out = HashSet::new();
    for name in rows {
        let key = ContactOrganization {
            name: name?,
            ..Default::default()
        };
        out.insert(org_key(&key));
    }
    Ok(out)
}

/// ID で組織カードを引く（ゴミ箱にあれば戻す）。無ければ None。
fn card_by_id(conn: &Connection, oid: i64) -> rusqlite::Result<Option<(i64, String)>> {
    let name: Option<String> = conn
        .query_row(
            "SELECT name FROM organizations WHERE id = ?1",
            params![oid],
            |r| r.get(0),
        )
        .optional()?;
    if name.is_some() {
        // 削除済みのカードに人をつなぐなら復活させる（ゴミ箱に所属者を残さない）。
        conn.execute(
            "UPDATE organizations SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL",
            params![oid],
        )?;
    }
    Ok(name.map(|n| (oid, n)))
}

/// 正規化名が一致する（削除されていない）組織カードを引く。複数あれば最も古いもの。
pub(super) fn find_org_by_key(
    conn: &Connection,
    key: &str,
) -> rusqlite::Result<Option<(i64, String)>> {
    if key.is_empty() {
        return Ok(None);
    }
    let mut stmt =
        conn.prepare("SELECT id, name FROM organizations WHERE deleted_at IS NULL ORDER BY id")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (id, name) = row?;
        if normalize_org(&name) == key {
            return Ok(Some((id, name)));
        }
    }
    Ok(None)
}

/// 組織名と完全に同じカードを引き、無ければ作る（ゴミ箱にあれば戻して使う）。
pub(super) fn find_or_create_org(conn: &Connection, name: &str) -> rusqlite::Result<(i64, String)> {
    let name = name.trim();
    let found: Option<i64> = conn
        .query_row(
            "SELECT id FROM organizations WHERE name = ?1",
            params![name],
            |r| r.get(0),
        )
        .optional()?;
    let id = match found {
        Some(id) => {
            conn.execute(
                "UPDATE organizations SET deleted_at = NULL WHERE id = ?1",
                params![id],
            )?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO organizations (name) VALUES (?1)",
                params![name],
            )?;
            conn.last_insert_rowid()
        }
    };
    Ok((id, name.to_string()))
}
