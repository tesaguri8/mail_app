//! 連絡先の読み出し（本体の行・子テーブル・タグ・つながり）。docs/CONTACT_MODEL.md §1。
//!
//! 一覧は本体の行に主値（position = 0）の写しとつながりだけを添えて軽く返し、詳細は子テーブルを
//! すべて充填する。重複整理のように全員の中身が要る場面では、子テーブルをまとめて読む
//! （1 人ずつ引くと件数ぶんの問い合わせになるため）。

use crate::models::{
    ContactAddress, ContactCustomField, ContactDate, ContactFields, ContactHandle, ContactLink,
    ContactOrganization, ContactProvider, ContactRelation, ContactSummary, ContactUrl,
    ContactValue, HandleKind,
};
use rusqlite::{params, Connection, Row, ToSql};
use std::collections::HashMap;

/// 一覧・詳細で読む本体の列（`c` は contacts の別名）。列順は [`summary_from_row`] と対応。
pub(super) const SUMMARY_COLS: &str = "c.id, c.display_name, c.name_prefix, c.family_name, \
     c.middle_name, c.given_name, c.name_suffix, c.phonetic_family, c.phonetic_middle, \
     c.phonetic_given, c.nickname, c.maiden_name, c.birthday, c.note, c.show_as_company, \
     c.is_favorite, c.is_business, c.allow_remote_images, c.sort_name, c.avatar_path, \
     c.deleted_at, \
     (SELECT value FROM contact_emails WHERE contact_id = c.id ORDER BY position, id LIMIT 1), \
     (SELECT value FROM contact_phones WHERE contact_id = c.id ORDER BY position, id LIMIT 1), \
     (SELECT name FROM contact_organizations WHERE contact_id = c.id \
      ORDER BY position, id LIMIT 1)";

/// 一覧の並び（お気に入り → よみ → 表示名）。
pub(super) const SUMMARY_ORDER: &str =
    "ORDER BY c.is_favorite DESC, c.sort_name COLLATE NOCASE, c.display_name COLLATE NOCASE";

/// 本体の 1 行を一覧用の [`ContactSummary`] に写す（子テーブル・タグ・つながりは空）。
fn summary_from_row(r: &Row) -> rusqlite::Result<ContactSummary> {
    let flag = |i: usize| -> rusqlite::Result<bool> { Ok(r.get::<_, i64>(i)? != 0) };
    Ok(ContactSummary {
        id: r.get::<_, i64>(0)? as i32,
        fields: ContactFields {
            display_name: r.get(1)?,
            name_prefix: r.get(2)?,
            family_name: r.get(3)?,
            middle_name: r.get(4)?,
            given_name: r.get(5)?,
            name_suffix: r.get(6)?,
            phonetic_family: r.get(7)?,
            phonetic_middle: r.get(8)?,
            phonetic_given: r.get(9)?,
            nickname: r.get(10)?,
            maiden_name: r.get(11)?,
            birthday: r.get(12)?,
            note: r.get(13)?,
            show_as_company: flag(14)?,
            is_favorite: flag(15)?,
            is_business: flag(16)?,
            allow_remote_images: flag(17)?,
            ..Default::default()
        },
        sort_name: r.get(18)?,
        avatar_path: r.get(19)?,
        deleted_at: r.get(20)?,
        primary_email: r.get(21)?,
        primary_phone: r.get(22)?,
        primary_organization: r.get(23)?,
        links: Vec::new(),
    })
}

/// 本体を条件つきで読み、つながりを添えた一覧用の連絡先を返す。
///
/// `filter` は `WHERE` 以降（`c` は contacts の別名。空なら全件）、`order` は並び。
pub(super) fn query_summaries(
    conn: &Connection,
    filter: &str,
    order: &str,
    binds: &[&dyn ToSql],
) -> rusqlite::Result<Vec<ContactSummary>> {
    let where_sql = if filter.is_empty() {
        String::new()
    } else {
        format!("WHERE {filter}")
    };
    let sql = format!("SELECT {SUMMARY_COLS} FROM contacts c {where_sql} {order}");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows: Vec<ContactSummary> = stmt
        .query_map(binds, summary_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    let mut links = load_links(conn, Scope::All)?;
    for c in &mut rows {
        c.links = links.remove(&(c.id as i64)).unwrap_or_default();
    }
    Ok(rows)
}

/// 1 人の連絡先を、子テーブル・タグ・つながりまで充填して返す。
pub(super) fn load_contact(conn: &Connection, id: i64) -> rusqlite::Result<ContactSummary> {
    let sql = format!("SELECT {SUMMARY_COLS} FROM contacts c WHERE c.id = ?1");
    let mut c = conn.query_row(&sql, params![id], summary_from_row)?;
    let mut children = Children::load(conn, Scope::One(id))?;
    children.apply(id, &mut c.fields);
    c.links = load_links(conn, Scope::One(id))?
        .remove(&id)
        .unwrap_or_default();
    Ok(c)
}

/// 削除済みを除く全員を、子テーブル・タグまで充填して返す（重複整理・照合用）。
pub(super) fn load_all_full(conn: &Connection) -> rusqlite::Result<Vec<ContactSummary>> {
    let mut rows = query_summaries(conn, "c.deleted_at IS NULL", SUMMARY_ORDER, &[])?;
    let mut children = Children::load(conn, Scope::All)?;
    for c in &mut rows {
        children.apply(c.id as i64, &mut c.fields);
    }
    Ok(rows)
}

/// 子テーブルを読む範囲。
#[derive(Clone, Copy)]
pub(super) enum Scope {
    One(i64),
    All,
}

impl Scope {
    /// `contact_id` 列（`col`）の条件と束縛値。
    fn filter(self, col: &str) -> (String, Option<i64>) {
        match self {
            Scope::One(id) => (format!("WHERE {col} = ?1"), Some(id)),
            Scope::All => (String::new(), None),
        }
    }
}

/// 子テーブル 1 つを (contact_id → 並び順の値) に読む。`map` は 1 列目以降を読む。
fn load_child<T>(
    conn: &Connection,
    table: &str,
    cols: &str,
    scope: Scope,
    map: impl Fn(&Row) -> rusqlite::Result<T>,
) -> rusqlite::Result<HashMap<i64, Vec<T>>> {
    let (where_sql, bind) = scope.filter("contact_id");
    let sql = format!(
        "SELECT contact_id, {cols} FROM {table} {where_sql} ORDER BY contact_id, position, id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let binds: Vec<&dyn ToSql> = bind.iter().map(|b| b as &dyn ToSql).collect();
    let mut out: HashMap<i64, Vec<T>> = HashMap::new();
    let rows = stmt.query_map(binds.as_slice(), |r| Ok((r.get::<_, i64>(0)?, map(r)?)))?;
    for row in rows {
        let (cid, v) = row?;
        out.entry(cid).or_default().push(v);
    }
    Ok(out)
}

/// 子テーブルとタグをまとめて読んだもの。
struct Children {
    organizations: HashMap<i64, Vec<ContactOrganization>>,
    emails: HashMap<i64, Vec<ContactValue>>,
    phones: HashMap<i64, Vec<ContactValue>>,
    addresses: HashMap<i64, Vec<ContactAddress>>,
    urls: HashMap<i64, Vec<ContactUrl>>,
    dates: HashMap<i64, Vec<ContactDate>>,
    relations: HashMap<i64, Vec<ContactRelation>>,
    handles: HashMap<i64, Vec<ContactHandle>>,
    custom_fields: HashMap<i64, Vec<ContactCustomField>>,
    tags: HashMap<i64, Vec<String>>,
}

impl Children {
    fn load(conn: &Connection, scope: Scope) -> rusqlite::Result<Self> {
        let value = |r: &Row| -> rusqlite::Result<ContactValue> {
            Ok(ContactValue {
                label: r.get(1)?,
                value: r.get(2)?,
                is_shared: r.get::<_, i64>(3)? != 0,
            })
        };
        Ok(Self {
            organizations: load_child(
                conn,
                "contact_organizations",
                "org_id, name, phonetic_name, title, department",
                scope,
                |r| {
                    Ok(ContactOrganization {
                        org_id: r.get::<_, Option<i64>>(1)?.map(|v| v as i32),
                        name: r.get(2)?,
                        phonetic_name: r.get(3)?,
                        title: r.get(4)?,
                        department: r.get(5)?,
                    })
                },
            )?,
            emails: load_child(
                conn,
                "contact_emails",
                "label, value, is_shared",
                scope,
                value,
            )?,
            phones: load_child(
                conn,
                "contact_phones",
                "label, value, is_shared",
                scope,
                value,
            )?,
            addresses: load_child(
                conn,
                "contact_addresses",
                "label, po_box, postal, region, city, street, extended, country, country_code",
                scope,
                |r| {
                    Ok(ContactAddress {
                        label: r.get(1)?,
                        po_box: r.get(2)?,
                        postal: r.get(3)?,
                        region: r.get(4)?,
                        city: r.get(5)?,
                        street: r.get(6)?,
                        extended: r.get(7)?,
                        country: r.get(8)?,
                        country_code: r.get(9)?,
                    })
                },
            )?,
            urls: load_child(conn, "contact_urls", "label, value", scope, |r| {
                Ok(ContactUrl {
                    label: r.get(1)?,
                    value: r.get(2)?,
                })
            })?,
            dates: load_child(conn, "contact_dates", "label, date", scope, |r| {
                Ok(ContactDate {
                    label: r.get(1)?,
                    date: r.get(2)?,
                })
            })?,
            relations: load_child(conn, "contact_relations", "label, name", scope, |r| {
                Ok(ContactRelation {
                    label: r.get(1)?,
                    name: r.get(2)?,
                })
            })?,
            handles: load_child(
                conn,
                "contact_handles",
                "kind, service, value, label",
                scope,
                |r| {
                    Ok(ContactHandle {
                        kind: HandleKind::from_db(&r.get::<_, String>(1)?),
                        service: r.get(2)?,
                        value: r.get(3)?,
                        label: r.get(4)?,
                    })
                },
            )?,
            custom_fields: load_child(conn, "contact_custom_fields", "key, value", scope, |r| {
                Ok(ContactCustomField {
                    key: r.get(1)?,
                    value: r.get(2)?,
                })
            })?,
            tags: load_tags(conn, scope)?,
        })
    }

    /// 読んだ値のうち `id` の分を `f` へ移す。
    fn apply(&mut self, id: i64, f: &mut ContactFields) {
        f.organizations = self.organizations.remove(&id).unwrap_or_default();
        f.emails = self.emails.remove(&id).unwrap_or_default();
        f.phones = self.phones.remove(&id).unwrap_or_default();
        f.addresses = self.addresses.remove(&id).unwrap_or_default();
        f.urls = self.urls.remove(&id).unwrap_or_default();
        f.dates = self.dates.remove(&id).unwrap_or_default();
        f.relations = self.relations.remove(&id).unwrap_or_default();
        f.handles = self.handles.remove(&id).unwrap_or_default();
        f.custom_fields = self.custom_fields.remove(&id).unwrap_or_default();
        f.tags = self.tags.remove(&id).unwrap_or_default();
    }
}

/// タグ名（メールと共通の tags）を名前順に読む。
fn load_tags(conn: &Connection, scope: Scope) -> rusqlite::Result<HashMap<i64, Vec<String>>> {
    let (where_sql, bind) = scope.filter("ct.contact_id");
    let sql = format!(
        "SELECT ct.contact_id, t.name FROM contact_tags ct JOIN tags t ON t.id = ct.tag_id \
         {where_sql} ORDER BY t.name COLLATE NOCASE"
    );
    let mut stmt = conn.prepare(&sql)?;
    let binds: Vec<&dyn ToSql> = bind.iter().map(|b| b as &dyn ToSql).collect();
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    let rows = stmt.query_map(binds.as_slice(), |r| Ok((r.get::<_, i64>(0)?, r.get(1)?)))?;
    for row in rows {
        let (cid, name) = row?;
        out.entry(cid).or_default().push(name);
    }
    Ok(out)
}

/// 1 人のタグ名を読む（名前順）。
pub(super) fn tags_of(conn: &Connection, id: i64) -> rusqlite::Result<Vec<String>> {
    Ok(load_tags(conn, Scope::One(id))?
        .remove(&id)
        .unwrap_or_default())
}

/// つながり（サービスとアカウント）を読む。同じアカウントへの重複（統合で 2 本になった等）は 1 つにする。
fn load_links(conn: &Connection, scope: Scope) -> rusqlite::Result<HashMap<i64, Vec<ContactLink>>> {
    let (where_sql, bind) = scope.filter("ci.contact_id");
    let cond = if where_sql.is_empty() {
        "WHERE ci.contact_id IS NOT NULL".to_string()
    } else {
        where_sql
    };
    let sql = format!(
        "SELECT DISTINCT ci.contact_id, ci.provider, ci.account_id, ga.email, \
                ga.disconnected_at IS NOT NULL \
         FROM contact_identities ci \
         LEFT JOIN google_accounts ga ON ci.provider = 'google' AND ga.id = ci.account_id \
         {cond} ORDER BY ci.contact_id, ci.provider, ci.account_id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let binds: Vec<&dyn ToSql> = bind.iter().map(|b| b as &dyn ToSql).collect();
    let mut out: HashMap<i64, Vec<ContactLink>> = HashMap::new();
    let rows = stmt.query_map(binds.as_slice(), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<bool>>(4)?.unwrap_or(false),
        ))
    })?;
    for row in rows {
        let (cid, provider, account_id, account_email, disconnected) = row?;
        if let Some(provider) = ContactProvider::from_db(&provider) {
            out.entry(cid).or_default().push(ContactLink {
                provider,
                account_id: account_id as i32,
                account_email,
                disconnected,
            });
        }
    }
    Ok(out)
}
