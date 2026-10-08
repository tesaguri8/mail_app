//! メールアドレス → 住所録の連絡先の引き当て（差出人名の解決・知り合い/お気に入り判定）。
//!
//! メールの一覧・スレッド・迷惑判定・グリーンドメインなど、連絡先を参照する機能はここを通す。
//! 照合は `contact_emails` の小文字の完全一致（式索引 `idx_contact_emails_value_lower`）。

use rusqlite::{params, Connection, OptionalExtension};

/// `addr_expr`（SQL 式）に一致するメールを持つ、削除されていない連絡先が居るかの SQL 断片。
/// `favorite_only` なら お気に入り（VIP）の連絡先に限る。
pub(super) fn contact_exists_sql(addr_expr: &str, favorite_only: bool) -> String {
    let fav = if favorite_only {
        " AND kc.is_favorite = 1"
    } else {
        ""
    };
    format!(
        "EXISTS (SELECT 1 FROM contact_emails kce JOIN contacts kc ON kc.id = kce.contact_id \
         WHERE kc.deleted_at IS NULL{fav} AND lower(kce.value) = lower({addr_expr}))"
    )
}

/// アドレスに一致する住所録の表示名（お気に入りを優先）。一致が無ければ None。
pub(super) fn contact_name_for(
    conn: &Connection,
    address: Option<&str>,
) -> rusqlite::Result<Option<String>> {
    let Some(addr) = address.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    conn.query_row(
        "SELECT kc.display_name FROM contact_emails kce JOIN contacts kc ON kc.id = kce.contact_id \
         WHERE kc.deleted_at IS NULL AND lower(kce.value) = lower(?1) \
         ORDER BY kc.is_favorite DESC LIMIT 1",
        params![addr],
        |r| r.get(0),
    )
    .optional()
}

/// アドレスが住所録の連絡先（`favorite_only` ならお気に入り）に一致するか。
pub(super) fn address_matches_contact(
    conn: &Connection,
    address: &str,
    favorite_only: bool,
) -> rusqlite::Result<bool> {
    let addr = address.trim();
    if addr.is_empty() {
        return Ok(false);
    }
    let sql = format!("SELECT {}", contact_exists_sql("?1", favorite_only));
    conn.query_row(&sql, params![addr], |r| r.get::<_, i64>(0))
        .map(|n| n != 0)
}

/// 削除されていない連絡先のメールアドレスをすべて返す（グリーンドメインの材料）。
pub(super) fn all_contact_emails(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT kce.value FROM contact_emails kce JOIN contacts kc ON kc.id = kce.contact_id \
         WHERE kc.deleted_at IS NULL",
    )?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    rows.collect()
}
