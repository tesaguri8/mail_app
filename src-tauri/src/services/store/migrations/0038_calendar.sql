-- カレンダー予定。docs/DATABASE_SCHEMA.md（events / event_attendees）。
-- v1 はローカル1カレンダーの予定 CRUD（月/アジェンダ表示）。繰り返し(RRULE)・
-- リマインダー・外部同期(ICS/Google/CalDAV)は後続段階（列だけ前方互換で用意）。
-- 日時は端末ローカルの素の ISO8601 文字列で持つ（終日='YYYY-MM-DD' / 時間指定='YYYY-MM-DDTHH:MM'）。
-- ゼロ詰め ISO なら辞書順比較＝時刻順になり、範囲抽出を単純な文字列比較で行える。
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    location TEXT,
    start_at TEXT NOT NULL,                     -- 開始（終日は日付のみ）
    end_at TEXT,                                -- 終了（任意。終日の複数日は最終日を含む）
    all_day INTEGER NOT NULL DEFAULT 0,         -- 終日フラグ
    recurrence TEXT,                            -- RRULE（後続。iCal 形式）
    reminder_minutes INTEGER,                   -- 開始何分前に通知（後続）
    color TEXT,                                 -- 色分け（任意）
    source TEXT NOT NULL DEFAULT 'local',       -- 'local' | 'ics' | 'google' | 'caldav'
    external_id TEXT,                           -- 連携元のID（同期用。後続）
    related_email_id INTEGER,                   -- メールから作成した場合の紐付け（後続）
    deleted_at TIMESTAMP,                       -- 論理削除（ゴミ箱）。非 null＝削除済み
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (related_email_id) REFERENCES emails(id) ON DELETE SET NULL
);

-- 予定の参加者（連絡先と紐付け。UI は後続。テーブルのみ前方互換で用意）。
CREATE TABLE IF NOT EXISTS event_attendees (
    event_id INTEGER NOT NULL,
    contact_id INTEGER NOT NULL,
    response TEXT NOT NULL DEFAULT 'none',      -- 'accepted' | 'declined' | 'tentative' | 'none'
    PRIMARY KEY (event_id, contact_id),
    FOREIGN KEY (event_id) REFERENCES events(id) ON DELETE CASCADE,
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
);

-- 範囲抽出（月/週の表示）と参加者引きの索引（docs/DATABASE_SCHEMA.md）。
CREATE INDEX IF NOT EXISTS idx_events_start        ON events(start_at);
CREATE INDEX IF NOT EXISTS idx_events_related_email ON events(related_email_id);
CREATE INDEX IF NOT EXISTS idx_event_attendees_c   ON event_attendees(contact_id);
