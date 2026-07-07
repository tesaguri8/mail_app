-- 全件メタデータ索引の下ごしらえ（docs/SYNC.md §3.6「取り込みの進め方」）。
-- サーバ総数（左下「ローカル/サーバ」表示）と、本文の取得状態を持つ。

-- サーバ側のメール総数（IMAP SELECT の EXISTS）。フォルダ単位で毎同期に更新する。
ALTER TABLE folder_sync ADD COLUMN server_total INTEGER;

-- 全件メタデータ索引が完了したフォルダの印（これ以上古い UID が無い）。0=未完 / 1=完了。
ALTER TABLE folder_sync ADD COLUMN index_complete INTEGER NOT NULL DEFAULT 0;

-- 本文の取得状態: 'present'（全文あり）/ 'evicted'（要約のみ＝旧 body_compacted）/ 'absent'（メタのみ・未取得）。
ALTER TABLE emails ADD COLUMN body_state TEXT NOT NULL DEFAULT 'present';

-- 既存行を移行: 要約済み(body_compacted=1)は 'evicted'、それ以外は 'present'。
UPDATE emails SET body_state = CASE WHEN body_compacted = 1 THEN 'evicted' ELSE 'present' END;
