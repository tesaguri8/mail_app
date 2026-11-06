-- 未読のメールだけの部分索引（メール一覧の日の見出しの「未読 N」。docs/MAIL_LIST_TIMELINE.md §6）。
--
-- 日ごとの未読のスレッド数は、未読のメールから逆に引く（未読は少ないので、部分索引で探す範囲が
-- 未読の行だけになる）。列は「フォルダ・迷惑・アカウント・論理スレッド」で、未読の行はこの索引だけで
-- 読める（本文などの大きな行を読みに行かない）。代表の行と照らし合わせるのは idx_emails_thread_folder。
CREATE INDEX IF NOT EXISTS idx_emails_unread
    ON emails(folder, is_junk, account_id, logical_thread_id) WHERE is_read = 0;
