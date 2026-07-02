-- アカウントの並び順（ユーザーがドラッグ＆ドロップで変更）。既存は id 順で初期化。
ALTER TABLE accounts ADD COLUMN sort_order INTEGER;
UPDATE accounts SET sort_order = id WHERE sort_order IS NULL;
