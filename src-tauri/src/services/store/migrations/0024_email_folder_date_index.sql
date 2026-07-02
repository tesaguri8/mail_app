-- 「全て」表示（全アカウント横断）の並び替えを高速化するインデックス。
-- account を絞らない ORDER BY date_ts でも先頭 N 件をインデックスで引ける。
CREATE INDEX IF NOT EXISTS idx_emails_folder_date ON emails(folder, date_ts DESC, id DESC);
