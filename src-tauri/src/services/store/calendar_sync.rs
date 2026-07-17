use super::Store;
use crate::models::GoogleAccount;
use rusqlite::{params, Connection, OptionalExtension, Row};

/// 予定のリマインダーを与えられた集合へ全置き換えする（同期の取り込み用。dirty は触らない）。
fn replace_event_reminders(
    conn: &Connection,
    event_id: i64,
    minutes: &[i32],
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM event_reminders WHERE event_id = ?1",
        params![event_id],
    )?;
    for m in minutes {
        conn.execute(
            "INSERT OR IGNORE INTO event_reminders (event_id, minutes) VALUES (?1, ?2)",
            params![event_id, m],
        )?;
    }
    Ok(())
}

/// 同期エンジン（services/gcal）が Store へ渡す「Google 側の予定」1件。
/// 日時などは既にローカル表現（'YYYY-MM-DD' / 'YYYY-MM-DDTHH:MM'）へ変換済み。
/// Store 層を gcal に依存させないため、境界の受け渡し型はここ（store 側）に置く。
#[derive(Debug, Clone, Default)]
pub struct RemoteEvent {
    pub external_id: String,
    pub etag: Option<String>,
    /// status == "cancelled"（Google 側で削除された）。
    pub cancelled: bool,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_at: String,
    pub end_at: Option<String>,
    pub all_day: bool,
    pub recurrence: Option<String>,
    /// 互換の代表値（最も早い通知＝最小の分。無ければ None）。列 events.reminder_minutes 用。
    pub reminder_minutes: Option<i32>,
    /// 全リマインダー（開始何分前）。event_reminders テーブルへ反映する。
    pub reminders: Vec<i32>,
    pub availability: String,
    pub visibility: String,
    pub color: Option<String>,
}

/// push 対象のローカル変更（dirty=1 の予定）。Google へ送る素材。
#[derive(Debug, Clone)]
pub struct LocalChange {
    pub id: i64,
    /// 連携済みなら Google の event id。None なら未連携（＝新規作成）。
    pub external_id: Option<String>,
    /// 論理削除済み（deleted_at != NULL）。true なら Google 側も削除する。
    pub deleted: bool,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_at: String,
    pub end_at: Option<String>,
    pub all_day: bool,
    pub recurrence: Option<String>,
    pub reminder_minutes: Option<i32>,
    /// 全リマインダー（開始何分前）。Google へ全通知を送るのに使う。
    pub reminders: Vec<i32>,
    pub availability: String,
    pub visibility: String,
}

/// apply_remote_event の結果（同期サマリの集計用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// 新規追加または更新した。
    Upserted,
    /// Google 側の削除を取り込んだ（ローカルを論理削除）。
    Deleted,
    /// 変化なし（既に削除済み等）。
    Skipped,
}

fn row_to_account(r: &Row) -> rusqlite::Result<GoogleAccount> {
    Ok(GoogleAccount {
        id: r.get::<_, i64>(0)? as i32,
        email: r.get(1)?,
        last_sync_at: r.get(2)?,
    })
}

impl Store {
    // ── 連携アカウント ──────────────────────────────────────────────

    /// Google アカウントを登録（既存なら external_id を更新）し、行 id を返す。
    pub fn upsert_calendar_account(
        &self,
        email: &str,
        external_id: Option<&str>,
    ) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO calendar_accounts (provider, email, external_id) VALUES ('google', ?1, ?2) \
             ON CONFLICT(provider, email) DO UPDATE SET external_id = COALESCE(?2, external_id)",
            params![email, external_id],
        )?;
        conn.query_row(
            "SELECT id FROM calendar_accounts WHERE provider = 'google' AND email = ?1",
            params![email],
            |r| r.get(0),
        )
    }

    /// 連携済み Google アカウント一覧。
    pub fn list_calendar_accounts(&self) -> rusqlite::Result<Vec<GoogleAccount>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, email, last_sync_at FROM calendar_accounts \
             WHERE provider = 'google' ORDER BY id",
        )?;
        let rows = stmt.query_map([], row_to_account)?;
        rows.collect()
    }

    /// アカウントのメールアドレス（keyring キー）を引く。
    pub fn calendar_account_email(&self, account_id: i64) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT email FROM calendar_accounts WHERE id = ?1",
            params![account_id],
            |r| r.get(0),
        )
        .optional()
    }

    /// 最終同期時刻を現在時刻に更新。
    pub fn touch_calendar_account_synced(&self, account_id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE calendar_accounts SET last_sync_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![account_id],
        )?;
        Ok(())
    }

    /// アカウントの連携を解除する。所属する Google カレンダーとその予定を削除する
    /// （ローカル専用カレンダー・予定には触れない）。
    pub fn delete_calendar_account(&self, account_id: i64) -> rusqlite::Result<()> {
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
            "DELETE FROM calendar_accounts WHERE id = ?1",
            params![account_id],
        )?;
        Ok(())
    }

    // ── Google カレンダー（calendars 行の同期メタ） ───────────────────

    /// Google カレンダーをローカル calendars に upsert（external_id で突き合わせ）し、行 id を返す。
    /// 表示名/色/権限は Google 側の最新で更新する。既定カレンダーには触れない。
    pub fn upsert_google_calendar(
        &self,
        account_id: i64,
        external_id: &str,
        name: &str,
        color: Option<&str>,
        access_role: &str,
        primary: bool,
    ) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM calendars WHERE account_id = ?1 AND external_id = ?2",
                params![account_id, external_id],
                |r| r.get(0),
            )
            .optional()?;
        let kind = if primary { "mine" } else { "other" };
        match existing {
            Some(id) => {
                conn.execute(
                    "UPDATE calendars SET name = ?1, color = COALESCE(?2, color), \
                        access_role = ?3, kind = ?4 WHERE id = ?5",
                    params![name, color, access_role, kind, id],
                )?;
                Ok(id)
            }
            None => {
                let next: i64 = conn.query_row(
                    "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM calendars",
                    [],
                    |r| r.get(0),
                )?;
                conn.execute(
                    "INSERT INTO calendars \
                        (name, color, kind, visible, is_default, source, external_id, \
                         account_id, access_role, sync_enabled, sort_order) \
                     VALUES (?1, ?2, ?3, 1, 0, 'google', ?4, ?5, ?6, 1, ?7)",
                    params![name, color, kind, external_id, account_id, access_role, next],
                )?;
                Ok(conn.last_insert_rowid())
            }
        }
    }

    /// 同期対象の Google カレンダー（local_id, external_id, sync_token, access_role）を返す。
    pub fn list_synced_google_calendars(
        &self,
        account_id: i64,
    ) -> rusqlite::Result<Vec<(i64, String, Option<String>, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, external_id, sync_token, COALESCE(access_role, 'reader') \
             FROM calendars \
             WHERE account_id = ?1 AND source = 'google' AND sync_enabled = 1 \
               AND external_id IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![account_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        rows.collect()
    }

    /// カレンダー（ローカル id）が Google 連携カレンダーなら (account_id, external_id, access_role)。
    /// ローカル専用カレンダーや未連携なら None。保存時の自動送信の判定に使う。
    pub fn google_calendar_meta(
        &self,
        calendar_local_id: i64,
    ) -> rusqlite::Result<Option<(i64, String, String)>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT account_id, external_id, COALESCE(access_role, 'reader') \
             FROM calendars \
             WHERE id = ?1 AND source = 'google' \
               AND account_id IS NOT NULL AND external_id IS NOT NULL",
            params![calendar_local_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
    }

    /// カレンダーの増分同期トークン（nextSyncToken）を保存。
    pub fn set_calendar_sync_token(
        &self,
        calendar_id: i64,
        token: Option<&str>,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE calendars SET sync_token = ?1 WHERE id = ?2",
            params![token, calendar_id],
        )?;
        Ok(())
    }

    // ── 予定の取り込み / 送信 ────────────────────────────────────────

    /// Google 側の 1 予定をローカルへ反映する（external_id で突き合わせ）。
    pub fn apply_remote_event(
        &self,
        calendar_local_id: i64,
        ev: &RemoteEvent,
    ) -> rusqlite::Result<ApplyOutcome> {
        let conn = self.conn.lock().unwrap();
        let existing: Option<(i64, Option<String>)> = conn
            .query_row(
                "SELECT id, deleted_at FROM events WHERE source = 'google' AND external_id = ?1",
                params![ev.external_id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .optional()?;

        if ev.cancelled {
            return match existing {
                Some((id, None)) => {
                    conn.execute(
                        "UPDATE events SET deleted_at = CURRENT_TIMESTAMP, dirty = 0 WHERE id = ?1",
                        params![id],
                    )?;
                    Ok(ApplyOutcome::Deleted)
                }
                _ => Ok(ApplyOutcome::Skipped),
            };
        }

        let event_id = match existing {
            Some((id, _)) => {
                conn.execute(
                    "UPDATE events SET \
                        title = ?1, description = ?2, location = ?3, start_at = ?4, end_at = ?5, \
                        all_day = ?6, recurrence = ?7, reminder_minutes = ?8, color = ?9, \
                        availability = ?10, visibility = ?11, calendar_id = ?12, \
                        remote_calendar = (SELECT external_id FROM calendars WHERE id = ?12), \
                        etag = ?13, dirty = 0, deleted_at = NULL, updated_at = CURRENT_TIMESTAMP \
                     WHERE id = ?14",
                    params![
                        ev.title,
                        ev.description,
                        ev.location,
                        ev.start_at,
                        ev.end_at,
                        ev.all_day as i64,
                        ev.recurrence,
                        ev.reminder_minutes,
                        ev.color,
                        ev.availability,
                        ev.visibility,
                        calendar_local_id,
                        ev.etag,
                        id,
                    ],
                )?;
                id
            }
            None => {
                conn.execute(
                    "INSERT INTO events \
                        (title, description, location, start_at, end_at, all_day, recurrence, \
                         reminder_minutes, color, availability, visibility, calendar_id, \
                         remote_calendar, source, external_id, etag, dirty) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, \
                             (SELECT external_id FROM calendars WHERE id = ?12), \
                             'google', ?13, ?14, 0)",
                    params![
                        ev.title,
                        ev.description,
                        ev.location,
                        ev.start_at,
                        ev.end_at,
                        ev.all_day as i64,
                        ev.recurrence,
                        ev.reminder_minutes,
                        ev.color,
                        ev.availability,
                        ev.visibility,
                        calendar_local_id,
                        ev.external_id,
                        ev.etag,
                    ],
                )?;
                conn.last_insert_rowid()
            }
        };
        // Google 側の全リマインダーを反映する（全置き換え）。
        replace_event_reminders(&conn, event_id, &ev.reminders)?;
        Ok(ApplyOutcome::Upserted)
    }

    /// 指定カレンダーの未送信ローカル変更（dirty=1）を返す。
    pub fn list_local_changes(&self, calendar_local_id: i64) -> rusqlite::Result<Vec<LocalChange>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, external_id, deleted_at, title, description, location, start_at, end_at, \
                    all_day, recurrence, reminder_minutes, availability, visibility \
             FROM events WHERE calendar_id = ?1 AND dirty = 1",
        )?;
        let mut changes: Vec<LocalChange> = stmt
            .query_map(params![calendar_local_id], |r| {
                Ok(LocalChange {
                    id: r.get::<_, i64>(0)?,
                    external_id: r.get::<_, Option<String>>(1)?,
                    deleted: r.get::<_, Option<String>>(2)?.is_some(),
                    title: r.get(3)?,
                    description: r.get(4)?,
                    location: r.get(5)?,
                    start_at: r.get(6)?,
                    end_at: r.get(7)?,
                    all_day: r.get::<_, i64>(8)? != 0,
                    recurrence: r.get(9)?,
                    reminder_minutes: r.get::<_, Option<i64>>(10)?.map(|v| v as i32),
                    availability: r.get::<_, Option<String>>(11)?.unwrap_or_else(|| "busy".into()),
                    visibility: r.get::<_, Option<String>>(12)?.unwrap_or_else(|| "default".into()),
                    reminders: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // 各予定の全リマインダーを詰める（Google へ全通知を送るため）。
        let mut rstmt =
            conn.prepare("SELECT minutes FROM event_reminders WHERE event_id = ?1 ORDER BY minutes")?;
        for ch in &mut changes {
            ch.reminders = rstmt
                .query_map(params![ch.id], |r| r.get::<_, i64>(0).map(|v| v as i32))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
        }
        Ok(changes)
    }

    /// 送信成功した予定を連携済みにする（external_id/etag を保存し dirty を落とす）。
    pub fn mark_event_pushed(
        &self,
        id: i64,
        external_id: &str,
        etag: Option<&str>,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        // remote_calendar は、この予定が今属しているカレンダーの external_id（＝送信先）。
        conn.execute(
            "UPDATE events SET external_id = ?1, etag = ?2, source = 'google', dirty = 0, \
                remote_calendar = (SELECT external_id FROM calendars \
                    WHERE id = (SELECT calendar_id FROM events WHERE id = ?3)) \
             WHERE id = ?3",
            params![external_id, etag, id],
        )?;
        Ok(())
    }

    /// 予定の (external_id, remote_calendar) を返す（更新前に控えてカレンダー移動を検出する用）。
    /// remote_calendar は「今 Google 上でこの予定が存在するカレンダー」の external_id。
    pub fn event_sync_ref(
        &self,
        id: i64,
    ) -> rusqlite::Result<Option<(Option<String>, Option<String>)>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT external_id, remote_calendar FROM events WHERE id = ?1",
            params![id],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .optional()
    }

    /// Google カレンダー（external_id）を所有する (account_id, access_role) を返す。
    /// 予定の実在カレンダーから削除する際、そのカレンダーのアカウント・権限を引くのに使う。
    pub fn google_calendar_by_ext(
        &self,
        external_id: &str,
    ) -> rusqlite::Result<Option<(i64, String)>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT account_id, COALESCE(access_role, 'reader') FROM calendars \
             WHERE source = 'google' AND external_id = ?1 AND account_id IS NOT NULL",
            params![external_id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()
    }

    /// 予定の Google 連携情報を解除する（別カレンダーへ移動したとき、新カレンダーで
    /// 新規作成扱いにするため）。dirty はそのまま（＝送信対象として残す）。
    pub fn reset_event_remote(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE events SET external_id = NULL, etag = NULL, remote_calendar = NULL, \
                source = 'local' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// 送信（削除・更新の反映）が済んだので dirty を落とす（削除済みはゴミ箱に残す）。
    pub fn clear_event_dirty(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE events SET dirty = 0 WHERE id = ?1", params![id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_store() -> Store {
        Store::open_in_memory_for_test()
    }

    fn remote(id: &str, title: &str, start: &str) -> RemoteEvent {
        RemoteEvent {
            external_id: id.into(),
            title: title.into(),
            start_at: start.into(),
            all_day: false,
            availability: "busy".into(),
            visibility: "default".into(),
            ..Default::default()
        }
    }

    #[test]
    fn account_upsert_and_list() {
        let s = mem_store();
        let id = s.upsert_calendar_account("a@gmail.com", Some("sub123")).unwrap();
        assert!(id > 0);
        // 同じメールなら同じ行（external_id を更新）
        let id2 = s.upsert_calendar_account("a@gmail.com", Some("sub456")).unwrap();
        assert_eq!(id, id2);
        let accts = s.list_calendar_accounts().unwrap();
        assert_eq!(accts.len(), 1);
        assert_eq!(accts[0].email, "a@gmail.com");
    }

    #[test]
    fn apply_remote_insert_update_delete() {
        let s = mem_store();
        let acct = s.upsert_calendar_account("a@gmail.com", None).unwrap();
        let cal = s
            .upsert_google_calendar(acct, "cal_ext_1", "予定表", Some("#64b5f6"), "owner", true)
            .unwrap();

        // 追加
        let out = s.apply_remote_event(cal, &remote("ev1", "会議", "2026-07-06T10:00")).unwrap();
        assert_eq!(out, ApplyOutcome::Upserted);
        let list = s.list_events("2026-07-01", "2026-08-01", false).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "会議");

        // 更新（同じ external_id）
        let out = s.apply_remote_event(cal, &remote("ev1", "会議（更新）", "2026-07-06T11:00")).unwrap();
        assert_eq!(out, ApplyOutcome::Upserted);
        let list = s.list_events("2026-07-01", "2026-08-01", false).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "会議（更新）");

        // 削除（cancelled）
        let mut cancel = remote("ev1", "会議（更新）", "2026-07-06T11:00");
        cancel.cancelled = true;
        let out = s.apply_remote_event(cal, &cancel).unwrap();
        assert_eq!(out, ApplyOutcome::Deleted);
        assert_eq!(s.list_events("2026-07-01", "2026-08-01", false).unwrap().len(), 0);
    }

    #[test]
    fn local_changes_are_tracked_and_cleared() {
        let s = mem_store();
        let acct = s.upsert_calendar_account("a@gmail.com", None).unwrap();
        let cal = s
            .upsert_google_calendar(acct, "cal_ext_1", "予定表", None, "owner", true)
            .unwrap();
        // ローカルで新規作成（upsert_event は dirty=1 を立てる）
        let ev = crate::models::EventInput {
            title: "新規".into(),
            start_at: "2026-07-06T10:00".into(),
            calendar_id: Some(cal as i32),
            ..Default::default()
        };
        let saved = s.upsert_event(&ev).unwrap();
        let changes = s.list_local_changes(cal).unwrap();
        assert_eq!(changes.len(), 1);
        assert!(changes[0].external_id.is_none());
        assert!(!changes[0].deleted);

        // 送信済みにすると dirty が落ちる
        s.mark_event_pushed(saved.id as i64, "gev1", Some("etag1")).unwrap();
        assert_eq!(s.list_local_changes(cal).unwrap().len(), 0);
    }

    #[test]
    fn disconnect_removes_google_calendars_and_events() {
        let s = mem_store();
        let acct = s.upsert_calendar_account("a@gmail.com", None).unwrap();
        let cal = s
            .upsert_google_calendar(acct, "cal_ext_1", "予定表", None, "owner", true)
            .unwrap();
        s.apply_remote_event(cal, &remote("ev1", "会議", "2026-07-06T10:00")).unwrap();

        s.delete_calendar_account(acct).unwrap();
        assert!(s.list_calendar_accounts().unwrap().is_empty());
        // 既定（ローカル）カレンダーは残る
        assert_eq!(s.list_calendars().unwrap().len(), 1);
        assert_eq!(s.list_events("2026-07-01", "2026-08-01", false).unwrap().len(), 0);
    }
}
