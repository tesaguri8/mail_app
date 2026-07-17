-- 予定の通知（リマインダー）を複数持てるようにする（docs/DATABASE_SCHEMA.md）。
-- これまでは events.reminder_minutes に単一の「開始何分前」だけを持っていたが、Google
-- カレンダー同様に複数の通知（15分前・1時間前 …）を設定できるよう別テーブルへ切り出す。
--
-- events.reminder_minutes は互換のため残し、以後は「最も早い通知（最小の分）」を表す派生値
-- として維持する（一覧のベルアイコン表示・スケジューラの絞り込みに使う）。全リマインダーの
-- 正本はこの event_reminders テーブル。

CREATE TABLE IF NOT EXISTS event_reminders (
    event_id INTEGER NOT NULL,
    minutes INTEGER NOT NULL,                 -- 開始何分前に通知するか（0=開始時刻）
    PRIMARY KEY (event_id, minutes),
    FOREIGN KEY (event_id) REFERENCES events(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_event_reminders_event ON event_reminders(event_id);

-- 既存の単一リマインダーを移行する（NULL は通知なし＝行を作らない）。
INSERT OR IGNORE INTO event_reminders (event_id, minutes)
SELECT id, reminder_minutes FROM events WHERE reminder_minutes IS NOT NULL;
