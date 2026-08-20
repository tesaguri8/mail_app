//! Google 連携アカウント（カレンダー・連絡先で共有）。
//!
//! アカウント 1 件 = refresh_token 1 本。カレンダーと連絡先で同じ Google アカウントを
//! 使い回し、サービスごとに有効フラグと最終同期時刻だけを分けて持つ（マイグレーション 0053）。
//! refresh_token 自体は keyring 側にあり、この層はメタデータのみを扱う。

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
    })
}

/// 一覧・単票で共通に使う選択列（row_to_account の並びと対応）。
const ACCOUNT_COLUMNS: &str =
    "id, email, sync_calendar, sync_contacts, last_calendar_sync_at, last_contacts_sync_at";

impl Store {
    /// Google アカウントを登録（既存なら external_id と許可スコープを更新）し、行 id を返す。
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
                 granted_scopes = COALESCE(?3, granted_scopes)",
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

    /// 指定サービスの同期が有効なアカウントだけを返す（自動同期の対象選別用）。
    pub fn list_google_accounts_for(
        &self,
        service: GoogleService,
    ) -> rusqlite::Result<Vec<GoogleAccount>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {ACCOUNT_COLUMNS} FROM google_accounts \
             WHERE provider = 'google' AND {} = 1 ORDER BY id",
            service.enabled_column()
        ))?;
        let rows = stmt.query_map([], row_to_account)?;
        rows.collect()
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

    /// 許可済みスコープ（スペース区切り）。未記録（0053 以前の連携）なら None。
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

    /// アカウントの連携を解除する。所属する Google カレンダーとその予定を削除する
    /// （ローカル専用カレンダー・予定には触れない）。
    ///
    /// 連絡先は削除しない。カレンダーは Google 側が正本で解除すれば残す意味がないが、
    /// 連絡先はローカル発のレコードと混在するため、解除時は連携情報を外すだけにする
    /// （その処理は連絡先同期の実装時にここへ足す）。
    pub fn delete_google_account(&self, account_id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        // このアカウントの Google カレンダーに属する予定を物理削除。
        conn.execute(
            "DELETE FROM events WHERE calendar_id IN \
                (SELECT id FROM calendars WHERE account_id = ?1)",
            params![account_id],
        )?;
        conn.execute(
            "DELETE FROM calendars WHERE account_id = ?1",
            params![account_id],
        )?;
        conn.execute(
            "DELETE FROM google_accounts WHERE id = ?1",
            params![account_id],
        )?;
        Ok(())
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
        // 既存アカウントはカレンダー有効・連絡先無効が既定（マイグレーション 0053）。
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
}
