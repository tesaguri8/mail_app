-- カレンダーのメタ拡張（docs/DATABASE_SCHEMA.md）。
-- ① 複数カレンダー（マイ/他）② 予定あり/なし(availability) ③ 公開設定(visibility)。
-- Google カレンダーと項目を揃え、ICS 相互運用の土台にする。

-- 複数カレンダー。kind でマイ/他を分け、visible で表示トグル、色で色分け。
CREATE TABLE IF NOT EXISTS calendars (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL DEFAULT '',          -- 空なら UI 側で既定名（マイカレンダー）を表示
    color TEXT,
    kind TEXT NOT NULL DEFAULT 'mine',       -- 'mine'（自分の）| 'other'（他の/購読）
    visible INTEGER NOT NULL DEFAULT 1,      -- 表示オン/オフ
    is_default INTEGER NOT NULL DEFAULT 0,   -- 既定（新規予定の初期カレンダー。削除不可）
    source TEXT NOT NULL DEFAULT 'local',    -- 'local' | 'ics' | 'google' | 'caldav'
    external_id TEXT,                        -- 連携元のID（同期・購読用。後続）
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- events にメタ列を追加（既定は Google 準拠: busy / default）。
ALTER TABLE events ADD COLUMN calendar_id INTEGER REFERENCES calendars(id);
ALTER TABLE events ADD COLUMN availability TEXT NOT NULL DEFAULT 'busy';   -- 'busy' | 'free'
ALTER TABLE events ADD COLUMN visibility TEXT NOT NULL DEFAULT 'default';  -- 'default' | 'public' | 'private'

-- 既定のマイカレンダーを1つ用意し、既存予定を割り当てる（名前は空＝UI で既定名表示）。
INSERT INTO calendars (name, color, kind, visible, is_default, sort_order)
VALUES ('', '#64b5f6', 'mine', 1, 1, 0);
UPDATE events SET calendar_id = (SELECT id FROM calendars WHERE is_default = 1)
WHERE calendar_id IS NULL;

CREATE INDEX IF NOT EXISTS idx_events_calendar ON events(calendar_id);

-- 参加者（ゲスト）を連絡先だけでなくメール/氏名でも持てるよう作り直す
-- （0038 で作成直後・データ無しのため DROP して再作成しても安全）。
DROP TABLE IF EXISTS event_attendees;
CREATE TABLE event_attendees (
    id INTEGER PRIMARY KEY,
    event_id INTEGER NOT NULL,
    contact_id INTEGER,                      -- 住所録と紐付く場合（任意）
    email TEXT,                              -- メールだけのゲスト（任意）
    name TEXT,                               -- 表示名（任意）
    response TEXT NOT NULL DEFAULT 'none',   -- 'accepted' | 'declined' | 'tentative' | 'none'
    FOREIGN KEY (event_id) REFERENCES events(id) ON DELETE CASCADE,
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_event_attendees_event ON event_attendees(event_id);
CREATE INDEX IF NOT EXISTS idx_event_attendees_contact ON event_attendees(contact_id);
