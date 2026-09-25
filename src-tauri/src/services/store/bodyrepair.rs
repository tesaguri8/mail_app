//! 「取得済み」と記録されているのに、実体に全文が無い行の修復。
//!
//! alpha.13 の「メタ先行・本文後追い」では、`mail_parser` が text/plain のメールのヘッダから
//! 合成する `<html><body></body></html>` を本文と数えてしまい、`body_state='present'` の行が
//! できた（docs/SYNC.md §3.6）。その行には後から `clean_body` だけが入るため、一覧の
//! プレビューには本文が出るのに、**全文（body_plain / HTML）は空のまま**になる。
//! HTML 本文が無いので、ニュースレターは HTML で描けず text/plain 側
//! （「HTML形式でご覧ください」等）しか出ない。
//!
//! 取り込み側は直したが、既にできてしまった行は記録が嘘をついたままなので、ここで
//! 'absent'（未取得）へ戻して背景の本文バックフィルに拾わせる。

use super::emails::has_html_body;
use rusqlite::{params, Connection, OptionalExtension};

/// 修復の版。直し方を変えたら上げる（次回起動でもう一度見直す）。
const REPAIR_VERSION: &str = "1";
/// 実施済みの版を控える設定キー（app_settings）。
const KEY_REPAIRED: &str = "body_state.repaired_version";

/// 記録は 'present' でも実体に全文が無い行を 'absent' に戻す（`Store::open` から一度だけ）。
///
/// 対象は「body_plain に文字が無く、HTML にも中身が無い」行だけ。要約落ち（'evicted'）と
/// 容量整理済み（body_compacted=1）は意図して本文を落とした行なので触らない。
///
/// 戻り値は戻した件数。
pub fn repair_missing_bodies(conn: &Connection) -> rusqlite::Result<usize> {
    let done: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![KEY_REPAIRED],
            |r| r.get(0),
        )
        .optional()?;
    if done.as_deref() == Some(REPAIR_VERSION) {
        return Ok(0);
    }
    // 全文が空の候補だけを見る（HTML の中身は SQL では判定できないので Rust 側で開く）。
    let candidates: Vec<(i64, Option<String>, Option<Vec<u8>>)> = {
        let mut stmt = conn.prepare(
            "SELECT id, body_html, body_html_z FROM emails
             WHERE COALESCE(body_state,'present') = 'present'
               AND COALESCE(body_compacted, 0) = 0
               AND COALESCE(TRIM(body_plain), '') = ''",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut repaired = 0;
    for (id, html, html_z) in candidates {
        let stored = match html_z {
            Some(z) => crate::services::compress::decompress_text(&z).ok(),
            None => html,
        };
        if stored.as_deref().is_some_and(has_html_body) {
            continue; // 画像だけの HTML 等、中身のある本文は触らない
        }
        conn.execute(
            "UPDATE emails SET body_state = 'absent' WHERE id = ?1",
            params![id],
        )?;
        repaired += 1;
    }
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        params![KEY_REPAIRED, REPAIR_VERSION],
    )?;
    Ok(repaired)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::store::Store;

    /// 本文列を直接指定して 1 行作る（取り込み経路を通さず、壊れた形をそのまま再現する）。
    fn seed(conn: &Connection, key: &str, plain: &str, html: Option<&str>, state: &str) -> i64 {
        let html_z = html
            .filter(|s| !s.is_empty())
            .map(crate::services::compress::compress_text);
        conn.execute(
            "INSERT INTO emails (account_id, canonical_key, folder, body_plain, clean_body,
                                 body_html_z, body_state)
             VALUES (1, ?1, 'inbox', ?2, '本文の新規部分', ?3, ?4)",
            params![key, plain, html_z, state],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn state_of(conn: &Connection, id: i64) -> String {
        conn.query_row(
            "SELECT body_state FROM emails WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn turns_content_free_rows_back_to_absent() {
        let store = Store::open_in_memory_for_test();
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO accounts (id, email, imap_host, smtp_host)
             VALUES (1, 'me@example.com', 'imap.example.com', 'smtp.example.com')",
            [],
        )
        .unwrap();
        // 壊れた行: 記録は present、実体は空の骨組みだけ。
        let broken = seed(
            &conn,
            "a",
            "",
            Some("<html><body></body></html>"),
            "present",
        );
        // 全文のある行は触らない。
        let ok_html = seed(
            &conn,
            "b",
            "",
            Some("<html><body>本文</body></html>"),
            "present",
        );
        let ok_plain = seed(&conn, "c", "本文", None, "present");
        // 画像だけの HTML も「本文あり」。
        let img = seed(
            &conn,
            "d",
            "",
            Some("<html><body><img src=\"cid:a\"></body></html>"),
            "present",
        );
        // 容量整理で落とした行は意図的なので触らない。
        let evicted = seed(&conn, "e", "", None, "evicted");

        assert_eq!(repair_missing_bodies(&conn).unwrap(), 1);
        assert_eq!(state_of(&conn, broken), "absent");
        assert_eq!(state_of(&conn, ok_html), "present");
        assert_eq!(state_of(&conn, ok_plain), "present");
        assert_eq!(state_of(&conn, img), "present");
        assert_eq!(state_of(&conn, evicted), "evicted");

        // 2 回目は印が付いているので走らない。
        assert_eq!(repair_missing_bodies(&conn).unwrap(), 0);
    }
}
