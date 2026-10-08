-- Google カレンダーの繰り返し予定の「1 回だけの変更・削除」（例外インスタンス）を取り込む
-- （docs/CALENDAR_SYNC.md §3-5）。0055〜0059 は連絡先同期の枝が使うため 60 を使う。

-- 1 回だけ変更された回は、通常の予定行として持ち、どの本体の・どの回かを記録する。
--   recurring_external_id: 本体（繰り返し元）の Google 予定 ID（recurringEventId）
--   original_start_at    : 本体の展開上の元の開始（originalStartTime をローカル表現にしたもの）
ALTER TABLE events ADD COLUMN recurring_external_id TEXT;
ALTER TABLE events ADD COLUMN original_start_at TEXT;
CREATE INDEX IF NOT EXISTS idx_events_recurring
    ON events(calendar_id, recurring_external_id) WHERE recurring_external_id IS NOT NULL;

-- 1 回だけ削除された回（status=cancelled の例外）。予定としては表示しないので events には
-- 置かず、本体の展開から除く日時だけを持つ。カレンダーごと消えたら一緒に消える。
CREATE TABLE IF NOT EXISTS event_cancelled_instances (
    calendar_id INTEGER NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,                  -- 例外インスタンスの Google 予定 ID
    recurring_external_id TEXT NOT NULL,        -- 本体の Google 予定 ID
    original_start_at TEXT NOT NULL,            -- 削除された回の元の開始（ローカル表現）
    PRIMARY KEY (calendar_id, external_id)
);
CREATE INDEX IF NOT EXISTS idx_event_cancelled_master
    ON event_cancelled_instances(calendar_id, recurring_external_id);

-- 既存 DB はこれまでの例外を取りこぼしているので、Google カレンダーをフル同期し直させる。
UPDATE calendars SET sync_token = NULL WHERE source = 'google';
