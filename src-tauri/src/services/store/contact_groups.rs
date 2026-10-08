//! Google の連絡先グループ（ラベル）とタグ名の対応表（`contact_group_identities`）。
//!
//! People API はグループを `contactGroups/{id}` で指し、所属の変更も ID で行うので、名前だけでは
//! 足りない。取り込みのたびに Google の一覧で洗い替え、送信で作ったラベルは次の取り込みまで覚える。

use super::Store;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;

impl Store {
    /// Google の連絡先グループ（ラベル）一覧で台帳を洗い替える。
    ///
    /// Google 側で消えたラベルの行が残っていると、そのタグが「Google の持ち物」と誤判定され、
    /// 取り込みで外されてしまう。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn replace_contact_groups(
        &self,
        account_id: i64,
        groups: &[(String, String)],
    ) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM contact_group_identities WHERE provider = 'google' AND account_id = ?1",
            params![account_id],
        )?;
        for (external_id, name) in groups {
            remember_group(&tx, account_id, external_id, name)?;
        }
        tx.commit()
    }

    /// ラベル名 → Google のグループ ID。未知の名前は None（送信側が作る）。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn contact_group_id(
        &self,
        account_id: i64,
        name: &str,
    ) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT external_id FROM contact_group_identities \
             WHERE provider = 'google' AND account_id = ?1 AND name = ?2",
            params![account_id, name],
            |r| r.get(0),
        )
        .optional()
    }

    /// Google 側に作ったラベルを台帳へ覚える（次の取り込みまで ID を引けるように）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn remember_contact_group(
        &self,
        account_id: i64,
        external_id: &str,
        name: &str,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        remember_group(&conn, account_id, external_id, name)
    }
}

/// ラベルの対応を 1 件覚える。
fn remember_group(
    conn: &Connection,
    account_id: i64,
    external_id: &str,
    name: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO contact_group_identities \
             (provider, account_id, external_id, name, fetched_at) \
         VALUES ('google', ?1, ?2, ?3, CURRENT_TIMESTAMP)",
        params![account_id, external_id, name],
    )?;
    Ok(())
}

/// このアカウントで Google が持っているラベル名の集合（取り込みで外してよいタグ）。
pub(super) fn managed_group_names(
    conn: &Connection,
    account_id: i64,
) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM contact_group_identities WHERE provider = 'google' AND account_id = ?1",
    )?;
    let rows = stmt.query_map(params![account_id], |r| r.get::<_, String>(0))?;
    rows.collect()
}
