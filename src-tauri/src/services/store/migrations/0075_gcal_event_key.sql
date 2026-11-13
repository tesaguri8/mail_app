-- Google の予定を、カレンダーの中で予定 ID を引く（(calendar_id, external_id)。docs/CALENDAR_SYNC.md §3-3）。
-- 予定 ID はカレンダーをまたいで一意ではない: 招待された予定は主催者と招待された側の両方に同じ ID で載り、
-- カレンダーを移しても ID は変わらない（移動元には同じ ID が cancelled で載る）。これまでは ID だけで
-- 引いていたので、1 行が両方の間を行き来し、片方の取り消しがもう片方を消していた。

-- 同じカレンダーに同じ ID の行が重なっていたら 1 行に畳む（ID だけで引いていたので、通常は無い）。
-- 残すのは「削除されていない」>「未送信の変更がある」>「新しい行」。残さない行は連携を外して
-- ゴミ箱へ入れる（中身は消さない。送信もしない）。
UPDATE events
   SET external_id = NULL, etag = NULL, remote_calendar = NULL, source = 'local', dirty = 0,
       deleted_at = COALESCE(deleted_at, CURRENT_TIMESTAMP)
 WHERE source = 'google' AND external_id IS NOT NULL AND calendar_id IS NOT NULL
   AND id <> (SELECT k.id FROM events k
               WHERE k.source = 'google' AND k.calendar_id = events.calendar_id
                 AND k.external_id = events.external_id
               ORDER BY (k.deleted_at IS NULL) DESC, k.dirty DESC, k.id DESC
               LIMIT 1);

CREATE UNIQUE INDEX IF NOT EXISTS idx_events_google_key
    ON events(calendar_id, external_id) WHERE source = 'google' AND external_id IS NOT NULL;

-- 両方のカレンダーに載っている予定が、これまで片方にしか無かった（もう片方へ付け替えられていた・
-- 取り消しで消えていた）ので、次の同期で全件を取り直して揃える（同期トークンは残す）。
UPDATE calendars SET last_full_sync_at = NULL WHERE source = 'google';
