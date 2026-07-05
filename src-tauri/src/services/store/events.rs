use super::Store;
use crate::models::{EventInput, EventSummary};
use rusqlite::{params, Row};

/// events の 1 行を EventSummary に写す（列順は EVENT_COLS と対応）。
fn row_to_event(r: &Row) -> rusqlite::Result<EventSummary> {
    Ok(EventSummary {
        id: r.get::<_, i64>(0)? as i32,
        title: r.get(1)?,
        description: r.get(2)?,
        location: r.get(3)?,
        start_at: r.get(4)?,
        end_at: r.get(5)?,
        all_day: r.get::<_, i64>(6)? != 0,
        color: r.get(7)?,
        recurrence: r.get(8)?,
        reminder_minutes: r.get::<_, Option<i64>>(9)?.map(|v| v as i32),
        related_email_id: r.get::<_, Option<i64>>(10)?.map(|v| v as i32),
        deleted_at: r.get(11)?,
    })
}

const EVENT_COLS: &str = "id, title, description, location, start_at, end_at, all_day, color, \
     recurrence, reminder_minutes, related_email_id, deleted_at";

/// 任意テキストを trim し、空なら None に倒す（保存時に空文字を NULL 化して表示分岐を単純化）。
fn trimmed(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

impl Store {
    /// 期間 [from, to)（'YYYY-MM-DD' 等の ISO 文字列）に重なる予定を開始順で返す。
    /// 重なり判定は overlap: start_at < to AND coalesce(end_at, start_at) >= from。
    /// ゼロ詰め ISO の辞書順＝時刻順なので、単純な文字列比較で範囲抽出できる。
    /// `include_deleted` が false なら論理削除済みを除く（既定の一覧）。
    pub fn list_events(
        &self,
        from: &str,
        to: &str,
        include_deleted: bool,
    ) -> rusqlite::Result<Vec<EventSummary>> {
        let conn = self.conn.lock().unwrap();
        let del = if include_deleted {
            ""
        } else {
            "AND deleted_at IS NULL"
        };
        let sql = format!(
            "SELECT {EVENT_COLS} FROM events \
             WHERE start_at < ?2 AND COALESCE(end_at, start_at) >= ?1 {del} \
             ORDER BY all_day DESC, start_at, title COLLATE NOCASE"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![from, to], row_to_event)?;
        rows.collect()
    }

    /// 論理削除済みの予定のみを返す（ゴミ箱一覧。削除日時の新しい順）。
    pub fn list_trashed_events(&self) -> rusqlite::Result<Vec<EventSummary>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {EVENT_COLS} FROM events \
             WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_event)?;
        rows.collect()
    }

    /// 単一の予定を取得。
    pub fn get_event(&self, id: i64) -> rusqlite::Result<EventSummary> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {EVENT_COLS} FROM events WHERE id = ?1");
        conn.query_row(&sql, params![id], row_to_event)
    }

    /// 予定を作成または更新し、確定後の行を返す。`input.id` が None なら新規。
    pub fn upsert_event(&self, input: &EventInput) -> rusqlite::Result<EventSummary> {
        let id = {
            let conn = self.conn.lock().unwrap();
            let end_at = trimmed(&input.end_at);
            match input.id {
                Some(id) => {
                    conn.execute(
                        "UPDATE events SET \
                             title = ?1, description = ?2, location = ?3, start_at = ?4, \
                             end_at = ?5, all_day = ?6, color = ?7, recurrence = ?8, \
                             reminder_minutes = ?9, related_email_id = ?10, \
                             updated_at = CURRENT_TIMESTAMP \
                         WHERE id = ?11",
                        params![
                            input.title.trim(),
                            trimmed(&input.description),
                            trimmed(&input.location),
                            input.start_at.trim(),
                            end_at,
                            input.all_day as i64,
                            trimmed(&input.color),
                            trimmed(&input.recurrence),
                            input.reminder_minutes,
                            input.related_email_id,
                            id,
                        ],
                    )?;
                    id as i64
                }
                None => {
                    conn.execute(
                        "INSERT INTO events \
                             (title, description, location, start_at, end_at, all_day, color, \
                              recurrence, reminder_minutes, related_email_id) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            input.title.trim(),
                            trimmed(&input.description),
                            trimmed(&input.location),
                            input.start_at.trim(),
                            end_at,
                            input.all_day as i64,
                            trimmed(&input.color),
                            trimmed(&input.recurrence),
                            input.reminder_minutes,
                            input.related_email_id,
                        ],
                    )?;
                    conn.last_insert_rowid()
                }
            }
        };
        self.get_event(id)
    }

    /// 予定を論理削除（ゴミ箱へ。deleted_at を立てて一覧から隠す。保持期間後に完全削除）。
    pub fn delete_event(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE events SET deleted_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// 論理削除した予定を復元する（deleted_at をクリア）。
    pub fn restore_event(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE events SET deleted_at = NULL WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 保持期間（日数）を過ぎたゴミ箱の予定を完全削除する（参加者は CASCADE）。起動時などに呼ぶ。
    pub fn purge_expired_events(&self, retention_days: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        let cutoff = format!("-{} days", retention_days.max(0));
        conn.execute(
            "DELETE FROM events WHERE deleted_at IS NOT NULL \
             AND deleted_at <= datetime('now', ?1)",
            params![cutoff],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mem_store() -> Store {
        // in-memory DB でマイグレーションを流した Store を作る。
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        super::super::migrations::run(&conn).unwrap();
        Store {
            conn: std::sync::Mutex::new(conn),
            path: std::sync::Mutex::new(PathBuf::from(":memory:")),
        }
    }

    fn ev(title: &str, start: &str, end: Option<&str>, all_day: bool) -> EventInput {
        EventInput {
            title: title.into(),
            start_at: start.into(),
            end_at: end.map(str::to_string),
            all_day,
            ..Default::default()
        }
    }

    #[test]
    fn upsert_get_and_range() {
        let s = mem_store();
        let a = s.upsert_event(&ev("会議", "2026-07-06T10:00", None, false)).unwrap();
        assert!(a.id > 0);
        assert_eq!(a.title, "会議");
        assert!(!a.all_day);

        s.upsert_event(&ev("旅行", "2026-07-20", Some("2026-07-22"), true))
            .unwrap();
        // 7月の範囲で 2 件とれる
        let july = s.list_events("2026-07-01", "2026-08-01", false).unwrap();
        assert_eq!(july.len(), 2);
        // 6月の範囲では 0 件
        let june = s.list_events("2026-06-01", "2026-07-01", false).unwrap();
        assert_eq!(june.len(), 0);
    }

    #[test]
    fn multi_day_overlaps_middle_range() {
        let s = mem_store();
        // 7/20〜7/22 の終日予定は、7/21 単日の範囲にも重なって出る。
        s.upsert_event(&ev("合宿", "2026-07-20", Some("2026-07-22"), true))
            .unwrap();
        let mid = s.list_events("2026-07-21", "2026-07-22", false).unwrap();
        assert_eq!(mid.len(), 1);
    }

    #[test]
    fn soft_delete_hides_then_restores() {
        let s = mem_store();
        let a = s.upsert_event(&ev("歯医者", "2026-07-06T09:00", None, false)).unwrap();
        s.delete_event(a.id as i64).unwrap();
        assert_eq!(s.list_events("2026-07-01", "2026-08-01", false).unwrap().len(), 0);
        assert_eq!(s.list_trashed_events().unwrap().len(), 1);
        s.restore_event(a.id as i64).unwrap();
        assert_eq!(s.list_events("2026-07-01", "2026-08-01", false).unwrap().len(), 1);
    }

    #[test]
    fn update_changes_fields() {
        let s = mem_store();
        let mut a = s.upsert_event(&ev("仮", "2026-07-06T10:00", None, false)).unwrap();
        let input = EventInput {
            id: Some(a.id),
            title: "確定".into(),
            start_at: "2026-07-06T11:00".into(),
            location: Some("会議室A".into()),
            ..Default::default()
        };
        a = s.upsert_event(&input).unwrap();
        assert_eq!(a.title, "確定");
        assert_eq!(a.start_at, "2026-07-06T11:00");
        assert_eq!(a.location.as_deref(), Some("会議室A"));
    }
}
