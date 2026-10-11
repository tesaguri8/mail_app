//! 組織（会社・組織）の読み出しと整理。docs/ORG_CONTACTS.md。
//!
//! 組織は連絡先の 1 件（`contacts.kind = 'organization'`）で、書き込みは連絡先と同じ
//! （`contact_upsert`）。ここは組織タブ（見る・つなぐ）のための読み出しと、重複の統合だけを持つ。
//! 人は `contact_organizations.org_id` で組織を指し、人の会社名は組織の名前にそろえる。

use super::contact_dedupe::merge_in;
use super::contact_rows::{load_contact, query_summaries, SUMMARY_ORDER};
use super::contact_write::{mark_dirty, write_contact, OrgLinking, WriteOptions};
use super::Store;
use crate::models::{
    OrgAddress, OrgDuplicateGroup, OrgSharedValue, OrganizationDetail, OrganizationSummary,
};
use crate::services::dedupe::{fold, normalize_org};
use rusqlite::{params, Connection, Row};
use std::collections::HashMap;

/// 組織の 1 行を OrganizationSummary に写す（列順は ORG_COLS と対応）。
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

/// 組織の主な住所の 1 列（別名 c の組織の、先頭の住所）。
fn address_col(col: &str) -> String {
    format!(
        "(SELECT {col} FROM contact_addresses WHERE contact_id = c.id ORDER BY position, id LIMIT 1)"
    )
}

/// 組織の取得列（別名 c の contacts。代表の値は子テーブルの先頭、FAX は FAX のラベルの先頭）。
fn org_cols() -> String {
    format!(
        "c.id, c.display_name, \
         (SELECT phonetic_name FROM contact_organizations WHERE contact_id = c.id \
          ORDER BY position, id LIMIT 1), \
         c.note, \
         (SELECT value FROM contact_phones WHERE contact_id = c.id AND COALESCE(label, '') <> 'FAX' \
          ORDER BY position, id LIMIT 1), \
         (SELECT value FROM contact_phones WHERE contact_id = c.id AND label = 'FAX' \
          ORDER BY position, id LIMIT 1), \
         (SELECT value FROM contact_emails WHERE contact_id = c.id ORDER BY position, id LIMIT 1), \
         (SELECT value FROM contact_urls WHERE contact_id = c.id ORDER BY position, id LIMIT 1), \
         {}, {}, {}, {}, {}, {}, \
         (SELECT count(DISTINCT co.contact_id) FROM contact_organizations co \
          JOIN contacts m ON m.id = co.contact_id \
          WHERE co.org_id = c.id AND m.deleted_at IS NULL), \
         c.deleted_at",
        address_col("postal"),
        address_col("region"),
        address_col("city"),
        address_col("street"),
        address_col("extended"),
        address_col("country"),
    )
}

/// 組織を 1 件読む（組織でなければ該当なし）。
pub(super) fn load_org(conn: &Connection, id: i64) -> rusqlite::Result<OrganizationSummary> {
    conn.query_row(
        &format!(
            "SELECT {} FROM contacts c WHERE c.id = ?1 AND c.kind = 'organization'",
            org_cols()
        ),
        params![id],
        row_to_org,
    )
}

/// 組織につながっている人の会社名を組織の名前にそろえる。名前が変わった人には
/// 送り直しの印を立てる（会社名は Google 等へ送る項目なので）。
pub(super) fn sync_member_names(conn: &Connection, org_id: i64) -> rusqlite::Result<()> {
    let changed: Vec<i64> = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT contact_id FROM contact_organizations \
             WHERE org_id = ?1 AND name IS NOT (SELECT display_name FROM contacts WHERE id = ?1)",
        )?;
        let rows = stmt.query_map(params![org_id], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    conn.execute(
        "UPDATE contact_organizations SET name = (SELECT display_name FROM contacts WHERE id = ?1) \
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
            conds.push("c.deleted_at IS NULL".to_string());
        }
        if !likes.is_empty() {
            let ors: Vec<String> = (1..=likes.len())
                .map(|n| {
                    format!(
                        "c.display_name LIKE ?{n} ESCAPE '\\' OR c.sort_name LIKE ?{n} ESCAPE '\\'"
                    )
                })
                .collect();
            conds.push(format!("({})", ors.join(" OR ")));
        }
        conds.push("c.kind = 'organization'".to_string());
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM contacts c WHERE {}",
            org_cols(),
            conds.join(" AND ")
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

    /// 組織を 1 件、所属人数つきで返す。
    ///
    /// # Errors
    /// 該当が無いときや DB の読み出しに失敗したとき。
    pub fn get_organization(&self, id: i64) -> rusqlite::Result<OrganizationSummary> {
        let conn = self.conn.lock().unwrap();
        load_org(&conn, id)
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

    /// 複数の組織を 1 件（`keep_id`）に統一する。連絡先の統合と同じく、つながり（Google 等）・
    /// `uid`・別人の記録・代表の値を残す側へ寄せ、人のつながり（`org_id`）も付け替える。
    /// 統一名 `name` を残す側の名前にし、つながっている人の会社名もそろえる。統一後の組織を返す。
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
        // 統合先は表に残すので、万一ゴミ箱にあっても戻す。
        tx.execute(
            "UPDATE contacts SET deleted_at = NULL WHERE id = ?1",
            params![keep_id],
        )?;
        merge_in(&tx, keep_id, drop_ids)?;
        rename_org(&tx, keep_id, name)?;
        tx.commit()?;
        load_org(&conn, keep_id)
    }
}

/// 組織の名前を変え、つながっている人の会社名もそろえる（変わったら送り直しの印）。
pub(super) fn rename_org(conn: &Connection, id: i64, name: &str) -> rusqlite::Result<()> {
    let name = name.trim();
    let mut org = load_contact(conn, id)?.fields;
    let current = org.organizations.first().and_then(|o| o.name.as_deref());
    if name.is_empty() || current == Some(name) {
        return sync_member_names(conn, id);
    }
    match org.organizations.first_mut() {
        Some(o) => o.name = Some(name.to_string()),
        None => org.organizations.push(crate::models::ContactOrganization {
            name: Some(name.to_string()),
            ..Default::default()
        }),
    }
    org.display_name = name.to_string();
    write_contact(
        conn,
        Some(id),
        &org,
        WriteOptions {
            mark_dirty: true,
            org_linking: OrgLinking::ExistingOnly,
        },
    )?;
    sync_member_names(conn, id)
}

#[cfg(test)]
mod tests;
