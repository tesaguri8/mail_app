-- 引用ブロックを手元のメールと照らし合わせるための索引（docs/THREADING.md §2）。
--
-- 引用の見出しの日時（"Sent: Friday, October 2, 2026 5:55 PM"）は時差が書かれていないので、
-- 前後 14 時間の幅で emails を引いてから、差出人と時刻で絞る。既存の索引はどれも
-- account_id か folder が先頭なので、日時だけの範囲では効かない。
CREATE INDEX IF NOT EXISTS idx_emails_date_ts ON emails(date_ts);
