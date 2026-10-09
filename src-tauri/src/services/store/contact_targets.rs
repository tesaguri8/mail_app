//! 連絡先ごとの同期先（docs/CONTACT_MODEL.md §3「同期先は 1 人ずつ選ぶ」・マイグレーション 0062）。
//!
//! - **加える**: 作成待ち（`contact_create_requests`）を置く。次の同期の push で作成し、できたら
//!   つながり（`contact_identities`）の行を作って作成待ちを消す（`Store::mark_contact_pushed`）
//! - **外す（向こうは残す）**: つながりの行をすぐ消す。連絡先は Rondine に残る
//! - **外す（向こうも消す）**: `unlink_requested` を立て、次の同期で削除を送る。送れたら行を消す
//!
//! 新規作成の画面で最初からチェックを入れるかは画面の既定（端末ごと・前回の選択を覚える）で、
//! DB には持たない。どこにもつながっていない連絡先を勝手に作ることはしない。
//! （`google_accounts.push_new_contacts` の列は以前の設定の名残で、使っていない / 利用者の判断
//! 2026-10-09）

use super::contact_sync::refresh_contact_dirty;
use super::Store;
use rusqlite::{params, Connection};

/// その連絡先がそのアカウントにもうつながっているか（作成待ちを置く意味が無い）。
fn linked(conn: &Connection, contact_id: i64, account_id: i64) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM contact_identities \
         WHERE provider = 'google' AND account_id = ?1 AND contact_id = ?2)",
        params![account_id, contact_id],
        |r| r.get(0),
    )
}

/// 既につながっているアカウントへの作成待ちを消す（統合で寄せたときなど、作ると二重になる）。
pub(super) fn drop_redundant_create_requests(
    conn: &Connection,
    contact_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM contact_create_requests WHERE contact_id = ?1 AND EXISTS ( \
             SELECT 1 FROM contact_identities ci WHERE ci.contact_id = ?1 \
               AND ci.provider = contact_create_requests.provider \
               AND ci.account_id = contact_create_requests.account_id)",
        params![contact_id],
    )?;
    Ok(())
}

impl Store {
    /// 「Google（このアカウント）にも保存」: 作成待ちを置く。既につながっている、または既に
    /// 作成待ちなら何もしない（二重に作らない）。
    ///
    /// アカウントが連携中で連絡先の同期をしているかは呼び出し側（commands 層）で確かめる。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn request_contact_create(&self, contact_id: i64, account_id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        if linked(&conn, contact_id, account_id)? {
            return Ok(());
        }
        conn.execute(
            "INSERT OR IGNORE INTO contact_create_requests (contact_id, provider, account_id) \
             VALUES (?1, 'google', ?2)",
            params![contact_id, account_id],
        )?;
        Ok(())
    }

    /// そのアカウントとの同期をやめる。作成待ちなら取り消すだけ。つながっていれば、
    /// `delete_remote` が偽なら行をすぐ消し（向こうの連絡先は残る）、真なら次の同期で向こうも
    /// 削除するよう印を立てる。どちらでも Rondine の連絡先は残る。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき（全体を巻き戻す）。
    pub fn stop_contact_sync(
        &self,
        contact_id: i64,
        account_id: i64,
        delete_remote: bool,
    ) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM contact_create_requests \
             WHERE contact_id = ?1 AND provider = 'google' AND account_id = ?2",
            params![contact_id, account_id],
        )?;
        if delete_remote {
            tx.execute(
                "UPDATE contact_identities SET unlink_requested = 1 \
                 WHERE provider = 'google' AND account_id = ?1 AND contact_id = ?2",
                params![account_id, contact_id],
            )?;
        } else {
            tx.execute(
                "DELETE FROM contact_identities \
                 WHERE provider = 'google' AND account_id = ?1 AND contact_id = ?2",
                params![account_id, contact_id],
            )?;
        }
        refresh_contact_dirty(&tx, contact_id)?;
        tx.commit()
    }
}

#[cfg(test)]
mod tests;
