-- 添付の取得キーを IMAP section パス（"1"/"2"/"1.1" 等）に（BODYSTRUCTURE 化）。
-- 開いた時に BODY[section] で該当パートだけ取得できるようにする（本体を先読みしない）。
-- 既存行は NULL（「添付メタ再導出」で埋める。無い間は従来の part_index 経路にフォールバック）。
ALTER TABLE attachments ADD COLUMN section TEXT;
