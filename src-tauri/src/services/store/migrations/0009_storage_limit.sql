-- アカウント毎のローカル保存容量の上限（バイト）。既定 2GB。
-- 上限を超えたら、保護対象（スター付き）以外で「最後に使ってから古い」添付バイトから
-- 順に追い出す（LRU）。メタ情報・本文は常に残し、添付は file_path を NULL にして
-- 必要時に再ダウンロードする。正本はサーバー。
ALTER TABLE accounts ADD COLUMN storage_limit INTEGER DEFAULT 2147483648;

-- 添付の最終アクセス時刻（ダウンロード/表示/オープン時に更新）。エビクションの LRU 基準。
ALTER TABLE attachments ADD COLUMN accessed_at TEXT;
