-- 一覧（thread_list）の副問い合わせ「スレッド×フォルダの未読数・メールID収集」を索引で直接引く。
-- これらは WHERE logical_thread_id=? AND folder=? だが、SQLite が folder 索引を選んで
-- フォルダ全体（数千件）を走査していた（代表100行×フォルダ全件＝数十万件）。
-- (logical_thread_id, folder) の複合索引で、スレッド内の数件だけを直接引けるようにする。
CREATE INDEX IF NOT EXISTS idx_emails_thread_folder ON emails(logical_thread_id, folder);
