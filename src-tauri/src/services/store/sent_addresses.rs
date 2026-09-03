//! 「自分から送ったことがある相手」の索引（docs/FILTERING.md §2）。
//!
//! 送信済み（sent）メールの To/Cc に現れたアドレスを `sent_addresses` に集約し、
//! 一覧の「返信歴あり」フィルタが差出人アドレスの完全一致 1 回で判定できるようにする。
//! Bcc は送信控え（Sent への APPEND）に残らず再構築で復元できないため、記録しない
//! （記録すると再構築のたびに結果が変わり、フィルタの見え方が安定しない）。

use super::Store;
use crate::services::addr;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeSet;

/// 索引の作り方のバージョン。アドレスの拾い方を変えたら上げる（起動時に作り直される）。
const INDEX_VERSION: &str = "1";
/// 構築済みバージョンを控える設定キー（app_settings）。
const KEY_BUILT: &str = "sent_index.built_version";

/// 1 通ぶんの宛先（To/Cc のヘッダ列）を索引へ記録する。
/// 同じアドレスが To と Cc の両方にあっても 1 通 1 回として数える。
pub fn record_sent(
    conn: &Connection,
    to_addresses: Option<&str>,
    cc_addresses: Option<&str>,
    date_ts: Option<i64>,
) -> rusqlite::Result<()> {
    let addrs: BTreeSet<String> = [to_addresses, cc_addresses]
        .into_iter()
        .flatten()
        .flat_map(addr::lowercase_addrs)
        .collect();
    for a in &addrs {
        record_one(conn, a, date_ts)?;
    }
    Ok(())
}

/// アドレス 1 件を upsert する（通数を +1、最終送信日時は新しい方を残す）。
fn record_one(conn: &Connection, address: &str, date_ts: Option<i64>) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sent_addresses (address, sent_count, last_sent_ts) VALUES (?1, 1, ?2)
         ON CONFLICT(address) DO UPDATE SET
           sent_count = sent_count + 1,
           last_sent_ts = CASE
             WHEN excluded.last_sent_ts IS NULL THEN sent_addresses.last_sent_ts
             WHEN sent_addresses.last_sent_ts IS NULL THEN excluded.last_sent_ts
             WHEN excluded.last_sent_ts > sent_addresses.last_sent_ts THEN excluded.last_sent_ts
             ELSE sent_addresses.last_sent_ts END",
        params![address, date_ts],
    )?;
    Ok(())
}

/// 手元の送信済みメールから索引を作り直し、登録アドレス数を返す。
/// ゴミ箱にある送信控え（prev_folder='sent'）も「送った事実」は変わらないので含める。
pub fn rebuild(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM sent_addresses", [])?;
    let mut stmt = conn.prepare(
        "SELECT to_addresses, cc_addresses, date_ts FROM emails
         WHERE folder = 'sent' OR (folder = 'trash' AND prev_folder = 'sent')",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<i64>>(2)?,
        ))
    })?;
    for row in rows {
        let (to, cc, ts) = row?;
        record_sent(conn, to.as_deref(), cc.as_deref(), ts)?;
    }
    conn.query_row("SELECT count(*) FROM sent_addresses", [], |r| {
        r.get::<_, i64>(0)
    })
    .map(|n| n as usize)
}

/// 未構築（または索引バージョンが古い）なら 1 度だけ作り直す。`Store::open` から呼ぶ。
/// 以降の更新は取り込み・送信のたびに [`record_sent`] が行うので、ここは通らない。
pub fn ensure_built(conn: &Connection) -> rusqlite::Result<()> {
    let built: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![KEY_BUILT],
            |r| r.get(0),
        )
        .optional()?;
    if built.as_deref() == Some(INDEX_VERSION) {
        return Ok(());
    }
    let n = rebuild(conn)?;
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        params![KEY_BUILT, INDEX_VERSION],
    )?;
    log::info!("sent address index built: {n} addresses");
    Ok(())
}

impl Store {
    /// 送信直後に宛先を索引へ加える（送信控えの同期を待たずにフィルタへ反映するため）。
    /// 引数は Compose の宛先（"名前 <a@b>" 形式もそのまま渡してよい）。
    pub fn record_sent_recipients(&self, to: &[String], cc: &[String]) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        record_sent(&conn, Some(&to.join(", ")), Some(&cc.join(", ")), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addrs(conn: &Connection) -> Vec<(String, i64)> {
        let mut stmt = conn
            .prepare("SELECT address, sent_count FROM sent_addresses ORDER BY address")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        rows
    }

    fn add_email(conn: &Connection, key: &str, folder: &str, to: &str, prev: Option<&str>) {
        conn.execute(
            "INSERT INTO emails (account_id, canonical_key, folder, to_addresses, prev_folder, date_ts)
             VALUES (1, ?1, ?2, ?3, ?4, 100)",
            params![key, folder, to, prev],
        )
        .unwrap();
    }

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::migrations::run(&conn).unwrap();
        conn.execute(
            "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'me@x','i','s')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn record_sent_lowercases_and_counts_once_per_mail() {
        let conn = test_conn();
        // 同じアドレスが To と Cc の両方にいても 1 通 1 回。
        record_sent(
            &conn,
            Some("Alice <Alice@Corp.com>, bob@corp.com"),
            Some("ALICE@corp.com"),
            Some(200),
        )
        .unwrap();
        assert_eq!(
            addrs(&conn),
            vec![("alice@corp.com".into(), 1), ("bob@corp.com".into(), 1)]
        );

        // 2 通目で加算され、最終送信日時は新しい方が残る。
        record_sent(&conn, Some("alice@corp.com"), None, Some(100)).unwrap();
        assert_eq!(addrs(&conn)[0], ("alice@corp.com".into(), 2));
        let ts: Option<i64> = conn
            .query_row(
                "SELECT last_sent_ts FROM sent_addresses WHERE address = 'alice@corp.com'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ts, Some(200));
    }

    #[test]
    fn rebuild_uses_sent_and_trashed_sent_only() {
        let conn = test_conn();
        add_email(&conn, "k1", "sent", "Alice <alice@corp.com>", None);
        add_email(&conn, "k2", "inbox", "me@x", None); // 受信は対象外
        add_email(&conn, "k3", "drafts", "draft@corp.com", None); // 下書きは「送っていない」
        add_email(&conn, "k4", "trash", "carol@corp.com", Some("sent")); // 捨てた送信控えも履歴
        add_email(&conn, "k5", "trash", "dave@corp.com", Some("inbox")); // 捨てた受信は対象外

        let n = rebuild(&conn).unwrap();
        assert_eq!(n, 2);
        assert_eq!(
            addrs(&conn),
            vec![("alice@corp.com".into(), 1), ("carol@corp.com".into(), 1)]
        );
    }

    #[test]
    fn ensure_built_runs_once() {
        let conn = test_conn();
        add_email(&conn, "k1", "sent", "alice@corp.com", None);
        ensure_built(&conn).unwrap();
        assert_eq!(addrs(&conn), vec![("alice@corp.com".into(), 1)]);

        // 2 回目は作り直さない（送信済みメールを足しても索引は変わらない）。
        add_email(&conn, "k2", "sent", "bob@corp.com", None);
        ensure_built(&conn).unwrap();
        assert_eq!(addrs(&conn), vec![("alice@corp.com".into(), 1)]);
    }
}
