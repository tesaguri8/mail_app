-- 差分同期のための状態（docs/SYNC.md）。
-- uid_validity が変わったら作り直し、last_uid 超のメッセージだけ取得する。
ALTER TABLE accounts ADD COLUMN uid_validity INTEGER;
ALTER TABLE accounts ADD COLUMN last_uid INTEGER;
