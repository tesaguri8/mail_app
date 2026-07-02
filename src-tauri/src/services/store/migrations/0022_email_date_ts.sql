-- 一覧の並び替え（新しい順）をインデックスで引けるようにする epoch 秒の列。
-- これまで ORDER BY datetime(date) は関数適用でインデックスが効かず、件数分の
-- 全走査＋ソートが毎回発生していた（2万件規模で顕著に遅い）。
ALTER TABLE emails ADD COLUMN date_ts INTEGER;

-- 既存行を date（rfc3339/ISO8601。TZ 付きも SQLite が UTC 換算）から埋める。
UPDATE emails SET date_ts = CAST(strftime('%s', date) AS INTEGER) WHERE date IS NOT NULL;

-- 旧データの folder NULL を 'inbox' に正規化（インデックスで folder= を引けるように）。
UPDATE emails SET folder = 'inbox' WHERE folder IS NULL;

-- 一覧クエリ用の複合インデックス（アカウント×フォルダ×新しい順）。先頭 N 件を直接引ける。
CREATE INDEX IF NOT EXISTS idx_emails_list ON emails(account_id, folder, date_ts DESC, id DESC);
