//! 連絡先のタグ（メールと共通の `tags`）の付け外し。
//!
//! 編集画面の保存はそろえる（[`set_tags`]）、ファイル取り込みは足す（[`add_tags`]）、Google の
//! 取り込みは Google が持っているラベルだけを付け外しする（[`reconcile_managed_tags`]）。

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;

/// タグ名から id を得る（無ければ作る。メールと共通の tags）。
fn find_or_create_tag(conn: &Connection, name: &str) -> rusqlite::Result<i64> {
    let found: Option<i64> = conn
        .query_row("SELECT id FROM tags WHERE name = ?1", params![name], |r| {
            r.get(0)
        })
        .optional()?;
    match found {
        Some(id) => Ok(id),
        None => {
            conn.execute(
                "INSERT INTO tags (name, kind) VALUES (?1, 'tag')",
                params![name],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// タグを付ける（冪等。空の名前は無視）。
pub(super) fn add_tags(conn: &Connection, id: i64, names: &[String]) -> rusqlite::Result<()> {
    for name in names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        let tid = find_or_create_tag(conn, name)?;
        conn.execute(
            "INSERT OR IGNORE INTO contact_tags (contact_id, tag_id) VALUES (?1, ?2)",
            params![id, tid],
        )?;
    }
    Ok(())
}

/// タグを `names` にそろえる（編集画面の保存）。
pub(super) fn set_tags(conn: &Connection, id: i64, names: &[String]) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM contact_tags WHERE contact_id = ?1",
        params![id],
    )?;
    add_tags(conn, id, names)
}

/// 外部サービス（Google のラベル）由来のタグだけを付け外しする。
///
/// `managed` は「そのサービスが持っているタグ名」。ここに載っている名前だけを外す対象にし、
/// 載っていない名前（利用者がアプリ内だけで付けたタグ）は触らない。付けるほうは `wanted` を
/// そのまま反映する（冪等）。
pub(super) fn reconcile_managed_tags(
    conn: &Connection,
    id: i64,
    wanted: &[String],
    managed: &HashSet<String>,
) -> rusqlite::Result<()> {
    for name in super::contact_rows::tags_of(conn, id)? {
        if managed.contains(&name) && !wanted.iter().any(|w| w.trim() == name) {
            conn.execute(
                "DELETE FROM contact_tags WHERE contact_id = ?1 \
                 AND tag_id IN (SELECT id FROM tags WHERE name = ?2)",
                params![id, name],
            )?;
        }
    }
    add_tags(conn, id, wanted)
}
