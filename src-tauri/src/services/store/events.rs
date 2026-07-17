use super::Store;
use crate::models::{AttendeeInput, EventAttendee, EventInput, EventSummary, IcsImportReport};
use crate::services::ics;
use rusqlite::{params, OptionalExtension, Row};

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
        calendar_id: r.get::<_, Option<i64>>(12)?.map(|v| v as i32),
        availability: r.get(13)?,
        visibility: r.get(14)?,
    })
}

const EVENT_COLS: &str = "id, title, description, location, start_at, end_at, all_day, color, \
     recurrence, reminder_minutes, related_email_id, deleted_at, calendar_id, availability, \
     visibility";

/// 任意テキストを trim し、空なら None に倒す（保存時に空文字を NULL 化して表示分岐を単純化）。
fn trimmed(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

/// query を LIKE 用にエスケープし、部分一致パターン（`%query%`）へ包む。
/// バックスラッシュを `ESCAPE '\'` に使うため、`\ % _` を `\` で退避する。
/// 空 query は `%%`＝任意一致になり、候補の「頻度順の既定リスト」に使える。
fn like_pattern(query: &str) -> String {
    let mut pattern = String::with_capacity(query.len() + 2);
    pattern.push('%');
    for ch in query.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern.push('%');
    pattern
}

impl Store {
    /// 期間 [from, to)（'YYYY-MM-DD' 等の ISO 文字列）に重なる予定を開始順で返す。
    /// 単発は overlap: start_at < to AND coalesce(end_at, start_at) >= from で抽出。
    /// 繰り返し（recurrence 非 null）は基準日が範囲外でも展開元として常に返し、
    /// 実際の出現はフロント側で範囲内に展開する（src/renderer/utils/recurrence.ts）。
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
        // 非表示カレンダーの予定は隠す（calendar_id が null＝既定扱いは常に表示）。
        let sql = format!(
            "SELECT {EVENT_COLS} FROM events \
             WHERE (recurrence IS NOT NULL \
                    OR (start_at < ?2 AND COALESCE(end_at, start_at) >= ?1)) {del} \
               AND (calendar_id IS NULL \
                    OR calendar_id IN (SELECT id FROM calendars WHERE visible = 1)) \
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

    /// タイトル・メモ・場所を横断して予定を検索する（部分一致・大文字小文字無視）。
    /// 非表示カレンダーとゴミ箱は除外。開始日時の新しい順（未来→過去）に最大 `limit` 件返す。
    /// 繰り返しは基準日の 1 件のみ返す（展開はしない。開けばシリーズ全体を編集できる）。
    pub fn search_events(&self, query: &str, limit: i64) -> rusqlite::Result<Vec<EventSummary>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let pattern = like_pattern(q);
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {EVENT_COLS} FROM events \
             WHERE deleted_at IS NULL \
               AND (calendar_id IS NULL \
                    OR calendar_id IN (SELECT id FROM calendars WHERE visible = 1)) \
               AND (title LIKE ?1 ESCAPE '\\' \
                    OR IFNULL(location, '') LIKE ?1 ESCAPE '\\' \
                    OR IFNULL(description, '') LIKE ?1 ESCAPE '\\') \
             ORDER BY start_at DESC, title COLLATE NOCASE \
             LIMIT ?2"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![pattern, limit], row_to_event)?;
        rows.collect()
    }

    /// 過去に入力した「場所」の候補を頻度順（同数なら直近入力順）で返す。オートコンプリート用。
    pub fn suggest_event_locations(
        &self,
        query: &str,
        limit: i64,
    ) -> rusqlite::Result<Vec<String>> {
        self.suggest_event_field("location", query, limit)
    }

    /// 過去に入力した「タイトル」の候補を頻度順（同数なら直近入力順）で返す。オートコンプリート用。
    pub fn suggest_event_titles(&self, query: &str, limit: i64) -> rusqlite::Result<Vec<String>> {
        self.suggest_event_field("title", query, limit)
    }

    /// title / location の過去入力を重複排除して頻度順に返す共通実装。
    /// `column` は呼び出し側の固定文字列のみ（ユーザー入力を渡さない＝SQL 埋め込み安全）。
    /// 空 query なら `%%` で全件を対象にし、よく使う値を上位に出す（フォーカス時の初期候補）。
    fn suggest_event_field(
        &self,
        column: &str,
        query: &str,
        limit: i64,
    ) -> rusqlite::Result<Vec<String>> {
        let pattern = like_pattern(query.trim());
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {column} FROM events \
             WHERE deleted_at IS NULL AND {column} IS NOT NULL AND TRIM({column}) <> '' \
               AND {column} LIKE ?1 ESCAPE '\\' \
             GROUP BY {column} COLLATE NOCASE \
             ORDER BY COUNT(*) DESC, MAX(start_at) DESC \
             LIMIT ?2"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![pattern, limit], |r| r.get::<_, String>(0))?;
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
            let availability = input.availability.as_deref().unwrap_or("busy");
            let visibility = input.visibility.as_deref().unwrap_or("default");
            // カレンダー未指定なら既定カレンダーに割り当てる（無ければ NULL＝既定扱い）。
            let calendar_id: Option<i64> = match input.calendar_id {
                Some(c) => Some(c as i64),
                None => conn
                    .query_row(
                        "SELECT id FROM calendars WHERE is_default = 1 LIMIT 1",
                        [],
                        |r| r.get(0),
                    )
                    .optional()?,
            };
            match input.id {
                Some(id) => {
                    // dirty=1: ユーザー編集は Google へ送信対象（Google カレンダー所属時のみ push される）。
                    conn.execute(
                        "UPDATE events SET \
                             title = ?1, description = ?2, location = ?3, start_at = ?4, \
                             end_at = ?5, all_day = ?6, color = ?7, recurrence = ?8, \
                             reminder_minutes = ?9, related_email_id = ?10, calendar_id = ?11, \
                             availability = ?12, visibility = ?13, dirty = 1, \
                             updated_at = CURRENT_TIMESTAMP \
                         WHERE id = ?14",
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
                            calendar_id,
                            availability,
                            visibility,
                            id,
                        ],
                    )?;
                    id as i64
                }
                None => {
                    // dirty=1: 新規作成もローカル変更として送信対象にする。
                    conn.execute(
                        "INSERT INTO events \
                             (title, description, location, start_at, end_at, all_day, color, \
                              recurrence, reminder_minutes, related_email_id, calendar_id, \
                              availability, visibility, dirty) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 1)",
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
                            calendar_id,
                            availability,
                            visibility,
                        ],
                    )?;
                    conn.last_insert_rowid()
                }
            }
        };
        self.get_event(id)
    }

    /// 予定を論理削除（ゴミ箱へ。deleted_at を立てて一覧から隠す。保持期間後に完全削除）。
    /// dirty=1 も立て、Google カレンダーの予定なら次回同期で Google 側も削除する。
    pub fn delete_event(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE events SET deleted_at = CURRENT_TIMESTAMP, dirty = 1 WHERE id = ?1",
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

    /// 予定の参加者（ゲスト）一覧。
    pub fn list_event_attendees(&self, event_id: i64) -> rusqlite::Result<Vec<EventAttendee>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, contact_id, email, name, response FROM event_attendees \
             WHERE event_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![event_id], |r| {
            Ok(EventAttendee {
                id: r.get::<_, i64>(0)? as i32,
                contact_id: r.get::<_, Option<i64>>(1)?.map(|v| v as i32),
                email: r.get(2)?,
                name: r.get(3)?,
                response: r.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// 参加者を入力の集合に一致させる（全置き換え）。空の値は除く。
    pub fn set_event_attendees(
        &self,
        event_id: i64,
        attendees: &[AttendeeInput],
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM event_attendees WHERE event_id = ?1",
            params![event_id],
        )?;
        for a in attendees {
            let email = a.email.as_deref().map(str::trim).filter(|v| !v.is_empty());
            let name = a.name.as_deref().map(str::trim).filter(|v| !v.is_empty());
            // メール・連絡先・名前のいずれも無い行は捨てる。
            if email.is_none() && a.contact_id.is_none() && name.is_none() {
                continue;
            }
            conn.execute(
                "INSERT INTO event_attendees (event_id, contact_id, email, name, response) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    event_id,
                    a.contact_id,
                    email,
                    name,
                    a.response.as_deref().unwrap_or("none"),
                ],
            )?;
        }
        Ok(())
    }

    /// 予定のリマインダー（開始何分前に通知するか）の一覧を昇順で返す。
    pub fn list_event_reminders(&self, event_id: i64) -> rusqlite::Result<Vec<i32>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT minutes FROM event_reminders WHERE event_id = ?1 ORDER BY minutes")?;
        let rows = stmt.query_map(params![event_id], |r| r.get::<_, i64>(0).map(|v| v as i32))?;
        rows.collect()
    }

    /// 予定のリマインダーを入力の集合に一致させる（全置き換え）。重複は除き昇順に正規化する。
    /// 互換列 events.reminder_minutes は最小値（最も早い通知。無ければ NULL）に更新し、ユーザー
    /// 編集として dirty=1 を立てる（Google カレンダー所属なら次の push で全通知を送る）。
    pub fn set_event_reminders(&self, event_id: i64, minutes: &[i32]) -> rusqlite::Result<()> {
        let mut mins: Vec<i32> = minutes.iter().copied().filter(|m| *m >= 0).collect();
        mins.sort_unstable();
        mins.dedup();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM event_reminders WHERE event_id = ?1",
            params![event_id],
        )?;
        for m in &mins {
            conn.execute(
                "INSERT OR IGNORE INTO event_reminders (event_id, minutes) VALUES (?1, ?2)",
                params![event_id, m],
            )?;
        }
        let earliest: Option<i32> = mins.first().copied();
        conn.execute(
            "UPDATE events SET reminder_minutes = ?1, dirty = 1, \
                updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
            params![earliest, event_id],
        )?;
        Ok(())
    }

    /// ICS テキストを取り込む（各 VEVENT を予定として追加。既定カレンダーへ）。
    pub fn import_ics(&self, text: &str) -> rusqlite::Result<IcsImportReport> {
        let parsed = ics::parse(text);
        let total = parsed.len() as i32;
        let mut imported = 0i32;
        for p in parsed {
            let input = EventInput {
                id: None,
                title: p.title,
                description: p.description,
                location: p.location,
                start_at: p.start_at,
                end_at: p.end_at,
                all_day: p.all_day,
                recurrence: p.recurrence,
                ..Default::default()
            };
            if input.title.trim().is_empty() || input.start_at.trim().is_empty() {
                continue;
            }
            self.upsert_event(&input)?;
            imported += 1;
        }
        Ok(IcsImportReport {
            total,
            imported,
            skipped: total - imported,
        })
    }

    /// 全予定（非削除）を ICS テキストへ書き出す。
    pub fn export_ics(&self) -> rusqlite::Result<String> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {EVENT_COLS} FROM events WHERE deleted_at IS NULL ORDER BY start_at"
        );
        let events: Vec<EventSummary> = {
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map([], row_to_event)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        drop(conn);
        Ok(ics::generate(&events))
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

    fn mem_store() -> Store {
        Store::open_in_memory_for_test()
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
    fn recurring_master_returned_regardless_of_range() {
        let s = mem_store();
        let mut e = ev("朝会", "2026-01-05T09:00", None, false);
        e.recurrence = Some("FREQ=WEEKLY".into());
        s.upsert_event(&e).unwrap();
        // 1月始まりの週次は、7月の範囲でも展開元として返る（実際の展開はフロント側）。
        let july = s.list_events("2026-07-01", "2026-08-01", false).unwrap();
        assert_eq!(july.len(), 1);
        assert_eq!(july[0].recurrence.as_deref(), Some("FREQ=WEEKLY"));
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

    /// 場所付きの予定を作るヘルパ。
    fn ev_loc(title: &str, start: &str, location: &str) -> EventInput {
        EventInput {
            title: title.into(),
            start_at: start.into(),
            location: Some(location.into()),
            ..Default::default()
        }
    }

    #[test]
    fn search_matches_title_location_and_description() {
        let s = mem_store();
        s.upsert_event(&ev_loc("歯医者", "2026-07-06T09:00", "みなと歯科クリニック"))
            .unwrap();
        s.upsert_event(&ev("ランチ", "2026-07-07T12:00", None, false)).unwrap();
        // タイトル一致
        assert_eq!(s.search_events("歯医者", 50).unwrap().len(), 1);
        // 場所の部分一致（大文字小文字・部分文字列）
        assert_eq!(s.search_events("歯科", 50).unwrap().len(), 1);
        // 非該当
        assert_eq!(s.search_events("会議", 50).unwrap().len(), 0);
        // 空クエリは 0 件（呼び出し側の無駄打ち抑止）
        assert_eq!(s.search_events("  ", 50).unwrap().len(), 0);
    }

    #[test]
    fn search_excludes_trashed() {
        let s = mem_store();
        let a = s.upsert_event(&ev("面談", "2026-07-06T09:00", None, false)).unwrap();
        assert_eq!(s.search_events("面談", 50).unwrap().len(), 1);
        s.delete_event(a.id as i64).unwrap();
        assert_eq!(s.search_events("面談", 50).unwrap().len(), 0);
    }

    #[test]
    fn location_suggestions_rank_by_frequency() {
        let s = mem_store();
        // 「本社」を 2 回、「支社」を 1 回使う → 本社が上位。
        s.upsert_event(&ev_loc("A", "2026-07-01T09:00", "本社ビル")).unwrap();
        s.upsert_event(&ev_loc("B", "2026-07-02T09:00", "本社ビル")).unwrap();
        s.upsert_event(&ev_loc("C", "2026-07-03T09:00", "支社ビル")).unwrap();
        let all = s.suggest_event_locations("", 8).unwrap();
        assert_eq!(all.first().map(String::as_str), Some("本社ビル"));
        assert_eq!(all.len(), 2);
        // 部分一致で絞り込み
        let hit = s.suggest_event_locations("支社", 8).unwrap();
        assert_eq!(hit, vec!["支社ビル".to_string()]);
    }

    #[test]
    fn title_suggestions_dedupe_and_filter() {
        let s = mem_store();
        s.upsert_event(&ev("定例ミーティング", "2026-07-01T09:00", None, false)).unwrap();
        s.upsert_event(&ev("定例ミーティング", "2026-07-08T09:00", None, false)).unwrap();
        s.upsert_event(&ev("ランチ", "2026-07-02T12:00", None, false)).unwrap();
        // 重複タイトルは 1 件に集約
        let all = s.suggest_event_titles("", 8).unwrap();
        assert_eq!(all.iter().filter(|t| *t == "定例ミーティング").count(), 1);
        // 前方の部分一致
        let hit = s.suggest_event_titles("定例", 8).unwrap();
        assert_eq!(hit, vec!["定例ミーティング".to_string()]);
    }
}
