//! 連絡先の書き込み（本体・子テーブル・組織とのつながり）。docs/CONTACT_MODEL.md §1・
//! docs/ORG_CONTACTS.md。
//!
//! 編集画面・ファイル取り込み・Google 同期・重複整理の統合は、どれも [`write_contact`] を通る。
//! 違うのは「会社名から組織の連絡先をどう引くか」（[`OrgLinking`]）と「未送信の印を立てるか」だけ。
//! 組織の連絡先（`kind = organization`）の先頭の会社は自分の会社の行で、どこにもつながない。

use super::organizations::sync_member_names;
use crate::models::{ContactFields, ContactKind, ContactOrganization};
use crate::services::contact_fields::{display_name_of, is_blank, org_key, sort_name};
use crate::services::dedupe::normalize_org;
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use std::collections::HashSet;

/// 会社名から組織の連絡先を引くときの振る舞い（docs/CONTACT_MODEL.md §1-5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OrgLinking {
    /// 編集画面から: 人が新しく入れた会社名は、組織が無ければ作る（Rondine だけの組織）。
    Editor,
    /// 取り込み・同期・統合から: 既存の組織と正規化名が一致したときだけつなぐ（作らない）。
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
/// 会社は [`OrgLinking`] に従って組織の連絡先へつなぎ、つながった会社の名前は組織の名前にそろえる。
pub(super) fn write_contact(
    conn: &Connection,
    id: Option<i64>,
    f: &ContactFields,
    opts: WriteOptions,
) -> rusqlite::Result<i64> {
    let orgs = resolve_orgs(conn, id, f, opts.org_linking)?;
    let id = write_row(conn, id, f, opts.mark_dirty)?;
    replace_children(conn, id, f, &orgs)?;
    if opts.mark_dirty {
        mark_dirty(conn, id)?;
    }
    match f.kind {
        // 組織の名前が変わったら、つながっている人の会社名もそろえる（変わった人は送り直す）。
        ContactKind::Organization => sync_member_names(conn, id)?,
        // 人に戻した連絡先には、もう誰もつながらない（会社名は文字として残る）。
        ContactKind::Person => {
            conn.execute(
                "UPDATE contact_organizations SET org_id = NULL WHERE org_id = ?1",
                params![id],
            )?;
        }
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
        display_name_of(f).into(),
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
        f.kind.as_db().to_string().into(),
        (f.is_favorite as i64).into(),
        (f.is_business as i64).into(),
        (f.allow_remote_images as i64).into(),
        (mark_dirty as i64).into(),
    ];
    const COLS: &str = "display_name, name_prefix, family_name, middle_name, given_name, \
         name_suffix, phonetic_family, phonetic_middle, phonetic_given, nickname, maiden_name, \
         sort_name, birthday, note, kind, is_favorite, is_business, \
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

/// 会社を組織の連絡先へつなぐ。空の会社は落とし、つながった会社の名前は組織の名前にそろえる。
///
/// 組織の連絡先の先頭の会社は自分の会社の行なので、どこにもつながない（自分や同名の組織を指さない）。
/// `Editor` のときでも、すでに保存されていた（人が今回入れたのではない）会社名からは組織を
/// 作らない — 同期で入った会社名が、編集画面で保存しただけで組織になるのを防ぐため。
fn resolve_orgs(
    conn: &Connection,
    id: Option<i64>,
    f: &ContactFields,
    linking: OrgLinking,
) -> rusqlite::Result<Vec<ContactOrganization>> {
    let stored: HashSet<String> = match (linking, id) {
        (OrgLinking::Editor, Some(id)) => stored_org_keys(conn, id)?,
        _ => HashSet::new(),
    };
    let mut out = Vec::with_capacity(f.organizations.len());
    for (i, o) in f.organizations.iter().enumerate() {
        if [&o.name, &o.phonetic_name, &o.title, &o.department]
            .into_iter()
            .all(is_blank)
        {
            continue;
        }
        let mut o = o.clone();
        if i == 0 && f.kind == ContactKind::Organization {
            o.org_id = None;
            out.push(o);
            continue;
        }
        let card = match o.org_id {
            Some(oid) => org_by_id(conn, i64::from(oid), id)?,
            None => None,
        };
        let card = match card {
            Some(c) => Some(c),
            None => match o.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
                Some(name) => {
                    let create =
                        linking == OrgLinking::Editor && !stored.contains(&normalize_org(name));
                    match find_org_by_key(conn, &normalize_org(name))? {
                        Some(c) if Some(c.0) != id => Some(c),
                        Some(_) => None,
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

/// ID で組織の連絡先を引く（ゴミ箱にあれば戻す）。組織でない・自分自身なら None。
fn org_by_id(
    conn: &Connection,
    oid: i64,
    self_id: Option<i64>,
) -> rusqlite::Result<Option<(i64, String)>> {
    if Some(oid) == self_id {
        return Ok(None);
    }
    let name: Option<String> = conn
        .query_row(
            "SELECT display_name FROM contacts WHERE id = ?1 AND kind = 'organization'",
            params![oid],
            |r| r.get(0),
        )
        .optional()?;
    if name.is_some() {
        // ゴミ箱の組織に人をつなぐなら戻す（ゴミ箱に所属者を残さない）。
        conn.execute(
            "UPDATE contacts SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL",
            params![oid],
        )?;
    }
    Ok(name.map(|n| (oid, n)))
}

/// 正規化名が一致する（ゴミ箱にない）組織の連絡先を引く。複数あれば最も古いもの。
pub(super) fn find_org_by_key(
    conn: &Connection,
    key: &str,
) -> rusqlite::Result<Option<(i64, String)>> {
    if key.is_empty() {
        return Ok(None);
    }
    let mut stmt = conn.prepare(
        "SELECT id, display_name FROM contacts \
         WHERE kind = 'organization' AND deleted_at IS NULL ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (id, name) = row?;
        if normalize_org(&name) == key {
            return Ok(Some((id, name)));
        }
    }
    Ok(None)
}

/// 組織名と完全に同じ組織の連絡先を引き、無ければ作る（ゴミ箱にあれば戻して使う）。
///
/// 作る組織は Rondine だけ（同期先は無し。利用者が組織ごとに選ぶ / docs/ORG_CONTACTS.md 要判断 D）。
pub(super) fn find_or_create_org(conn: &Connection, name: &str) -> rusqlite::Result<(i64, String)> {
    let name = name.trim();
    let found: Option<i64> = conn
        .query_row(
            "SELECT id FROM contacts WHERE kind = 'organization' AND display_name = ?1 \
             ORDER BY deleted_at IS NOT NULL, id LIMIT 1",
            params![name],
            |r| r.get(0),
        )
        .optional()?;
    let id = match found {
        Some(id) => {
            conn.execute(
                "UPDATE contacts SET deleted_at = NULL WHERE id = ?1",
                params![id],
            )?;
            id
        }
        None => {
            let org = ContactFields {
                display_name: name.to_string(),
                kind: ContactKind::Organization,
                organizations: vec![ContactOrganization {
                    name: Some(name.to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            };
            let id = write_row(conn, None, &org, false)?;
            replace_children(conn, id, &org, &org.organizations)?;
            id
        }
    };
    Ok((id, name.to_string()))
}
