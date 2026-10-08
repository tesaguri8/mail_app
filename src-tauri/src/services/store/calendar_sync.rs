use super::Store;
use rusqlite::{params, Connection, OptionalExtension};

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

/// 繰り返しの本体 `master_id` を消したとき、その 1 回だけ変更された回も論理削除する。
///
/// Google は本体の削除で例外インスタンスも消すので、ここで送信対象（dirty）にはしない。
/// 本体でない予定（例外を持たない）なら何もしない。
pub(super) fn cascade_delete_instances(conn: &Connection, master_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE events SET deleted_at = (SELECT deleted_at FROM events WHERE id = ?1), dirty = 0 \
         WHERE deleted_at IS NULL AND recurring_external_id IS NOT NULL \
           AND (calendar_id, recurring_external_id) = \
               (SELECT calendar_id, external_id FROM events \
                WHERE id = ?1 AND recurrence IS NOT NULL AND external_id IS NOT NULL)",
        params![master_id],
    )?;
    Ok(())
}

/// 同期エンジン（services/google/calendar）が Store へ渡す「Google 側の予定」1件。
/// 日時などは既にローカル表現（'YYYY-MM-DD' / 'YYYY-MM-DDTHH:MM'）へ変換済み。
/// Store 層を同期エンジンに依存させないため、境界の受け渡し型はここ（store 側）に置く。
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
    /// 繰り返しの例外インスタンス（1 回だけ変更・削除された回）なら、本体との対応。
    pub instance: Option<RecurringInstance>,
}

/// 例外インスタンスが「どの本体の・どの回か」。
#[derive(Debug, Clone, Default)]
pub struct RecurringInstance {
    /// 本体（繰り返し元）の Google 予定 ID。
    pub recurring_external_id: String,
    /// 本体の展開上の元の開始（ローカル表現。終日='YYYY-MM-DD' / 時間指定='YYYY-MM-DDTHH:MM'）。
    pub original_start_at: String,
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

/// 同期対象の Google カレンダー 1 件（同期エンジンが列挙に使う）。
#[derive(Debug, Clone)]
pub struct SyncedCalendar {
    /// ローカル calendars.id。
    pub local_id: i64,
    /// Google 側のカレンダー ID。
    pub external_id: String,
    /// 増分同期トークン（未取得なら None＝フル同期）。
    pub sync_token: Option<String>,
    /// owner | writer | reader | freeBusyReader。
    pub access_role: String,
}

/// apply_remote_event の結果（同期サマリの集計用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// 新規追加または更新した。
    Upserted,
    /// Google 側の削除を取り込んだ（ローカルを論理削除）。
    Deleted,
    /// 変化なし（既に削除済み、または手元と同じ版＝etag が同じ）。何も書き換えていない。
    Skipped,
}

/// 取り込み済みの Google の予定（同じ版かどうかを見るための最小限）。
struct StoredRemote {
    id: i64,
    deleted_at: Option<String>,
    etag: Option<String>,
    calendar_id: Option<i64>,
}

impl Store {
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

    /// 同期対象（sync_enabled かつ Google 連携済み）のカレンダーを返す。
    pub fn list_synced_google_calendars(
        &self,
        account_id: i64,
    ) -> rusqlite::Result<Vec<SyncedCalendar>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, external_id, sync_token, COALESCE(access_role, 'reader') \
             FROM calendars \
             WHERE account_id = ?1 AND source = 'google' AND sync_enabled = 1 \
               AND external_id IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![account_id], |r| {
            Ok(SyncedCalendar {
                local_id: r.get(0)?,
                external_id: r.get(1)?,
                sync_token: r.get(2)?,
                access_role: r.get(3)?,
            })
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
        let stored: Option<StoredRemote> = conn
            .query_row(
                "SELECT id, deleted_at, etag, calendar_id FROM events \
                 WHERE source = 'google' AND external_id = ?1",
                params![ev.external_id],
                |r| {
                    Ok(StoredRemote {
                        id: r.get(0)?,
                        deleted_at: r.get(1)?,
                        etag: r.get(2)?,
                        calendar_id: r.get(3)?,
                    })
                },
            )
            .optional()?;
        // 手元と同じ版（etag が同じ・同じカレンダー・削除されていない）なら何もしない。
        // 同期トークンを受け付けないカレンダー（Google の祝日カレンダーは毎回 410 を返す）は毎回
        // フル取得になるので、ここで落とさないと変化の無い予定まで「取り込み」に数え、書き直して
        // しまう（`[実測]` 2026-10-09: 日本の祝日 169 件が同期のたびに取り込みとして出ていた）。
        // 手元の未送信の変更（dirty）も、変わっていない Google の版で上書きしない。
        if !ev.cancelled
            && stored.as_ref().is_some_and(|st| {
                st.deleted_at.is_none()
                    && st.etag.is_some()
                    && st.etag == ev.etag
                    && st.calendar_id == Some(calendar_local_id)
            })
        {
            return Ok(ApplyOutcome::Skipped);
        }
        let existing = stored.map(|st| (st.id, st.deleted_at));

        if ev.cancelled {
            // 1 回だけの削除は、本体の展開からその回を除くために記録する（予定行が無くても）。
            let recorded = match &ev.instance {
                Some(inst) => {
                    conn.execute(
                        "INSERT INTO event_cancelled_instances \
                            (calendar_id, external_id, recurring_external_id, original_start_at) \
                         VALUES (?1, ?2, ?3, ?4) \
                         ON CONFLICT(calendar_id, external_id) DO UPDATE SET \
                            recurring_external_id = ?3, original_start_at = ?4",
                        params![
                            calendar_local_id,
                            ev.external_id,
                            inst.recurring_external_id,
                            inst.original_start_at,
                        ],
                    )?;
                    true
                }
                None => false,
            };
            return match existing {
                Some((id, None)) => {
                    conn.execute(
                        "UPDATE events SET deleted_at = CURRENT_TIMESTAMP, dirty = 0 WHERE id = ?1",
                        params![id],
                    )?;
                    // 本体の削除なら、1 回だけ変更された回も一緒に消す（孤立させない）。
                    cascade_delete_instances(&conn, id)?;
                    Ok(ApplyOutcome::Deleted)
                }
                _ if recorded => Ok(ApplyOutcome::Deleted),
                _ => Ok(ApplyOutcome::Skipped),
            };
        }

        // 削除されていた回が復活した（例外として戻った）なら、削除の記録を外す。
        if ev.instance.is_some() {
            conn.execute(
                "DELETE FROM event_cancelled_instances WHERE calendar_id = ?1 AND external_id = ?2",
                params![calendar_local_id, ev.external_id],
            )?;
        }
        let (recurring_external_id, original_start_at) = ev
            .instance
            .as_ref()
            .map(|i| {
                (
                    Some(i.recurring_external_id.as_str()),
                    Some(i.original_start_at.as_str()),
                )
            })
            .unwrap_or((None, None));

        let event_id = match existing {
            Some((id, _)) => {
                conn.execute(
                    "UPDATE events SET \
                        title = ?1, description = ?2, location = ?3, start_at = ?4, end_at = ?5, \
                        all_day = ?6, recurrence = ?7, reminder_minutes = ?8, color = ?9, \
                        availability = ?10, visibility = ?11, calendar_id = ?12, \
                        remote_calendar = (SELECT external_id FROM calendars WHERE id = ?12), \
                        etag = ?13, recurring_external_id = ?15, original_start_at = ?16, \
                        dirty = 0, deleted_at = NULL, updated_at = CURRENT_TIMESTAMP \
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
                        recurring_external_id,
                        original_start_at,
                    ],
                )?;
                id
            }
            None => {
                conn.execute(
                    "INSERT INTO events \
                        (title, description, location, start_at, end_at, all_day, recurrence, \
                         reminder_minutes, color, availability, visibility, calendar_id, \
                         remote_calendar, source, external_id, etag, recurring_external_id, \
                         original_start_at, dirty) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, \
                             (SELECT external_id FROM calendars WHERE id = ?12), \
                             'google', ?13, ?14, ?15, ?16, 0)",
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
                        recurring_external_id,
                        original_start_at,
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
    use crate::models::EventSummary;

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
    fn the_same_version_is_not_applied_again() {
        let s = mem_store();
        let acct = s.upsert_google_account("a@gmail.com", None, None).unwrap();
        let cal = s
            .upsert_google_calendar(acct, "holiday", "日本の祝日", None, "reader", false)
            .unwrap();
        let v1 = RemoteEvent {
            etag: Some("\"1\"".into()),
            ..remote("h1", "元日", "2026-01-01")
        };
        assert!(matches!(
            s.apply_remote_event(cal, &v1).unwrap(),
            ApplyOutcome::Upserted
        ));
        // フル取得で同じ版がもう一度来ても、取り込みに数えない。
        assert!(matches!(
            s.apply_remote_event(cal, &v1).unwrap(),
            ApplyOutcome::Skipped
        ));
        // 版が変われば取り込む。
        let v2 = RemoteEvent {
            etag: Some("\"2\"".into()),
            ..remote("h1", "元日（祝）", "2026-01-01")
        };
        assert!(matches!(
            s.apply_remote_event(cal, &v2).unwrap(),
            ApplyOutcome::Upserted
        ));
        let titles: Vec<String> = s
            .list_events("2026-01-01", "2026-01-02", false)
            .unwrap()
            .into_iter()
            .map(|e| e.title)
            .collect();
        assert_eq!(titles, vec!["元日（祝）".to_string()]);
    }

    #[test]
    fn an_unchanged_remote_version_does_not_overwrite_an_unsent_local_edit() {
        let s = mem_store();
        let acct = s.upsert_google_account("a@gmail.com", None, None).unwrap();
        let cal = s
            .upsert_google_calendar(acct, "cal", "予定表", None, "owner", true)
            .unwrap();
        let v1 = RemoteEvent {
            etag: Some("\"1\"".into()),
            ..remote("e1", "会議", "2026-07-06T10:00")
        };
        s.apply_remote_event(cal, &v1).unwrap();
        let ev = &s.list_events("2026-07-01", "2026-08-01", false).unwrap()[0];
        // 手元で編集（まだ送っていない）。
        s.upsert_event(&crate::models::EventInput {
            id: Some(ev.id),
            title: "会議（変更）".into(),
            start_at: "2026-07-06T10:00".into(),
            calendar_id: Some(cal as i32),
            ..Default::default()
        })
        .unwrap();
        // Google 側は変わっていない版が来る。
        assert!(matches!(
            s.apply_remote_event(cal, &v1).unwrap(),
            ApplyOutcome::Skipped
        ));
        assert_eq!(
            s.list_local_changes(cal).unwrap().len(),
            1,
            "未送信の変更は残る"
        );
        assert_eq!(
            s.list_events("2026-07-01", "2026-08-01", false).unwrap()[0].title,
            "会議（変更）"
        );
    }

    /// 例外インスタンス（本体 `master` の、元の開始 `original` の回）。
    fn instance(id: &str, master: &str, original: &str, start: &str) -> RemoteEvent {
        RemoteEvent {
            instance: Some(RecurringInstance {
                recurring_external_id: master.into(),
                original_start_at: original.into(),
            }),
            ..remote(id, "定例MTG", start)
        }
    }

    /// Google カレンダー 1 つと、週次の本体（金曜 10:30）を用意する。
    fn weekly_master(s: &Store) -> i64 {
        let acct = s.upsert_google_account("a@gmail.com", None, None).unwrap();
        let cal = s
            .upsert_google_calendar(acct, "cal_ext_1", "予定表", None, "owner", true)
            .unwrap();
        let master = RemoteEvent {
            recurrence: Some("FREQ=WEEKLY;WKST=MO".into()),
            ..remote("m1", "定例MTG", "2026-09-04T10:30")
        };
        s.apply_remote_event(cal, &master).unwrap();
        cal
    }

    fn find<'a>(list: &'a [EventSummary], start: &str) -> Option<&'a EventSummary> {
        list.iter().find(|e| e.start_at == start)
    }

    #[test]
    fn modified_instance_is_listed_and_excluded_from_master() {
        let s = mem_store();
        let cal = weekly_master(&s);
        // 10/9（金）10:30 の回を 10/8（木）15:00 へ動かした。
        let out = s
            .apply_remote_event(
                cal,
                &instance("m1_1009", "m1", "2026-10-09T10:30", "2026-10-08T15:00"),
            )
            .unwrap();
        assert_eq!(out, ApplyOutcome::Upserted);

        let list = s.list_events("2026-10-05", "2026-10-12", false).unwrap();
        let moved = find(&list, "2026-10-08T15:00").expect("動かした回が出る");
        assert!(moved.recurrence.is_none());
        assert_eq!(moved.original_start_at.as_deref(), Some("2026-10-09T10:30"));
        let master = find(&list, "2026-09-04T10:30").expect("本体は展開元として出る");
        assert_eq!(master.exdates, vec!["2026-10-09T10:30".to_string()]);
    }

    #[test]
    fn cancelled_instance_is_recorded_without_event_row() {
        let s = mem_store();
        let cal = weekly_master(&s);
        let mut cancel = instance("m1_1016", "m1", "2026-10-16T10:30", "");
        cancel.cancelled = true;
        assert_eq!(
            s.apply_remote_event(cal, &cancel).unwrap(),
            ApplyOutcome::Deleted
        );

        let list = s.list_events("2026-10-12", "2026-10-19", false).unwrap();
        assert_eq!(list.len(), 1, "削除された回は予定行として出ない");
        assert_eq!(list[0].exdates, vec!["2026-10-16T10:30".to_string()]);
        assert!(
            s.list_trashed_events().unwrap().is_empty(),
            "ゴミ箱にも入らない"
        );

        // 同じ回が例外として戻ったら、削除の記録は外れる（変更後の回として除かれる）。
        s.apply_remote_event(
            cal,
            &instance("m1_1016", "m1", "2026-10-16T10:30", "2026-10-16T13:00"),
        )
        .unwrap();
        let list = s.list_events("2026-10-12", "2026-10-19", false).unwrap();
        assert_eq!(list.len(), 2);
        let master = find(&list, "2026-09-04T10:30").unwrap();
        assert_eq!(master.exdates, vec!["2026-10-16T10:30".to_string()]);
    }

    #[test]
    fn deleting_master_takes_instances_along() {
        let s = mem_store();
        let cal = weekly_master(&s);
        s.apply_remote_event(
            cal,
            &instance("m1_1009", "m1", "2026-10-09T10:30", "2026-10-08T15:00"),
        )
        .unwrap();
        let master_id = s
            .list_events("2026-10-05", "2026-10-12", false)
            .unwrap()
            .iter()
            .find(|e| e.recurrence.is_some())
            .map(|e| e.id as i64)
            .unwrap();

        // ローカルで本体を消すと、変更された回も一緒にゴミ箱へ（送信対象は本体だけ）。
        s.delete_event(master_id).unwrap();
        assert!(s
            .list_events("2026-10-05", "2026-10-12", false)
            .unwrap()
            .is_empty());
        let changes = s.list_local_changes(cal).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].id, master_id);

        // 本体を戻すと、一緒に消えた回も戻る。
        s.restore_event(master_id).unwrap();
        assert_eq!(
            s.list_events("2026-10-05", "2026-10-12", false)
                .unwrap()
                .len(),
            2
        );

        // Google 側で本体が消えても、変更された回は孤立せず消える。
        let mut cancel = remote("m1", "定例MTG", "");
        cancel.cancelled = true;
        s.apply_remote_event(cal, &cancel).unwrap();
        assert!(s
            .list_events("2026-10-05", "2026-10-12", false)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn apply_remote_insert_update_delete() {
        let s = mem_store();
        let acct = s.upsert_google_account("a@gmail.com", None, None).unwrap();
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
        let acct = s.upsert_google_account("a@gmail.com", None, None).unwrap();
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
    fn disconnect_keeps_calendars_and_unsent_changes_until_purged() {
        let s = mem_store();
        let acct = s.upsert_google_account("a@gmail.com", None, None).unwrap();
        let cal = s
            .upsert_google_calendar(acct, "cal_ext_1", "予定表", None, "owner", true)
            .unwrap();
        s.apply_remote_event(cal, &remote("ev1", "会議", "2026-07-06T10:00")).unwrap();
        // 未送信のローカル変更。
        s.upsert_event(&crate::models::EventInput {
            title: "未送信".into(),
            start_at: "2026-07-07T10:00".into(),
            calendar_id: Some(cal as i32),
            ..Default::default()
        })
        .unwrap();

        // 解除中: カレンダー・予定・未送信の変更は残り、解除中と分かる。
        s.disconnect_google_account(acct).unwrap();
        assert!(s.google_account_disconnected(acct).unwrap());
        assert_eq!(s.list_events("2026-07-01", "2026-08-01", false).unwrap().len(), 2);
        assert_eq!(s.list_local_changes(cal).unwrap().len(), 1);
        let cals = s.list_calendars().unwrap();
        assert!(cals.iter().any(|c| c.account_disconnected));

        // 完全に解除: Google のカレンダーと予定の写しが消える。
        s.purge_google_account(acct).unwrap();
        assert!(s.list_google_accounts().unwrap().is_empty());
        // 既定（ローカル）カレンダーは残る
        assert_eq!(s.list_calendars().unwrap().len(), 1);
        assert_eq!(s.list_events("2026-07-01", "2026-08-01", false).unwrap().len(), 0);
    }
}
