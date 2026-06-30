-- 添付メタの保存と、オンデマンド再取得のための情報を追加。
-- 同期時は本体を落とさずメタ（ファイル名/型/サイズ）だけ attachments に保存し、
-- ダウンロード時に emails.uid + attachments.part_index で IMAP から該当パートを再取得する。

-- メッセージの IMAP UID（再取得のキー。uid_validity は accounts 側で管理）。
ALTER TABLE emails ADD COLUMN uid INTEGER;

-- 添付が message 内の何番目の attachment かを示す序数（mail-parser の attachments() 序数）。
ALTER TABLE attachments ADD COLUMN part_index INTEGER;

CREATE INDEX idx_attachments_email ON attachments(email_id);
