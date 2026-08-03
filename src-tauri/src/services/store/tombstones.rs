//! 破棄済み下書きの「墓標」（tombstone）。docs/COMPOSE.md §1。
//!
//! 送信・破棄でローカルから消した下書きは、サーバー Drafts のコピーも消す。ただしその削除は
//! best-effort で失敗しうるうえ、実行中の同期は「削除より前に取得した一覧」を持っているため、
//! そのままだと消したはずの下書きが次の同期で復活してしまう（送信済みメールの下書きが
//! 下書きフォルダに溜まり続ける原因）。
//!
//! そこでローカル削除の直前に canonical_key を墓標として残し、次の二段構えで復活を止める。
//!
//!  - 取り込み側（[`super::emails::insert_email`]）は墓標のあるキーを取り込まない
//!  - サーバー側のコピー削除が済んでいない墓標（`remote_pending=1`）は同期のたびに再試行する
//!
//! 役目を終えた墓標は一定期間後に掃除する。

use super::Store;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

/// 墓標を残しておく期間（日）。サーバー側の削除が済んだものは、これを過ぎたら掃除する。
/// 同期の取りこぼし（長期間オフラインだった端末など）を吸収できる程度に長めにとる。
const KEEP_DAYS: i64 = 30;

/// サーバー側のコピー削除が未完了の墓標（同期での再試行に使う）。
pub struct PendingRemoteDelete {
    /// emails.canonical_key（フォルダ接頭辞つき）。削除完了の印を付けるときの鍵。
    pub canonical_key: String,
    /// Message-ID の中身（山括弧なし）。サーバー上の該当メールを HEADER 検索で引く。
    pub message_id_inner: String,
}

/// この (アカウント, canonical_key) が破棄済み（墓標あり）か。
pub fn is_tombstoned(
    conn: &Connection,
    account_id: i64,
    canonical_key: &str,
) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT 1 FROM deleted_keys WHERE account_id = ?1 AND canonical_key = ?2",
        params![account_id, canonical_key],
        |_| Ok(()),
    )
    .optional()
    .map(|hit| hit.is_some())
}

/// サーバー側のコピー削除が済んでいない下書き墓標を取り出す（同期での再試行用）。
pub fn pending_remote_deletes(
    conn: &Connection,
    account_id: i64,
) -> rusqlite::Result<Vec<PendingRemoteDelete>> {
    let mut stmt = conn.prepare(
        "SELECT canonical_key, message_id FROM deleted_keys \
         WHERE account_id = ?1 AND folder = 'drafts' AND remote_pending = 1 \
           AND message_id IS NOT NULL \
         ORDER BY deleted_at",
    )?;
    let rows = stmt.query_map(params![account_id], |r| {
        Ok(PendingRemoteDelete {
            canonical_key: r.get(0)?,
            message_id_inner: r.get(1)?,
        })
    })?;
    rows.collect()
}

/// サーバー側のコピー削除が済んだ墓標の再試行予約を下ろす（canonical_key 指定）。
pub fn mark_remote_deleted(
    conn: &Connection,
    account_id: i64,
    canonical_key: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE deleted_keys SET remote_pending = 0 \
         WHERE account_id = ?1 AND canonical_key = ?2",
        params![account_id, canonical_key],
    )?;
    Ok(())
}

/// 墓標を残す `INSERT ... SELECT` の共通部分（絞り込みだけ呼び出し側で足す）。
/// `?1` は削除時刻（Unix 秒）。対象は「下書き由来の行」＝`canonical_key` が `drafts:` で
/// 始まる行（ゴミ箱へ移したあとの下書きも含む）。
const INSERT_TOMBSTONES: &str = "INSERT OR REPLACE INTO deleted_keys \
       (account_id, canonical_key, message_id, folder, remote_pending, deleted_at) \
     SELECT account_id, \
            canonical_key, \
            NULLIF(ltrim(rtrim(COALESCE(message_id, ''), '>'), '<'), ''), \
            'drafts', \
            CASE WHEN COALESCE(message_id, '') = '' THEN 0 ELSE 1 END, \
            ?1 \
       FROM emails \
      WHERE canonical_key LIKE 'drafts:%'";

/// 役目を終えた墓標（サーバー削除済み＋[`KEEP_DAYS`] 経過）を掃除する。削除件数を返す。
pub fn purge_old_tombstones(conn: &Connection) -> rusqlite::Result<usize> {
    let cutoff = chrono::Utc::now().timestamp() - KEEP_DAYS * 86_400;
    conn.execute(
        "DELETE FROM deleted_keys WHERE remote_pending = 0 AND deleted_at < ?1",
        params![cutoff],
    )
}

impl Store {
    /// 指定メールのうち**下書き由来の行だけ**について墓標を残す（ローカル削除の直前に呼ぶ）。
    ///
    /// 対象は `canonical_key` が `drafts:` で始まる行（ゴミ箱へ移したあとの下書きも含む）。
    /// それ以外の行は何もしない。Message-ID を持つものは `remote_pending=1` として登録し、
    /// サーバー側コピーの削除を予約する（実際の削除は呼び出し側の背景タスク、失敗したぶんは
    /// 次回同期で再試行される）。
    ///
    /// # エラー
    /// SQLite の実行に失敗したときにエラーを返す。
    pub fn tombstone_drafts(&self, ids: &[i64]) -> rusqlite::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        // ?1 は削除時刻、?2 以降が id（SQLite の連番プレースホルダで明示する）。
        let placeholders = (2..=ids.len() + 1)
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("{INSERT_TOMBSTONES} AND id IN ({placeholders})");
        let mut args: Vec<i64> = Vec::with_capacity(ids.len() + 1);
        args.push(chrono::Utc::now().timestamp());
        args.extend_from_slice(ids);
        let conn = self.conn.lock().unwrap();
        conn.execute(&sql, params_from_iter(args))?;
        Ok(())
    }

    /// 指定フォルダを空にする直前に、その中の下書き由来の行へまとめて墓標を残す。
    /// `account_id` が None なら全アカウント。ゴミ箱を空にしたときに、そこへ移してあった
    /// 下書きがサーバーのコピーから復活するのを防ぐ。
    ///
    /// # エラー
    /// SQLite の実行に失敗したときにエラーを返す。
    pub fn tombstone_drafts_in_folder(
        &self,
        account_id: Option<i64>,
        folder: &str,
    ) -> rusqlite::Result<()> {
        let sql = format!("{INSERT_TOMBSTONES} AND folder = ?2 AND (?3 IS NULL OR account_id = ?3)");
        let conn = self.conn.lock().unwrap();
        conn.execute(
            &sql,
            params![chrono::Utc::now().timestamp(), folder, account_id],
        )?;
        Ok(())
    }

    /// サーバー側のコピー削除が済んだ下書き墓標の再試行予約を下ろす（Message-ID 指定）。
    /// 破棄直後の背景削除が成功したときに呼ぶ（失敗時は予約を残し、次回同期で再試行する）。
    ///
    /// # エラー
    /// SQLite の実行に失敗したときにエラーを返す。
    pub fn clear_tombstone_remote(
        &self,
        account_id: i64,
        message_id_inner: &str,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE deleted_keys SET remote_pending = 0 \
             WHERE account_id = ?1 AND message_id = ?2",
            params![account_id, message_id_inner],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::DraftInput;
    use crate::services::store::{insert_email, InsertOutcome, NewEmail};

    fn test_store() -> Store {
        let store = Store::open_in_memory_for_test();
        store
            .conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'me@b','i','s')",
                [],
            )
            .unwrap();
        store
    }

    /// 下書きを 1 通保存し、(id, canonical_key, Message-ID の中身) を返す。
    fn save_draft(store: &Store) -> (i64, String, String) {
        let id = store
            .save_draft(&DraftInput {
                draft_id: None,
                account_id: 1,
                to: vec!["you@example.com".to_string()],
                cc: vec![],
                bcc: vec![],
                subject: "書きかけ".to_string(),
                body: "本文".to_string(),
                in_reply_to: None,
                attachments: vec![],
            })
            .unwrap();
        let conn = store.conn.lock().unwrap();
        let (key, mid): (String, String) = conn
            .query_row(
                "SELECT canonical_key, message_id FROM emails WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let inner = mid.trim_start_matches('<').trim_end_matches('>').to_string();
        (id, key, inner)
    }

    /// 同期がサーバー Drafts から下書きを取り込むときの NewEmail 相当。
    fn inbound_draft(canonical_key: &str, message_id_inner: &str) -> NewEmail {
        NewEmail {
            account_id: 1,
            message_id: Some(message_id_inner.to_string()),
            // 同期側は folder_key() でフォルダ接頭辞を付けるため、ここでは接頭辞なしを渡す。
            canonical_key: canonical_key
                .strip_prefix("drafts:")
                .unwrap_or(canonical_key)
                .to_string(),
            subject: Some("書きかけ".to_string()),
            from_address: Some("me@b".to_string()),
            from_name: None,
            to_addresses: None,
            to_name: None,
            reply_to: None,
            cc_addresses: None,
            date: Some("2026-01-01 00:00:00".to_string()),
            date_ts: Some(1_767_225_600),
            body_plain: Some("本文".to_string()),
            clean_body: Some("本文".to_string()),
            body_html: None,
            auth_result: None,
            list_id: None,
            in_reply_to: None,
            references_ids: None,
            thread_index: None,
            raw_headers: None,
            quotes: vec![],
            has_attachments: false,
            is_read: true,
            uid: Some(42),
            folder: "drafts".to_string(),
            verified_self: false,
            attachments: vec![],
        }
    }

    /// 破棄した下書きは、サーバーに残ったコピーを同期が取り込もうとしても復活しない。
    #[test]
    fn discarded_draft_is_not_reimported() {
        let store = test_store();
        let (id, key, inner) = save_draft(&store);

        store.tombstone_drafts(&[id]).unwrap();
        store.delete_emails(&[id]).unwrap();

        let conn = store.conn.lock().unwrap();
        // 同期がサーバー Drafts のコピーを取り込もうとする → 墓標で弾かれる。
        let outcome = insert_email(&conn, &inbound_draft(&key, &inner)).unwrap();
        assert!(matches!(outcome, InsertOutcome::Unchanged));
        let n: i64 = conn
            .query_row("SELECT count(*) FROM emails", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "破棄した下書きが復活している");
    }

    /// 墓標はサーバー削除の再試行を予約し、削除できたら予約だけ下ろす（墓標自体は残す）。
    #[test]
    fn tombstone_reserves_remote_delete_until_cleared() {
        let store = test_store();
        let (id, key, inner) = save_draft(&store);
        store.tombstone_drafts(&[id]).unwrap();
        store.delete_emails(&[id]).unwrap();

        {
            let conn = store.conn.lock().unwrap();
            let pending = pending_remote_deletes(&conn, 1).unwrap();
            assert_eq!(pending.len(), 1);
            // 山括弧は落として保存する（サーバーの HEADER 検索にそのまま使うため）。
            assert_eq!(pending[0].message_id_inner, inner);
            assert_eq!(pending[0].canonical_key, key);
        }

        store.clear_tombstone_remote(1, &inner).unwrap();
        let conn = store.conn.lock().unwrap();
        assert!(pending_remote_deletes(&conn, 1).unwrap().is_empty());
        // 予約は下ろしても墓標は残る（以後の取り込みも弾き続ける）。
        assert!(is_tombstoned(&conn, 1, &key).unwrap());
    }

    /// 下書き以外（受信メール等）は墓標の対象外。
    #[test]
    fn non_draft_rows_are_not_tombstoned() {
        let store = test_store();
        let id = {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO emails (account_id, canonical_key, message_id, folder) \
                 VALUES (1, 'inbox@example.com', 'inbox@example.com', 'inbox')",
                [],
            )
            .unwrap();
            conn.last_insert_rowid()
        };
        store.tombstone_drafts(&[id]).unwrap();
        let conn = store.conn.lock().unwrap();
        assert!(!is_tombstoned(&conn, 1, "inbox@example.com").unwrap());
    }

    /// 役目を終えた墓標だけ掃除し、サーバー削除が未完了のものは残す。
    #[test]
    fn purge_old_keeps_pending_rows() {
        let store = test_store();
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO deleted_keys \
               (account_id, canonical_key, message_id, folder, remote_pending, deleted_at) \
             VALUES (1, 'drafts:old@example.com', 'old@example.com', 'drafts', 0, 0), \
                    (1, 'drafts:pending@example.com', 'pending@example.com', 'drafts', 1, 0)",
            [],
        )
        .unwrap();
        assert_eq!(purge_old_tombstones(&conn).unwrap(), 1);
        // サーバー削除が未完了のものは、いくら古くても掃除しない（再試行が要るため）。
        assert!(is_tombstoned(&conn, 1, "drafts:pending@example.com").unwrap());
    }
}
