-- Google カレンダーの「最後に全件を取り直した時刻」（UTC 'YYYY-MM-DD HH:MM:SS'。未取得は NULL）。
--
-- 増分同期（syncToken）が予定を取りこぼしたまま「取得 0 件・次のトークンあり」を返し続けることがある
-- （`[実測]` vaio 2026-11-10: Google の画面にある予定が手元に一度も届かなかった）。24 時間を超えたら
-- 増分ではなく全件で取り直し、手元を Google に合わせる（返ってこなかった予定は削除扱い）。
-- 手動の「最初から同期し直す」はこれを NULL に戻して次の同期で全件にする。
ALTER TABLE calendars ADD COLUMN last_full_sync_at TEXT;
