//! Google 連携アカウント（カレンダー・連絡先で共有）。
//!
//! アカウント 1 件 = refresh_token 1 本。カレンダーと連絡先で同じ Google アカウントを
//! 使い回し、サービスごとに有効フラグと最終同期時刻だけを分けて持つ（マイグレーション 0055）。
//! refresh_token 自体は keyring 側にあり、この層はメタデータのみを扱う。
//!
//! 解除は 2 段（マイグレーション 0061）:
//! - **解除中**（既定。[`Store::disconnect_google_account`]）: 行・カレンダー・予定・連絡先の
//!   つながり・未送信の変更を残し、同期の対象から外す。同じアカウントで連携し直すと
//!   ([`Store::upsert_google_account`]) 印が消えて、そのまま使い直せる
//! - **完全に解除**（[`Store::purge_google_account`]）: つながりを外し、カレンダーと予定の写し・
//!   行を消す。どちらも Google 側の連絡先・予定には触れない

use super::Store;
use crate::models::GoogleAccount;
use rusqlite::{params, OptionalExtension, Row};

/// 1 つの Google アカウントが兼ねる同期サービス。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoogleService {
    Calendar,
    Contacts,
}

impl GoogleService {
    /// 有効フラグを保持する列名。
    const fn enabled_column(self) -> &'static str {
        match self {
            Self::Calendar => "sync_calendar",
            Self::Contacts => "sync_contacts",
        }
    }

    /// 最終同期時刻を保持する列名。
    const fn last_sync_column(self) -> &'static str {
        match self {
            Self::Calendar => "last_calendar_sync_at",
            Self::Contacts => "last_contacts_sync_at",
        }
    }
}

fn row_to_account(r: &Row) -> rusqlite::Result<GoogleAccount> {
    Ok(GoogleAccount {
        id: r.get::<_, i64>(0)? as i32,
        email: r.get(1)?,
        sync_calendar: r.get::<_, i64>(2)? != 0,
        sync_contacts: r.get::<_, i64>(3)? != 0,
        last_calendar_sync_at: r.get(4)?,
        last_contacts_sync_at: r.get(5)?,
        push_new_contacts: r.get::<_, i64>(6)? != 0,
        disconnected_at: r.get(7)?,
    })
}

/// 一覧・単票で共通に使う選択列（row_to_account の並びと対応）。
const ACCOUNT_COLUMNS: &str =
    "id, email, sync_calendar, sync_contacts, last_calendar_sync_at, last_contacts_sync_at, \
     push_new_contacts, disconnected_at";

impl Store {
    /// Google アカウントを登録（既存なら external_id と許可スコープを更新）し、行 id を返す。
    ///
    /// 同じアカウント（メールアドレスで同一判定）が解除中なら、その行を使い直して解除の印を
    /// 消す。行の id が変わらないので、連絡先のつながり・カレンダー・同期の印はそのまま生きる。
    ///
    /// `granted_scopes` は同意で実際に許可されたスコープ（スペース区切り）。要求と一致しない
    /// ことがあるため、トークン応答の値をそのまま記録する。後から別サービスを有効化する際に
    /// 再同意が要るかの判定に使う。
    pub fn upsert_google_account(
        &self,
        email: &str,
        external_id: Option<&str>,
        granted_scopes: Option<&str>,
    ) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO google_accounts (provider, email, external_id, granted_scopes) \
             VALUES ('google', ?1, ?2, ?3) \
             ON CONFLICT(provider, email) DO UPDATE SET \
                 external_id = COALESCE(?2, external_id), \
                 granted_scopes = COALESCE(?3, granted_scopes), \
                 disconnected_at = NULL",
            params![email, external_id, granted_scopes],
        )?;
        conn.query_row(
            "SELECT id FROM google_accounts WHERE provider = 'google' AND email = ?1",
            params![email],
            |r| r.get(0),
        )
    }

    /// 連携済み Google アカウント一覧。
    pub fn list_google_accounts(&self) -> rusqlite::Result<Vec<GoogleAccount>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {ACCOUNT_COLUMNS} FROM google_accounts WHERE provider = 'google' ORDER BY id"
        ))?;
        let rows = stmt.query_map([], row_to_account)?;
        rows.collect()
    }

    /// 指定サービスの同期が有効で、解除中でないアカウントだけを返す（自動同期の対象選別用）。
    pub fn list_google_accounts_for(
        &self,
        service: GoogleService,
    ) -> rusqlite::Result<Vec<GoogleAccount>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {ACCOUNT_COLUMNS} FROM google_accounts \
             WHERE provider = 'google' AND {} = 1 AND disconnected_at IS NULL ORDER BY id",
            service.enabled_column()
        ))?;
        let rows = stmt.query_map([], row_to_account)?;
        rows.collect()
    }

    /// アカウント 1 件。無ければ None。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn google_account(&self, account_id: i64) -> rusqlite::Result<Option<GoogleAccount>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            &format!("SELECT {ACCOUNT_COLUMNS} FROM google_accounts WHERE id = ?1"),
            params![account_id],
            row_to_account,
        )
        .optional()
    }

    /// アカウントのメールアドレス（keyring キー）を引く。
    pub fn google_account_email(&self, account_id: i64) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT email FROM google_accounts WHERE id = ?1",
            params![account_id],
            |r| r.get(0),
        )
        .optional()
    }

    /// 許可済みスコープ（スペース区切り）。未記録（0055 以前の連携）なら None。
    pub fn google_account_scopes(&self, account_id: i64) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT granted_scopes FROM google_accounts WHERE id = ?1",
            params![account_id],
            |r| r.get(0),
        )
        .optional()
        .map(Option::flatten)
    }

    /// 指定サービスの同期有効フラグを切り替える。
    pub fn set_google_account_service(
        &self,
        account_id: i64,
        service: GoogleService,
        enabled: bool,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            &format!(
                "UPDATE google_accounts SET {} = ?2 WHERE id = ?1",
                service.enabled_column()
            ),
            params![account_id, i64::from(enabled)],
        )?;
        Ok(())
    }

    /// 指定サービスの最終同期時刻を現在時刻に更新。
    pub fn touch_google_account_synced(
        &self,
        account_id: i64,
        service: GoogleService,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            &format!(
                "UPDATE google_accounts SET {} = CURRENT_TIMESTAMP WHERE id = ?1",
                service.last_sync_column()
            ),
            params![account_id],
        )?;
        Ok(())
    }

    /// アカウントが解除中か（行が無いときも同期できないので true）。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn google_account_disconnected(&self, account_id: i64) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let at: Option<Option<String>> = conn
            .query_row(
                "SELECT disconnected_at FROM google_accounts WHERE id = ?1",
                params![account_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(!matches!(at, Some(None)))
    }

    /// アカウントを「解除中」にする。行・カレンダー・予定・連絡先のつながり・未送信の変更は
    /// 残し、同期の対象から外す（refresh token の削除は呼び出し側＝keyring の層）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn disconnect_google_account(&self, account_id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE google_accounts SET disconnected_at = CURRENT_TIMESTAMP \
             WHERE id = ?1 AND disconnected_at IS NULL",
            params![account_id],
        )?;
        Ok(())
    }

    /// アカウントを「完全に解除」する。
    ///
    /// - 連絡先のつながり（`contact_identities` / `contact_group_identities`）を外す。連絡先は
    ///   Rondine のみとして残し、送信待ちの印はほかのつながりの分だけに戻す
    /// - そのアカウントの Google カレンダーと予定の写しを消す（未送信の予定の変更も消える）
    /// - アカウントの行を消す
    ///
    /// ローカル専用のカレンダー・予定と、Google 側の連絡先・予定には触れない。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき（全体を巻き戻す）。
    pub fn purge_google_account(&self, account_id: i64) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let touched: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT DISTINCT contact_id FROM contact_identities \
                 WHERE provider = 'google' AND account_id = ?1 AND contact_id IS NOT NULL",
            )?;
            let rows = stmt.query_map(params![account_id], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        tx.execute(
            "DELETE FROM contact_identities WHERE provider = 'google' AND account_id = ?1",
            params![account_id],
        )?;
        tx.execute(
            "DELETE FROM contact_group_identities WHERE provider = 'google' AND account_id = ?1",
            params![account_id],
        )?;
        // 本体の送信待ちは「残ったつながりのどれかが送信待ち」にそろえる（外したアカウントへ
        // 送るはずだった変更が、ほかのアカウントへ新規作成として流れないように）。
        touched.iter().try_for_each(|cid| {
            tx.execute(
                "UPDATE contacts SET dirty = EXISTS( \
                     SELECT 1 FROM contact_identities WHERE contact_id = ?1 AND dirty = 1) \
                 WHERE id = ?1",
                params![cid],
            )
            .map(|_| ())
        })?;
        tx.execute(
            "DELETE FROM events WHERE calendar_id IN \
                (SELECT id FROM calendars WHERE account_id = ?1)",
            params![account_id],
        )?;
        tx.execute(
            "DELETE FROM calendars WHERE account_id = ?1",
            params![account_id],
        )?;
        tx.execute(
            "DELETE FROM google_accounts WHERE id = ?1",
            params![account_id],
        )?;
        tx.commit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_store() -> Store {
        Store::open_in_memory_for_test()
    }

    #[test]
    fn upsert_is_idempotent_per_email() {
        let s = mem_store();
        let id = s
            .upsert_google_account("a@gmail.com", Some("sub123"), Some("openid email"))
            .unwrap();
        assert!(id > 0);
        // 同じメールなら同じ行（external_id / スコープを更新）。
        let id2 = s
            .upsert_google_account("a@gmail.com", Some("sub456"), None)
            .unwrap();
        assert_eq!(id, id2);

        let accts = s.list_google_accounts().unwrap();
        assert_eq!(accts.len(), 1);
        assert_eq!(accts[0].email, "a@gmail.com");
        // 既存アカウントはカレンダー有効・連絡先無効が既定（マイグレーション 0055）。
        assert!(accts[0].sync_calendar);
        assert!(!accts[0].sync_contacts);
    }

    #[test]
    fn granted_scopes_survive_a_reconnect_without_scopes() {
        let s = mem_store();
        let id = s
            .upsert_google_account("a@gmail.com", None, Some("openid email calendar"))
            .unwrap();
        // None を渡した再連携でスコープを消さない（COALESCE）。
        s.upsert_google_account("a@gmail.com", None, None).unwrap();
        assert_eq!(
            s.google_account_scopes(id).unwrap().as_deref(),
            Some("openid email calendar")
        );
    }

    #[test]
    fn service_flags_and_last_sync_are_independent() {
        let s = mem_store();
        let id = s.upsert_google_account("a@gmail.com", None, None).unwrap();

        // 連絡先だけを有効化しても、カレンダーの状態は動かない。
        s.set_google_account_service(id, GoogleService::Contacts, true)
            .unwrap();
        let a = &s.list_google_accounts().unwrap()[0];
        assert!(a.sync_calendar);
        assert!(a.sync_contacts);

        // 最終同期時刻もサービスごとに独立している。
        s.touch_google_account_synced(id, GoogleService::Contacts)
            .unwrap();
        let a = &s.list_google_accounts().unwrap()[0];
        assert!(a.last_contacts_sync_at.is_some());
        assert!(a.last_calendar_sync_at.is_none());
    }

    #[test]
    fn list_for_filters_by_service() {
        let s = mem_store();
        let cal_only = s.upsert_google_account("cal@gmail.com", None, None).unwrap();
        let both = s.upsert_google_account("both@gmail.com", None, None).unwrap();
        s.set_google_account_service(both, GoogleService::Contacts, true)
            .unwrap();
        s.set_google_account_service(cal_only, GoogleService::Contacts, false)
            .unwrap();

        let cal = s.list_google_accounts_for(GoogleService::Calendar).unwrap();
        assert_eq!(cal.len(), 2);
        let con = s.list_google_accounts_for(GoogleService::Contacts).unwrap();
        assert_eq!(con.len(), 1);
        assert_eq!(con[0].email, "both@gmail.com");
    }

    /// 連絡先を 1 件作り、Google アカウントにつなぐ（dirty は送信待ちの印）。
    fn linked_contact(s: &Store, account_id: i64, name: &str, dirty: bool) -> i64 {
        let c = s
            .upsert_contact(&crate::services::store::test_support::person(name, &[]))
            .unwrap();
        let conn = s.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO contact_identities (provider, account_id, external_id, contact_id, dirty) \
             VALUES ('google', ?1, ?2, ?3, ?4)",
            params![account_id, format!("people/{name}"), c.id, i64::from(dirty)],
        )
        .unwrap();
        conn.execute(
            "UPDATE contacts SET dirty = ?2 WHERE id = ?1",
            params![c.id, i64::from(dirty)],
        )
        .unwrap();
        i64::from(c.id)
    }

    fn dirty_of(s: &Store, contact_id: i64) -> (i64, Vec<i64>) {
        let conn = s.conn.lock().unwrap();
        let body = conn
            .query_row(
                "SELECT dirty FROM contacts WHERE id = ?1",
                params![contact_id],
                |r| r.get(0),
            )
            .unwrap();
        let mut stmt = conn
            .prepare("SELECT dirty FROM contact_identities WHERE contact_id = ?1 ORDER BY id")
            .unwrap();
        let links = stmt
            .query_map(params![contact_id], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        (body, links)
    }

    #[test]
    fn disconnect_keeps_links_and_reconnect_reuses_the_row() {
        let s = mem_store();
        let id = s
            .upsert_google_account("a@gmail.com", None, Some("openid"))
            .unwrap();
        // 後から連携した別アカウント（最大 id を消すと振り直される、の逆を確かめる）。
        let other = s.upsert_google_account("b@gmail.com", None, None).unwrap();
        let c = linked_contact(&s, id, "山田", true);

        s.disconnect_google_account(id).unwrap();
        assert!(s.google_account_disconnected(id).unwrap());
        assert!(!s.google_account_disconnected(other).unwrap());
        // 解除中は自動同期の対象外。一覧には解除中として残る。
        let cal = s.list_google_accounts_for(GoogleService::Calendar).unwrap();
        assert_eq!(
            cal.iter().map(|a| i64::from(a.id)).collect::<Vec<_>>(),
            vec![other]
        );
        let all = s.list_google_accounts().unwrap();
        assert!(all
            .iter()
            .any(|a| i64::from(a.id) == id && a.disconnected_at.is_some()));
        // つながりも送信待ちの印も残る。
        assert_eq!(dirty_of(&s, c), (1, vec![1]));
        let links = s.get_contact(c).unwrap().links;
        assert_eq!(links.len(), 1);
        assert!(links[0].disconnected);
        assert_eq!(links[0].account_email.as_deref(), Some("a@gmail.com"));

        // 同じアカウントで連携し直すと、同じ行を使い直して印が消える。
        let again = s.upsert_google_account("a@gmail.com", None, None).unwrap();
        assert_eq!(again, id);
        assert!(!s.google_account_disconnected(id).unwrap());
        assert!(!s.get_contact(c).unwrap().links[0].disconnected);
        assert_eq!(
            dirty_of(&s, c),
            (1, vec![1]),
            "未送信の変更は再接続後の同期で送る"
        );
    }

    #[test]
    fn purge_unlinks_contacts_and_settles_their_dirty_flag() {
        let s = mem_store();
        let a = s.upsert_google_account("a@gmail.com", None, None).unwrap();
        let b = s.upsert_google_account("b@gmail.com", None, None).unwrap();
        // a だけにつながっていて送信待ち → 外すと Rondine のみ、送信待ちも落ちる。
        let only_a = linked_contact(&s, a, "田中", true);
        // a（送信待ち）と b（送信済み）の両方 → b のつながりは残り、b へは送るものが無い。
        let both = linked_contact(&s, a, "佐藤", true);
        {
            let conn = s.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO contact_identities (provider, account_id, external_id, contact_id) \
                 VALUES ('google', ?1, 'people/b-sato', ?2)",
                params![b, both],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO contact_group_identities (provider, account_id, external_id, name) \
                 VALUES ('google', ?1, 'contactGroups/1', '家族')",
                params![a],
            )
            .unwrap();
        }

        s.purge_google_account(a).unwrap();
        assert_eq!(
            s.list_google_accounts()
                .unwrap()
                .iter()
                .map(|x| i64::from(x.id))
                .collect::<Vec<_>>(),
            vec![b]
        );
        // 連絡先は残る。
        assert!(s.get_contact(only_a).unwrap().links.is_empty());
        assert_eq!(dirty_of(&s, only_a), (0, vec![]));
        let links = s.get_contact(both).unwrap().links;
        assert_eq!(links.len(), 1);
        assert_eq!(i64::from(links[0].account_id), b);
        assert_eq!(dirty_of(&s, both), (0, vec![0]));
        let groups: i64 = s
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM contact_group_identities", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(groups, 0);
    }
}
