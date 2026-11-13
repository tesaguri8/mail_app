//! Google カレンダーの全件の取り直し（docs/CALENDAR_SYNC.md）。
//!
//! 増分同期（syncToken）は、予定を取りこぼしたまま「取得 0 件・次のトークンあり」を返し続けることが
//! ある（`[実測]` vaio 2026-11-10: Google の画面にある予定が、手元に一度も届かなかった）。そこで
//! カレンダーごとに、手動（「最初から同期し直す」）と 1 日 1 回の自動で全件を取り直し、手元を合わせる:
//!
//! - Google から返ってきた予定は、手元で削除扱いでも戻す（`apply_remote_event_detail`）
//! - 取り直した期間の中で、手元にあるのに返ってこなかった予定は削除扱いにする（ここ）
//!   - 未送信のローカル変更（dirty=1）は消さない
//!   - 期間より前の予定には触らない（Google は期間外を返さないので、無いことにならない）
//! - 繰り返しの 1 回だけの削除の記録（`event_cancelled_instances`）も、期間の中で返ってこなかった
//!   ものは外す（Google 側で戻された回を、手元で隠したままにしない）
//!
//! Google 側でカレンダーそのものが消えたとき（calendarList に返ってこない / 予定の取得が 404）は、
//! 手元のカレンダーとその予定を外す（`[実測]` vaio 2026-11-12: 消したカレンダーの pull が 30 秒ごとに
//! 404 で失敗し続け、一覧にも残っていた）。消し方は連携解除（`delete_google_account`）の後片付けと同じ。

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};

use super::calendar_sync::cascade_delete_instances;
use super::Store;

/// 全件の取り直しの突き合わせの結果。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FullPullReconciled {
    /// 手元にあるのに返ってこなかったので削除扱いにした予定。
    pub removed: usize,
    /// 返ってこなかったので外した「1 回だけの削除」の記録。
    pub uncancelled: usize,
}

/// Google から無くなったので外したカレンダー。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedCalendar {
    /// ローカルの calendars.id。
    pub id: i64,
    pub name: String,
    /// Google のカレンダー ID。
    pub external_id: String,
    /// 一緒に消した予定の件数（削除扱いのものも含む）。
    pub events: usize,
}

/// Google のカレンダー 1 つを、その予定ごと外す（Google のカレンダーでなければ何もしない）。
/// 1 回だけの削除の記録（`event_cancelled_instances`）とリマインダー・参加者は外部キーの
/// CASCADE で消える。
fn remove_google_calendar_in(
    conn: &Connection,
    calendar_id: i64,
) -> rusqlite::Result<Option<RemovedCalendar>> {
    let Some((name, external_id)) = conn
        .query_row(
            "SELECT name, external_id FROM calendars \
             WHERE id = ?1 AND source = 'google' AND external_id IS NOT NULL",
            params![calendar_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let events = conn.execute(
        "DELETE FROM events WHERE calendar_id = ?1",
        params![calendar_id],
    )?;
    conn.execute("DELETE FROM calendars WHERE id = ?1", params![calendar_id])?;
    Ok(Some(RemovedCalendar {
        id: calendar_id,
        name,
        external_id,
        events,
    }))
}

impl Store {
    /// Google で無くなったカレンダー（予定の取得が 404）を、その予定ごと外す。
    /// Google のカレンダーでなければ何もしない（None）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn remove_google_calendar(
        &self,
        calendar_id: i64,
    ) -> rusqlite::Result<Option<RemovedCalendar>> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let removed = remove_google_calendar_in(&tx, calendar_id)?;
        tx.commit()?;
        Ok(removed)
    }

    /// アカウントの Google カレンダーのうち、calendarList に返ってこなかったものを外す。
    ///
    /// - `present`: calendarList に返ってきた（削除の印が無い）カレンダーの Google の ID
    ///
    /// ほかのアカウントのカレンダーと、Google 以外のカレンダーには触らない。一覧が空なら
    /// 何もしない（主カレンダーは必ずあるので、空は取得の不具合とみなす）。
    ///
    /// # Errors
    /// DB の読み書きに失敗したとき。
    pub fn prune_missing_google_calendars(
        &self,
        account_id: i64,
        present: &HashSet<String>,
    ) -> rusqlite::Result<Vec<RemovedCalendar>> {
        if present.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let gone: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT id, external_id FROM calendars \
                 WHERE account_id = ?1 AND source = 'google' AND external_id IS NOT NULL",
            )?;
            let rows = stmt.query_map(params![account_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?;
            rows.filter_map(|row| match row {
                Ok((_, ext)) if present.contains(&ext) => None,
                Ok((id, _)) => Some(Ok(id)),
                Err(e) => Some(Err(e)),
            })
            .collect::<rusqlite::Result<_>>()?
        };
        let removed = gone
            .into_iter()
            .map(|id| remove_google_calendar_in(&tx, id))
            .filter_map(Result::transpose)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        tx.commit()?;
        Ok(removed)
    }

    /// 次の同期で、このカレンダーを最初から（全件で）取り直す（「最初から同期し直す」）。
    /// 同期トークンを捨て、最後に全件を取り直した時刻も消す。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn request_calendar_full_resync(&self, calendar_id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE calendars SET sync_token = NULL, last_full_sync_at = NULL WHERE id = ?1",
            params![calendar_id],
        )?;
        Ok(())
    }

    /// 全件を取り直し終えた時刻を記録する（次の自動の全件は 24 時間後）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn mark_calendar_full_synced(&self, calendar_id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE calendars SET last_full_sync_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![calendar_id],
        )?;
        Ok(())
    }

    /// 全件を取り直したあと、返ってこなかったものを手元から外す。
    ///
    /// - `seen`: 取り直しで返ってきた予定の Google の ID（cancelled も含む）
    /// - `cutoff`: これ以降に終わる予定だけを対象にする（ローカル表現の 'YYYY-MM-DD'。取り直しの
    ///   timeMin より後ろへ少し寄せて渡す＝境目の予定を誤って消さない）
    ///
    /// # Errors
    /// DB の読み書きに失敗したとき。
    pub fn reconcile_full_pull(
        &self,
        calendar_id: i64,
        seen: &HashSet<String>,
        cutoff: &str,
    ) -> rusqlite::Result<FullPullReconciled> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let missing: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT id, external_id FROM events \
                 WHERE calendar_id = ?1 AND source = 'google' AND external_id IS NOT NULL \
                   AND deleted_at IS NULL AND dirty = 0 \
                   AND COALESCE(end_at, start_at) >= ?2",
            )?;
            let rows = stmt.query_map(params![calendar_id, cutoff], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?;
            rows.filter_map(|row| match row {
                Ok((_, ext)) if seen.contains(&ext) => None,
                Ok((id, _)) => Some(Ok(id)),
                Err(e) => Some(Err(e)),
            })
            .collect::<rusqlite::Result<_>>()?
        };
        for id in &missing {
            tx.execute(
                "UPDATE events SET deleted_at = CURRENT_TIMESTAMP, dirty = 0 \
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![id],
            )?;
            // 本体が無くなったなら、1 回だけ変更された回も一緒に消す（孤立させない）。
            cascade_delete_instances(&tx, *id)?;
        }
        let stale: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT external_id FROM event_cancelled_instances \
                 WHERE calendar_id = ?1 AND original_start_at >= ?2",
            )?;
            let rows = stmt.query_map(params![calendar_id, cutoff], |r| r.get::<_, String>(0))?;
            rows.filter_map(|row| match row {
                Ok(ext) if seen.contains(&ext) => None,
                other => Some(other),
            })
            .collect::<rusqlite::Result<_>>()?
        };
        for ext in &stale {
            tx.execute(
                "DELETE FROM event_cancelled_instances WHERE calendar_id = ?1 AND external_id = ?2",
                params![calendar_id, ext],
            )?;
        }
        tx.commit()?;
        Ok(FullPullReconciled {
            removed: missing.len(),
            uncancelled: stale.len(),
        })
    }
}

#[cfg(test)]
mod tests;
