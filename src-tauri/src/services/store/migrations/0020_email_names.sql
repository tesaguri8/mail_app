-- 差出人/宛先の表示名（ヘッダ From/To の名前部）を保存する列。
-- 既存メールは NULL のまま（住所録から解決してフォールバック表示する）。
ALTER TABLE emails ADD COLUMN from_name TEXT;
ALTER TABLE emails ADD COLUMN to_name TEXT;
