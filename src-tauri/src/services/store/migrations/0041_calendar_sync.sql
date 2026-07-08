-- Google カレンダー双方向同期のための同期メタ（docs/CALENDAR_SYNC.md）。
-- ① 連携アカウント（複数 Google 対応）② calendars/events に同期状態列を追加。
-- calendars.source / external_id と events.source / external_id は 0038/0039 で作成済み。

-- 連携した Google アカウント（複数対応。資格情報＝refresh_token は keyring、ここはメタのみ）。
CREATE TABLE IF NOT EXISTS calendar_accounts (
    id INTEGER PRIMARY KEY,
    provider TEXT NOT NULL DEFAULT 'google',   -- 将来 'caldav' 等へ拡張
    email TEXT NOT NULL,                        -- 連携した Google アカウント（keyring のキー）
    external_id TEXT,                           -- Google の安定 ID（sub）
    last_sync_at TIMESTAMP,                     -- 最終同期時刻（UTC）
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(provider, email)
);

-- calendars に同期メタを追加。
ALTER TABLE calendars ADD COLUMN account_id INTEGER REFERENCES calendar_accounts(id);
ALTER TABLE calendars ADD COLUMN sync_token TEXT;            -- Google 増分同期トークン（nextSyncToken）
ALTER TABLE calendars ADD COLUMN access_role TEXT;          -- owner|writer|reader|freeBusyReader
ALTER TABLE calendars ADD COLUMN sync_enabled INTEGER NOT NULL DEFAULT 1;  -- 同期対象にするか

-- events に同期メタを追加。
ALTER TABLE events ADD COLUMN etag TEXT;                     -- Google etag（参考・将来の競合検出用）
ALTER TABLE events ADD COLUMN dirty INTEGER NOT NULL DEFAULT 0;  -- ローカル変更が未 push（1＝要送信）

CREATE INDEX IF NOT EXISTS idx_events_external  ON events(external_id);
CREATE INDEX IF NOT EXISTS idx_events_dirty     ON events(dirty) WHERE dirty = 1;
CREATE INDEX IF NOT EXISTS idx_calendars_account ON calendars(account_id);
