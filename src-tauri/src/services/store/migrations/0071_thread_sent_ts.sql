-- 一覧の並びを「会話の最後のやり取り」にする（docs/THREADING.md §5）。
--
-- 自分の返信は送信済みフォルダにあるので、受信箱の代表（フォルダ内の最新）の日時だけで並べると、
-- 返信した会話が相手のメールの時刻の位置に残る。会話ごとに「自分が最後に送った時刻」を
-- thread_sent_ts として会話の行（下書きを除く）に持ち、並びの鍵を
--   max(date_ts, coalesce(thread_sent_ts, date_ts))
-- にする。代表の行だけの式索引で、一覧は先頭 N 件を索引で引ける（emails.rs の activity_ts と同じ式）。
--
-- 自分の送信 = 送信済みフォルダ（folder='sent'）の、迷惑に隔離していないメール。下書き・ゴミ箱・迷惑は数えない。
ALTER TABLE emails ADD COLUMN thread_sent_ts INTEGER;

-- 既存データを埋める（利用者に「再構築」を要らせない）。
UPDATE emails SET thread_sent_ts = s.ts
FROM (SELECT logical_thread_id AS tid, MAX(date_ts) AS ts FROM emails
      WHERE folder = 'sent' AND is_junk = 0 AND logical_thread_id IS NOT NULL
      GROUP BY logical_thread_id) AS s
WHERE emails.logical_thread_id = s.tid AND emails.folder <> 'drafts';

-- 代表の日時順の索引（0032）は使わなくなるので、新しい並びの式索引に置き換える。
DROP INDEX IF EXISTS idx_emails_folder_rep;
CREATE INDEX IF NOT EXISTS idx_emails_folder_rep_activity
  ON emails(folder, max(date_ts, coalesce(thread_sent_ts, date_ts))) WHERE is_folder_rep = 1;
