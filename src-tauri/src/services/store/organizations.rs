//! 組織カード（会社・組織）の読み書き。docs/CONTACT_MODEL.md §4。
//!
//! 個人の連絡先は `contact_organizations.org_id` でカードを指す。カードの代表電話・FAX・
//! 代表メール・URL・所在地はどのサービスにも送らない（向こうに「組織」という単位が無い）。
//! 名前はカードが正で、つながっている人の会社名はカードの名前にそろえる。

use super::contact_rows::{query_summaries, SUMMARY_ORDER};
use super::contact_write::mark_dirty;
use super::Store;
use crate::models::{
    OrgAddress, OrgDuplicateGroup, OrgSharedValue, OrganizationDetail, OrganizationInput,
    OrganizationSummary,
};
use crate::services::dedupe::{fold, normalize_org};
use rusqlite::{params, Connection, Row};
use std::collections::HashMap;

/// organizations の 1 行を OrganizationSummary に写す（列順は ORG_COLS と対応）。
fn row_to_org(r: &Row) -> rusqlite::Result<OrganizationSummary> {
    Ok(OrganizationSummary {
        id: r.get::<_, i64>(0)? as i32,
        name: r.get(1)?,
        name_kana: r.get(2)?,
        note: r.get(3)?,
        phone: r.get(4)?,
        fax: r.get(5)?,
        email: r.get(6)?,
        url: r.get(7)?,
        address: OrgAddress {
            postal: r.get(8)?,
            region: r.get(9)?,
            city: r.get(10)?,
            street: r.get(11)?,
            extended: r.get(12)?,
            country: r.get(13)?,
        },
        member_count: r.get::<_, i64>(14)? as i32,
        deleted_at: r.get(15)?,
    })
}

/// 組織の取得列（別名 o の organizations と、所属人数のサブクエリ）。
const ORG_COLS: &str = "o.id, o.name, o.name_kana, o.note, o.phone, o.fax, o.email, o.url, \
     o.postal, o.region, o.city, o.street, o.extended, o.country, \
     (SELECT count(DISTINCT co.contact_id) FROM contact_organizations co \
      JOIN contacts c ON c.id = co.contact_id \
      WHERE co.org_id = o.id AND c.deleted_at IS NULL), \
     o.deleted_at";

/// 組織カードの項目（組織名以外）。統合時に「統合先で空の項目を統合元で埋める」のに使う。
const ORG_CARD_COLS: [&str; 12] = [
    "name_kana",
    "note",
    "phone",
    "fax",
    "email",
    "url",
    "postal",
    "region",
    "city",
    "street",
    "extended",
    "country",
];

/// 組織カードを 1 枚読む。
pub(super) fn load_org(conn: &Connection, id: i64) -> rusqlite::Result<OrganizationSummary> {
    conn.query_row(
        &format!("SELECT {ORG_COLS} FROM organizations o WHERE o.id = ?1"),
        params![id],
        row_to_org,
    )
}

/// カードにつながっている人の会社名をカードの名前にそろえる。名前が変わった人には
/// 送り直しの印を立てる（会社名は Google 等へ送る項目なので）。
pub(super) fn sync_member_names(conn: &Connection, org_id: i64) -> rusqlite::Result<()> {
    let changed: Vec<i64> = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT contact_id FROM contact_organizations \
             WHERE org_id = ?1 AND name IS NOT (SELECT name FROM organizations WHERE id = ?1)",
        )?;
        let rows = stmt.query_map(params![org_id], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    conn.execute(
        "UPDATE contact_organizations SET name = (SELECT name FROM organizations WHERE id = ?1) \
         WHERE org_id = ?1",
        params![org_id],
    )?;
    changed.into_iter().try_for_each(|id| mark_dirty(conn, id))
}

/// 組織名検索のトークン化。空白で分割し、各断片を「ASCII 英数字の連なり」と
/// 「それ以外の連なり」に分けてトークンにする（2 文字以上のみ・重複排除）。
/// 例: "sngDESIGN浦添アトリエ" → ["sngdesign", "浦添アトリエ"]。
fn org_search_tokens(query: &str) -> Vec<String> {
    fn flush(cur: &mut String, tokens: &mut Vec<String>) {
        if cur.chars().count() >= 2 && !tokens.iter().any(|t| t == cur) {
            tokens.push(cur.clone());
        }
        cur.clear();
    }
    let mut tokens: Vec<String> = Vec::new();
    for piece in query.split_whitespace() {
        let mut cur = String::new();
        let mut cur_ascii: Option<bool> = None;
        for ch in piece.chars() {
            let is_ascii = ch.is_ascii_alphanumeric();
            if cur_ascii.is_some_and(|prev| prev != is_ascii) {
                flush(&mut cur, &mut tokens);
            }
            cur_ascii = Some(is_ascii);
            cur.push(if is_ascii {
                ch.to_ascii_lowercase()
            } else {
                ch
            });
        }
        flush(&mut cur, &mut tokens);
    }
    tokens
}

/// 組織名がクエリの各トークンをいくつ含むか（関連度スコア）。
fn org_name_score(name: &str, tokens: &[String]) -> usize {
    let folded = fold(name);
    tokens
        .iter()
        .filter(|t| folded.contains(t.as_str()))
        .count()
}

impl Store {
    /// 組織一覧（所属人数つき＝削除済み連絡先は数えない）。`query` があれば「似た名前」を出す。
    ///
    /// クエリをトークンに分け、そのいずれかを含む組織を拾って（OR）、一致トークン数の多い順に
    /// 並べる。`include_deleted` が true なら論理削除済みの組織も含める。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn list_organizations(
        &self,
        query: Option<&str>,
        include_deleted: bool,
    ) -> rusqlite::Result<Vec<OrganizationSummary>> {
        let conn = self.conn.lock().unwrap();
        let tokens = query
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(org_search_tokens)
            .unwrap_or_default();
        let likes: Vec<String> = tokens
            .iter()
            .map(|t| format!("%{}%", t.replace('%', "\\%").replace('_', "\\_")))
            .collect();
        let mut conds: Vec<String> = Vec::new();
        if !include_deleted {
            conds.push("o.deleted_at IS NULL".to_string());
        }
        if !likes.is_empty() {
            let ors: Vec<String> = (1..=likes.len())
                .map(|n| {
                    format!("o.name LIKE ?{n} ESCAPE '\\' OR o.name_kana LIKE ?{n} ESCAPE '\\'")
                })
                .collect();
            conds.push(format!("({})", ors.join(" OR ")));
        }
        let where_sql = if conds.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conds.join(" AND "))
        };
        let mut stmt = conn.prepare(&format!(
            "SELECT {ORG_COLS} FROM organizations o {where_sql}"
        ))?;
        let mut rows: Vec<OrganizationSummary> = stmt
            .query_map(rusqlite::params_from_iter(likes.iter()), row_to_org)?
            .collect::<rusqlite::Result<_>>()?;
        // 関連度順: 一致トークン数 desc → 有効(非削除)優先 → 所属多い順 → 名前。
        rows.sort_by(|a, b| {
            org_name_score(&b.name, &tokens)
                .cmp(&org_name_score(&a.name, &tokens))
                .then_with(|| a.deleted_at.is_some().cmp(&b.deleted_at.is_some()))
                .then_with(|| b.member_count.cmp(&a.member_count))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(rows)
    }

    /// 組織カードを 1 枚、所属人数つきで返す。
    ///
    /// # Errors
    /// 該当が無いときや DB の読み出しに失敗したとき。
    pub fn get_organization(&self, id: i64) -> rusqlite::Result<OrganizationSummary> {
        let conn = self.conn.lock().unwrap();
        load_org(&conn, id)
    }

    /// 組織カードを作成/編集する。`input.id` 指定で更新し、つながっている人の会社名も
    /// 新しい名前へそろえる（名前が変われば、その人たちは次の同期で送り直す）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn upsert_organization(
        &self,
        input: &OrganizationInput,
    ) -> rusqlite::Result<OrganizationSummary> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let a = &input.address;
        let name = input.name.trim();
        let id =
            match input.id.map(i64::from) {
                Some(id) => {
                    tx.execute(
                        "UPDATE organizations SET name = ?1, name_kana = ?2, note = ?3, \
                     phone = ?4, fax = ?5, email = ?6, url = ?7, postal = ?8, region = ?9, \
                     city = ?10, street = ?11, extended = ?12, country = ?13, \
                     updated_at = CURRENT_TIMESTAMP WHERE id = ?14",
                        params![
                            name,
                            input.name_kana,
                            input.note,
                            input.phone,
                            input.fax,
                            input.email,
                            input.url,
                            a.postal,
                            a.region,
                            a.city,
                            a.street,
                            a.extended,
                            a.country,
                            id,
                        ],
                    )?;
                    sync_member_names(&tx, id)?;
                    id
                }
                None => {
                    tx.execute(
                    "INSERT INTO organizations (name, name_kana, note, phone, fax, email, url, \
                     postal, region, city, street, extended, country) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![
                        name, input.name_kana, input.note, input.phone, input.fax, input.email,
                        input.url, a.postal, a.region, a.city, a.street, a.extended, a.country,
                    ],
                )?;
                    tx.last_insert_rowid()
                }
            };
        tx.commit()?;
        load_org(&conn, id)
    }

    /// 組織を論理削除する（ゴミ箱へ）。つながっている人（削除済みを除く）がいるときは
    /// 削除せず false を返す（安全側）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn delete_organization(&self, id: i64) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        if load_org(&conn, id)?.member_count > 0 {
            return Ok(false);
        }
        conn.execute(
            "UPDATE organizations SET deleted_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![id],
        )?;
        Ok(true)
    }

    /// 論理削除した組織を復元する。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn restore_organization(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE organizations SET deleted_at = NULL WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// 組織の詳細（つながっている人＋共有アドレスを件数つきで）。住所録の「組織」タブ用。
    ///
    /// # Errors
    /// 該当が無いときや DB の読み出しに失敗したとき。
    pub fn organization_detail(&self, id: i64) -> rusqlite::Result<OrganizationDetail> {
        let conn = self.conn.lock().unwrap();
        let org = load_org(&conn, id)?;
        let members = query_summaries(
            &conn,
            "c.deleted_at IS NULL AND EXISTS (SELECT 1 FROM contact_organizations co \
             WHERE co.contact_id = c.id AND co.org_id = ?1)",
            SUMMARY_ORDER,
            &[&id],
        )?;
        let mut shared_values: Vec<OrgSharedValue> = Vec::new();
        for (table, kind, group) in [
            ("contact_emails", "email", "lower(v.value)"),
            ("contact_phones", "phone", "v.value"),
        ] {
            let mut stmt = conn.prepare(&format!(
                "SELECT v.value, count(DISTINCT v.contact_id), max(v.label) \
                 FROM {table} v JOIN contacts c ON c.id = v.contact_id \
                 WHERE v.is_shared = 1 AND c.deleted_at IS NULL AND EXISTS ( \
                     SELECT 1 FROM contact_organizations co \
                     WHERE co.contact_id = v.contact_id AND co.org_id = ?1) \
                 GROUP BY {group} ORDER BY 2 DESC, v.value"
            ))?;
            let rows = stmt.query_map(params![id], |r| {
                Ok(OrgSharedValue {
                    kind: kind.to_string(),
                    label: r.get(2)?,
                    value: r.get(0)?,
                    count: r.get::<_, i64>(1)? as i32,
                })
            })?;
            for row in rows {
                shared_values.push(row?);
            }
        }
        Ok(OrganizationDetail {
            org,
            members,
            shared_values,
        })
    }

    /// 組織名の重複候補を正規化名で束ねて返す（2 件以上、所属合計の多い順）。
    /// 「株式会社◯◯」と「(株)◯◯」など法人格・表記ゆれを同一グループにする。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn find_organization_duplicates(&self) -> rusqlite::Result<Vec<OrgDuplicateGroup>> {
        let mut map: HashMap<String, Vec<OrganizationSummary>> = HashMap::new();
        for o in self.list_organizations(None, false)? {
            let key = normalize_org(&o.name);
            if !key.is_empty() {
                map.entry(key).or_default().push(o);
            }
        }
        let mut groups: Vec<OrgDuplicateGroup> = map
            .into_values()
            .filter(|v| v.len() > 1)
            .map(|mut v| {
                // 既定の統一名: 最多所属 → 名前が長い → 名前順。
                v.sort_by(|a, b| {
                    b.member_count
                        .cmp(&a.member_count)
                        .then_with(|| b.name.chars().count().cmp(&a.name.chars().count()))
                        .then_with(|| a.name.cmp(&b.name))
                });
                OrgDuplicateGroup {
                    canonical: v[0].name.clone(),
                    organizations: v,
                }
            })
            .collect();
        groups.sort_by(|a, b| {
            let sum = |g: &OrgDuplicateGroup| -> i32 {
                g.organizations.iter().map(|o| o.member_count).sum()
            };
            sum(b)
                .cmp(&sum(a))
                .then_with(|| a.canonical.cmp(&b.canonical))
        });
        Ok(groups)
    }

    /// 複数の組織を 1 件（`keep_id`）に統一する。統一名 `name` を keep に設定し、drop 側に
    /// つながっている人を keep へ付け替え、drop 組織を削除する。統一後の組織を返す。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき（全体を巻き戻す）。
    pub fn merge_organizations(
        &self,
        keep_id: i64,
        drop_ids: &[i64],
        name: &str,
    ) -> rusqlite::Result<OrganizationSummary> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        // 統合先は表に残すので、万一ゴミ箱にあっても復活させる。
        tx.execute(
            "UPDATE organizations SET name = ?1, deleted_at = NULL, \
             updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
            params![name.trim(), keep_id],
        )?;
        // 統合先で空のカード項目は、統合元に入っていた値で埋める（消える情報をなくす）。
        let fill = ORG_CARD_COLS
            .iter()
            .map(|c| format!("{c} = COALESCE({c}, (SELECT {c} FROM organizations WHERE id = ?1))"))
            .collect::<Vec<_>>()
            .join(", ");
        let fill_sql = format!("UPDATE organizations SET {fill} WHERE id = ?2");
        for did in drop_ids {
            tx.execute(&fill_sql, params![did, keep_id])?;
            tx.execute(
                "UPDATE contact_organizations SET org_id = ?1 WHERE org_id = ?2",
                params![keep_id, did],
            )?;
            tx.execute("DELETE FROM organizations WHERE id = ?1", params![did])?;
        }
        sync_member_names(&tx, keep_id)?;
        tx.commit()?;
        load_org(&conn, keep_id)
    }
}

#[cfg(test)]
mod tests;
