-- 予定が「今 Google 上のどのカレンダーに存在するか」（Google カレンダー ID）を保持する。
-- ローカル calendar_id はユーザーの付け替えでズレ得るため、Google 上の実在場所を別に持ち、
-- カレンダー移動時に「実在場所から削除 → 新カレンダーへ作成」を確実に行う（docs/CALENDAR_SYNC.md）。
ALTER TABLE events ADD COLUMN remote_calendar TEXT;

-- 既存の Google 予定は、現在の所属カレンダーの external_id で backfill（未移動なら正しい）。
UPDATE events SET remote_calendar = (
    SELECT c.external_id FROM calendars c
    WHERE c.id = events.calendar_id AND c.source = 'google'
)
WHERE source = 'google';
