use super::Store;
use crate::models::AccountSummary;
use rusqlite::{params, OptionalExtension};

/// アカウント挿入用（内部）。資格情報は含めない（keyring で別管理）。
pub struct NewAccount {
    pub email: String,
    pub display_name: Option<String>,
    pub username: Option<String>,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub server_account_id: Option<i64>,
}

/// 送信に必要なアカウント情報。email は keyring のキー、from_* は差出人ヘッダ用。
pub struct SmtpAccount {
    /// 資格情報キー（keyring のユーザー名）＝アカウントのメールアドレス。
    pub email: String,
    /// ログイン用サーバーユーザー名（未設定なら email）。
    pub login_user: String,
    /// 差出人の表示名（未設定なら None）。
    pub display_name: Option<String>,
    pub smtp_host: String,
    pub smtp_port: u16,
    /// 'ssl' | 'starttls' | その他。server_accounts から取得（既定 'starttls'）。
    pub smtp_security: String,
}

impl Store {
    pub fn insert_account(&self, a: &NewAccount) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        // 新規アカウントの初回同期は現行パイプラインで行われるので、データ形式は現行として記録。
        conn.execute(
            "INSERT INTO accounts (email, display_name, username, imap_host, imap_port, smtp_host, smtp_port, server_account_id, ingest_version, parse_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                a.email,
                a.display_name,
                a.username,
                a.imap_host,
                a.imap_port,
                a.smtp_host,
                a.smtp_port,
                a.server_account_id,
                crate::services::dataver::INGEST_VERSION,
                crate::services::dataver::PARSE_VERSION
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// データ形式バージョン（ingest, parse）の記録を取得。アカウントが無ければ None。
    pub fn data_versions(&self, id: i64) -> rusqlite::Result<Option<(i64, i64)>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT ingest_version, parse_version FROM accounts WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
    }

    /// 全体再取り込みの完了を記録（取り込み形式・解析とも現行バージョンに）。
    /// 中断された再取り込みでは呼ばないこと（データが現行形式に揃っていないため）。
    pub fn mark_resynced(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET ingest_version = ?1, parse_version = ?2 WHERE id = ?3",
            params![
                crate::services::dataver::INGEST_VERSION,
                crate::services::dataver::PARSE_VERSION,
                id
            ],
        )?;
        Ok(())
    }

    /// ローカル再解析の完了を記録（解析バージョンのみ現行に）。
    pub fn mark_reprocessed(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET parse_version = ?1 WHERE id = ?2",
            params![crate::services::dataver::PARSE_VERSION, id],
        )?;
        Ok(())
    }

    /// 同期に必要な IMAP 接続情報（email, login_user, host, port）を取得。
    /// login_user = username があればそれ、無ければ email。
    pub fn get_account_imap(
        &self,
        id: i64,
    ) -> rusqlite::Result<Option<(String, String, String, u16)>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT email, COALESCE(NULLIF(username, ''), email), imap_host, imap_port
             FROM accounts WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)? as u16,
                ))
            },
        )
        .map(Some)
        .or_else(|e| {
            if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                Ok(None)
            } else {
                Err(e)
            }
        })
    }

    /// 送信に必要な SMTP 接続情報を取得。smtp_security は紐づく server_accounts から。
    pub fn get_account_smtp(&self, id: i64) -> rusqlite::Result<Option<SmtpAccount>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT a.email, COALESCE(NULLIF(a.username, ''), a.email), a.display_name,
                    a.smtp_host, a.smtp_port, COALESCE(s.smtp_security, 'starttls')
             FROM accounts a
             LEFT JOIN server_accounts s ON s.id = a.server_account_id
             WHERE a.id = ?1",
            params![id],
            |r| {
                Ok(SmtpAccount {
                    email: r.get(0)?,
                    login_user: r.get(1)?,
                    display_name: r.get(2)?,
                    smtp_host: r.get(3)?,
                    smtp_port: r.get::<_, i64>(4)? as u16,
                    smtp_security: r.get(5)?,
                })
            },
        )
        .optional()
    }

    /// アカウントの自己検証用 HMAC 秘密（16 進）を返す。未生成なら生成して保存する。
    /// 自分宛メールに付ける X-Rondine-Self の署名鍵（docs/SPAM.md）。
    pub fn get_or_create_self_secret(&self, account_id: i64) -> rusqlite::Result<String> {
        let conn = self.conn.lock().unwrap();
        let existing: Option<String> = conn
            .query_row(
                "SELECT self_secret FROM accounts WHERE id = ?1",
                params![account_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        if let Some(s) = existing.filter(|s| !s.is_empty()) {
            return Ok(s);
        }
        let secret = crate::services::selfmark::generate_secret_hex();
        conn.execute(
            "UPDATE accounts SET self_secret = ?1 WHERE id = ?2",
            params![secret, account_id],
        )?;
        Ok(secret)
    }

    pub fn list_accounts(&self) -> rusqlite::Result<Vec<AccountSummary>> {
        // 参照専用接続（左下カウント等の読み取りは書き込みに待たされない）。
        let conn = self.read_conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, email, display_name, imap_host, smtp_host, COALESCE(sync_window,'6m'),
                    COALESCE(full_window,'all'), COALESCE(body_window,'off'), signature_id,
                    (SELECT COUNT(*) FROM emails e WHERE e.account_id = accounts.id AND e.is_read = 0
                       AND COALESCE(e.folder,'inbox') = 'inbox' AND e.is_junk = 0),
                    (SELECT COUNT(*) FROM emails e WHERE e.account_id = accounts.id
                       AND COALESCE(e.folder,'inbox') = 'inbox' AND e.is_junk = 0),
                    -- 未同期のアカウントは folder_sync に行が無く、相関サブクエリ自体が NULL に
                    -- なる（COALESCE をサブクエリの内側に置くと拾えない）。外側で 0 に畳む。
                    COALESCE((SELECT server_total FROM folder_sync fs
                       WHERE fs.account_id = accounts.id AND fs.folder = 'inbox'), 0)
             FROM accounts ORDER BY COALESCE(sort_order, id), id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(AccountSummary {
                id: r.get::<_, i64>(0)? as i32,
                email: r.get(1)?,
                display_name: r.get(2)?,
                imap_host: r.get(3)?,
                smtp_host: r.get(4)?,
                sync_window: r.get(5)?,
                full_window: r.get(6)?,
                body_window: r.get(7)?,
                signature_id: r.get::<_, Option<i64>>(8)?.map(|v| v as i32),
                unread_count: r.get::<_, i64>(9)? as i32,
                total_count: r.get::<_, i64>(10)? as i32,
                server_total_count: r.get::<_, i64>(11)? as i32,
            })
        })?;
        rows.collect()
    }

    /// ホームのアカウント別バッジ用: inbox（is_junk=0）の未読メールを、対象カテゴリ別に数える。
    /// all=全体 / known=住所録一致 / vip=お気に入り一致 / green=住所録本人 または ドメインが
    /// グリーン集合（手動グリーン ∪ 住所録由来ドメイン − フリーメール − 警告）に属する。
    /// read_conn は query_only（書き込み不可）のため、グリーン集合はパラメータ化した IN リスト
    /// でドメイン一致を判定する（一時表は使わない）。
    pub fn home_unread_counts(&self) -> rusqlite::Result<Vec<crate::models::HomeUnreadCounts>> {
        let conn = self.read_conn.lock().unwrap();
        let green: Vec<String> = super::greendomain::green_domain_set(&conn)?
            .into_iter()
            .collect();
        // 住所録一致（known）／お気に入り一致（vip）の EXISTS 断片（emails.rs の known_vip_cols と同義）。
        let known = "(EXISTS(SELECT 1 FROM contacts c WHERE c.deleted_at IS NULL AND lower(c.email)=lower(e.from_address)) \
                      OR EXISTS(SELECT 1 FROM contact_emails ce JOIN contacts c3 ON c3.id=ce.contact_id \
                                WHERE c3.deleted_at IS NULL AND lower(ce.value)=lower(e.from_address)))";
        let vip = "(EXISTS(SELECT 1 FROM contacts c WHERE c.deleted_at IS NULL AND c.is_favorite=1 AND lower(c.email)=lower(e.from_address)) \
                    OR EXISTS(SELECT 1 FROM contact_emails ce JOIN contacts c2 ON c2.id=ce.contact_id \
                              WHERE c2.deleted_at IS NULL AND c2.is_favorite=1 AND lower(ce.value)=lower(e.from_address)))";
        // グリーンドメイン一致: 差出人ドメインが集合に含まれるか（空集合なら住所録本人のみ green）。
        let green_domain = if green.is_empty() {
            "1=0".to_string()
        } else {
            let ph = vec!["?"; green.len()].join(",");
            format!("lower(substr(e.from_address, instr(e.from_address,'@')+1)) IN ({ph})")
        };
        let sql = format!(
            "SELECT e.account_id, COUNT(*), \
                    SUM(CASE WHEN {known} OR {green_domain} THEN 1 ELSE 0 END), \
                    SUM(CASE WHEN {known} THEN 1 ELSE 0 END), \
                    SUM(CASE WHEN {vip} THEN 1 ELSE 0 END) \
             FROM emails e \
             WHERE COALESCE(e.folder,'inbox')='inbox' AND e.is_junk=0 AND e.is_read=0 \
             GROUP BY e.account_id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(green.iter()), |r| {
            Ok(crate::models::HomeUnreadCounts {
                account_id: r.get::<_, i64>(0)? as i32,
                all: r.get::<_, i64>(1)? as i32,
                green: r.get::<_, i64>(2)? as i32,
                known: r.get::<_, i64>(3)? as i32,
                vip: r.get::<_, i64>(4)? as i32,
            })
        })?;
        rows.collect()
    }

    /// アカウントの並び順を設定する（渡された ID 順に sort_order を 0,1,2... で振る）。
    pub fn reorder_accounts(&self, ids: &[i64]) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("UPDATE accounts SET sort_order = ?1 WHERE id = ?2")?;
            for (i, id) in ids.iter().enumerate() {
                stmt.execute(params![i as i64, id])?;
            }
        }
        tx.commit()
    }

    /// アカウントの編集（差出人名・既定署名）。
    pub fn update_account(
        &self,
        id: i64,
        display_name: Option<&str>,
        signature_id: Option<i64>,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET display_name = ?1, signature_id = ?2 WHERE id = ?3",
            params![display_name, signature_id, id],
        )?;
        Ok(())
    }

    /// 同期範囲を変更。次回同期で新範囲を初回取得し直せるよう UID 状態もリセットする。
    /// アカウントと、その受信メール（FTS含む）を削除する。
    /// メールアドレスから既存アカウントの id を引く（大文字小文字は無視）。
    /// 同じアドレスを二重に登録させないための確認に使う。
    pub fn find_account_id_by_email(&self, email: &str) -> rusqlite::Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id FROM accounts WHERE lower(email) = lower(?1) LIMIT 1",
            params![email],
            |r| r.get(0),
        )
        .optional()
    }

    /// 同じメールアドレスを使う他のアカウントの件数（`except_id` を除く）。
    ///
    /// keyring のキーはメールアドレスなので、重複登録があるときに 1 件消しただけで
    /// 資格情報を消すと、残した側までログインできなくなる（実測 2026-09-01）。
    /// 削除前にこれで確認する。
    pub fn count_accounts_with_email(&self, email: &str, except_id: i64) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT count(*) FROM accounts WHERE lower(email) = lower(?1) AND id <> ?2",
            params![email, except_id],
            |r| r.get(0),
        )
    }

    /// アカウントと、そのアカウントに属するデータを削除する。
    ///
    /// **子から順に消す。**`message_quotes` は `emails` への外部キー（ON DELETE なし）を
    /// 持つため、先に消さないと `DELETE FROM emails` が FOREIGN KEY constraint failed で
    /// 失敗する（引用ブロックはほぼ全メールに付くので、実質すべてのアカウントが消せなかった）。
    ///
    /// 途中で失敗したときに中途半端な状態を残さないよう、全体を 1 トランザクションで行う。
    pub fn delete_account(&self, id: i64) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let of_account = "SELECT id FROM emails WHERE account_id=?1";
        // メール本体にぶら下がるもの（FTS は外部コンテンツ表なので rowid で消す）。
        for sql in [
            format!("DELETE FROM email_fts WHERE rowid IN ({of_account})"),
            format!("DELETE FROM message_quotes WHERE email_id IN ({of_account})"),
            format!("DELETE FROM attachments WHERE email_id IN ({of_account})"),
            format!("DELETE FROM email_tags WHERE email_id IN ({of_account})"),
        ] {
            tx.execute(&sql, params![id])?;
        }
        // メール本体 → アカウント単位のもの → アカウント本体。
        for sql in [
            "DELETE FROM emails WHERE account_id=?1",
            "DELETE FROM folder_sync WHERE account_id=?1",
            "DELETE FROM logical_threads WHERE account_id=?1",
            "DELETE FROM deleted_keys WHERE account_id=?1",
            "DELETE FROM accounts WHERE id=?1",
        ] {
            tx.execute(sql, params![id])?;
        }
        // 送信履歴の索引はアドレス単位でアカウントに紐づかないので、消したアカウントの分
        // だけを抜けない。残ったメールから作り直す（アカウント削除は稀な操作）。
        super::sent_addresses::rebuild(&tx)?;
        tx.commit()
    }

    pub fn set_sync_window(&self, id: i64, window: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET sync_window=?1, uid_validity=NULL, last_uid=NULL WHERE id=?2",
            params![window, id],
        )?;
        // フォルダ別の同期状態も消し、新しい範囲で全フォルダを取り直す。
        conn.execute("DELETE FROM folder_sync WHERE account_id=?1", params![id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::dataver;

    fn test_store() -> Store {
        Store::open_in_memory_for_test()
    }

    #[test]
    fn data_versions_track_rebuild_lifecycle() {
        let store = test_store();
        // バージョン記録の導入前に作られたアカウント（列は既定 0 ＝形式不明）。
        store
            .conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'a@b','i','s')",
                [],
            )
            .unwrap();
        assert_eq!(store.data_versions(1).unwrap(), Some((0, 0)));
        assert!(dataver::needs_resync(0));

        // ローカル再解析は解析バージョンだけを現行にする（取り込み形式は不明のまま）。
        store.mark_reprocessed(1).unwrap();
        assert_eq!(
            store.data_versions(1).unwrap(),
            Some((0, dataver::PARSE_VERSION))
        );
        assert!(dataver::needs_resync(0));

        // 全体再取り込みの完走で両方とも現行になり、以後はローカル再解析で足りる。
        store.mark_resynced(1).unwrap();
        assert_eq!(
            store.data_versions(1).unwrap(),
            Some((dataver::INGEST_VERSION, dataver::PARSE_VERSION))
        );
        assert!(!dataver::needs_resync(dataver::INGEST_VERSION));
    }

    #[test]
    fn new_accounts_start_at_current_versions() {
        let store = test_store();
        let id = store
            .insert_account(&NewAccount {
                email: "x@y".into(),
                display_name: None,
                username: None,
                imap_host: "i".into(),
                imap_port: 993,
                smtp_host: "s".into(),
                smtp_port: 587,
                server_account_id: None,
            })
            .unwrap();
        assert_eq!(
            store.data_versions(id).unwrap(),
            Some((dataver::INGEST_VERSION, dataver::PARSE_VERSION))
        );
    }

    /// 追加直後（一度も同期していない）のアカウントも一覧に出ること。
    /// folder_sync に行が無いと相関サブクエリが NULL になり、i64 での取り出しが
    /// InvalidColumnType で落ちていた（アカウントを追加しても画面に出ない不具合）。
    #[test]
    fn list_accounts_includes_never_synced_account() {
        let store = test_store();
        store
            .insert_account(&NewAccount {
                email: "new@example.com".into(),
                display_name: None,
                username: None,
                imap_host: "imap.example.com".into(),
                imap_port: 993,
                smtp_host: "smtp.example.com".into(),
                smtp_port: 587,
                server_account_id: None,
            })
            .unwrap();

        let accounts = store.list_accounts().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].email, "new@example.com");
        // 未同期なのでサーバ側総数は 0 として扱う（NULL で落とさない）。
        assert_eq!(accounts[0].server_total_count, 0);
        assert_eq!(accounts[0].unread_count, 0);
        assert_eq!(accounts[0].total_count, 0);
    }

    /// 引用ブロック（message_quotes）が残っていてもアカウントを削除できること。
    /// message_quotes は emails への外部キーを持つので、先に消さないと
    /// 「DELETE FROM emails」が FOREIGN KEY constraint failed で失敗し、
    /// UI 上は「− を押しても無反応」になっていた（2026-09-01 の実測）。
    #[test]
    fn delete_account_removes_rows_that_reference_emails() {
        let store = test_store();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'a@b','i','s')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO emails (id, account_id, canonical_key, folder, subject)
                 VALUES (10, 1, 'k1', 'inbox', '件名')",
                [],
            )
            .unwrap();
            // 引用ブロック（外部キーで emails を参照。ここが削除を止めていた）。
            conn.execute(
                "INSERT INTO message_quotes (email_id, block_order) VALUES (10, 0)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO logical_threads (id, account_id) VALUES (5, 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO folder_sync (account_id, folder) VALUES (1, 'inbox')",
                [],
            )
            .unwrap();
        }

        store.delete_account(1).unwrap();

        let conn = store.conn.lock().unwrap();
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT count(*) FROM accounts"), 0);
        assert_eq!(count("SELECT count(*) FROM emails"), 0);
        assert_eq!(count("SELECT count(*) FROM message_quotes"), 0);
        assert_eq!(count("SELECT count(*) FROM logical_threads"), 0);
        assert_eq!(count("SELECT count(*) FROM folder_sync"), 0);
    }

    /// 他アカウントのデータは巻き添えにしない。
    #[test]
    fn delete_account_leaves_other_accounts_intact() {
        let store = test_store();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO accounts (id, email, imap_host, smtp_host)
                 VALUES (1,'a@b','i','s'), (2,'c@d','i','s')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO emails (id, account_id, canonical_key, folder)
                 VALUES (10, 1, 'k1', 'inbox'), (11, 2, 'k1', 'inbox')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO message_quotes (email_id, block_order) VALUES (10, 0), (11, 0)",
                [],
            )
            .unwrap();
        }

        store.delete_account(1).unwrap();

        let conn = store.conn.lock().unwrap();
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT count(*) FROM accounts"), 1);
        assert_eq!(count("SELECT count(*) FROM emails WHERE account_id = 2"), 1);
        assert_eq!(count("SELECT count(*) FROM message_quotes"), 1);
    }

    /// アカウントを消したら、そのアカウントの送信履歴も「返信歴あり」の索引から消える
    /// （索引はアドレス単位なので、残ったメールから作り直す）。
    #[test]
    fn delete_account_rebuilds_sent_address_index() {
        let store = test_store();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO accounts (id, email, imap_host, smtp_host)
                 VALUES (1,'a@b','i','s'), (2,'c@d','i','s')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO emails (id, account_id, canonical_key, folder, to_addresses)
                 VALUES (10, 1, 'k1', 'sent', 'gone@x.com'), (11, 2, 'k1', 'sent', 'kept@x.com')",
                [],
            )
            .unwrap();
            super::super::sent_addresses::rebuild(&conn).unwrap();
        }

        store.delete_account(1).unwrap();

        let conn = store.conn.lock().unwrap();
        let addrs: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT address FROM sent_addresses ORDER BY address")
                .unwrap();
            let rows = stmt.query_map([], |r| r.get(0)).unwrap();
            rows.collect::<rusqlite::Result<_>>().unwrap()
        };
        assert_eq!(addrs, vec!["kept@x.com".to_string()]);
    }

    /// 二重登録の確認は大文字小文字を無視する（同じ受信箱を別物として扱わない）。
    #[test]
    fn find_account_id_by_email_ignores_case() {
        let store = test_store();
        store
            .conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO accounts (id, email, imap_host, smtp_host)
                 VALUES (1,'Suematsu@SNG-Design.com','i','s')",
                [],
            )
            .unwrap();
        assert_eq!(
            store
                .find_account_id_by_email("suematsu@sng-design.com")
                .unwrap(),
            Some(1)
        );
        assert_eq!(store.find_account_id_by_email("other@example.com").unwrap(), None);
    }

    /// 同じアドレスのアカウントが他にも残っているかを数えられること。
    /// keyring のキーはメールアドレスなので、削除時にこれを見ないと、重複を 1 件消しただけで
    /// 残した側の資格情報まで巻き添えで消える（実測 2026-09-01）。
    #[test]
    fn count_accounts_with_email_excludes_itself() {
        let store = test_store();
        store
            .conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES
                   (1,'me@example.com','i','s'),
                   (2,'ME@Example.com','i','s'),
                   (3,'other@example.com','i','s')",
                [],
            )
            .unwrap();
        // 1 を消そうとしている時点で、同じアドレスは 2 が残っている（大文字小文字は無視）。
        assert_eq!(
            store.count_accounts_with_email("me@example.com", 1).unwrap(),
            1
        );
        // 2 も消した後を想定すると、残りは 0（このとき初めて資格情報を消してよい）。
        store.delete_account(2).unwrap();
        assert_eq!(
            store.count_accounts_with_email("me@example.com", 1).unwrap(),
            0
        );
        // 別アドレスは巻き込まない。
        assert_eq!(
            store
                .count_accounts_with_email("other@example.com", 1)
                .unwrap(),
            1
        );
    }
}
