use super::Store;
use crate::models::{CalendarInput, CalendarSummary};
use rusqlite::{params, OptionalExtension, Row};

fn row_to_calendar(r: &Row) -> rusqlite::Result<CalendarSummary> {
    Ok(CalendarSummary {
        id: r.get::<_, i64>(0)? as i32,
        name: r.get(1)?,
        color: r.get(2)?,
        kind: r.get(3)?,
        visible: r.get::<_, i64>(4)? != 0,
        is_default: r.get::<_, i64>(5)? != 0,
        sort_order: r.get::<_, i64>(6)? as i32,
    })
}

const CAL_COLS: &str = "id, name, color, kind, visible, is_default, sort_order";

impl Store {
    /// カレンダー一覧（既定→種別→並び順→名前）。マイを先、他を後に並べる。
    pub fn list_calendars(&self) -> rusqlite::Result<Vec<CalendarSummary>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {CAL_COLS} FROM calendars \
             ORDER BY is_default DESC, (kind = 'other'), sort_order, name COLLATE NOCASE, id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_calendar)?;
        rows.collect()
    }

    /// カレンダーを作成または更新し、確定後の行を返す。
    pub fn upsert_calendar(&self, input: &CalendarInput) -> rusqlite::Result<CalendarSummary> {
        let id = {
            let conn = self.conn.lock().unwrap();
            let kind = input.kind.as_deref().unwrap_or("mine");
            match input.id {
                Some(id) => {
                    conn.execute(
                        "UPDATE calendars SET name = ?1, color = ?2, kind = ?3 WHERE id = ?4",
                        params![input.name.trim(), input.color, kind, id],
                    )?;
                    id as i64
                }
                None => {
                    // 末尾に追加（sort_order は最大+1）。
                    let next: i64 = conn.query_row(
                        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM calendars",
                        [],
                        |r| r.get(0),
                    )?;
                    conn.execute(
                        "INSERT INTO calendars (name, color, kind, sort_order) VALUES (?1, ?2, ?3, ?4)",
                        params![input.name.trim(), input.color, kind, next],
                    )?;
                    conn.last_insert_rowid()
                }
            }
        };
        self.get_calendar(id)
    }

    /// 単一のカレンダーを取得。
    pub fn get_calendar(&self, id: i64) -> rusqlite::Result<CalendarSummary> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {CAL_COLS} FROM calendars WHERE id = ?1");
        conn.query_row(&sql, params![id], row_to_calendar)
    }

    /// 表示オン/オフを切り替える。
    pub fn set_calendar_visible(&self, id: i64, visible: bool) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE calendars SET visible = ?1 WHERE id = ?2",
            params![visible as i64, id],
        )?;
        Ok(())
    }

    /// カレンダーを削除する。既定カレンダーは削除不可（false）。
    /// 所属予定は既定カレンダーへ付け替えてから削除する（予定は消さない）。
    pub fn delete_calendar(&self, id: i64) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let is_default: bool = conn
            .query_row(
                "SELECT is_default FROM calendars WHERE id = ?1",
                params![id],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .map(|v| v != 0)
            .unwrap_or(false);
        if is_default {
            return Ok(false);
        }
        let default_id: Option<i64> = conn
            .query_row(
                "SELECT id FROM calendars WHERE is_default = 1 LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        conn.execute(
            "UPDATE events SET calendar_id = ?1 WHERE calendar_id = ?2",
            params![default_id, id],
        )?;
        conn.execute("DELETE FROM calendars WHERE id = ?1", params![id])?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_store() -> Store {
        Store::open_in_memory_for_test()
    }

    #[test]
    fn default_calendar_seeded() {
        let s = mem_store();
        let cals = s.list_calendars().unwrap();
        assert_eq!(cals.len(), 1);
        assert!(cals[0].is_default);
        assert_eq!(cals[0].kind, "mine");
    }

    #[test]
    fn create_toggle_and_delete_moves_events_to_default() {
        let s = mem_store();
        let default_id = s.list_calendars().unwrap()[0].id as i64;
        let work = s
            .upsert_calendar(&CalendarInput {
                name: "仕事".into(),
                color: Some("#e57373".into()),
                kind: Some("mine".into()),
                ..Default::default()
            })
            .unwrap();
        // 予定をこのカレンダーに作る
        let ev = crate::models::EventInput {
            title: "会議".into(),
            start_at: "2026-07-06T10:00".into(),
            calendar_id: Some(work.id),
            ..Default::default()
        };
        let saved = s.upsert_event(&ev).unwrap();
        assert_eq!(saved.calendar_id, Some(work.id));

        // 非表示にすると一覧から消える
        s.set_calendar_visible(work.id as i64, false).unwrap();
        assert_eq!(s.list_events("2026-07-01", "2026-08-01", false).unwrap().len(), 0);
        s.set_calendar_visible(work.id as i64, true).unwrap();
        assert_eq!(s.list_events("2026-07-01", "2026-08-01", false).unwrap().len(), 1);

        // 削除すると予定は既定カレンダーへ付け替わる
        assert!(s.delete_calendar(work.id as i64).unwrap());
        let after = s.get_event(saved.id as i64).unwrap();
        assert_eq!(after.calendar_id, Some(default_id as i32));

        // 既定カレンダーは削除不可
        assert!(!s.delete_calendar(default_id).unwrap());
    }
}
