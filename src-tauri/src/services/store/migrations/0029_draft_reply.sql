-- 下書きの返信情報。作成画面で書いた返信下書きを再編集・再送信するとき、
-- 返信元の Message-ID（In-Reply-To）を復元してスレッド化を保つために保持する。
-- 通常のメールでは NULL のままでよい（下書き保存時のみ設定する）。
ALTER TABLE emails ADD COLUMN in_reply_to TEXT;
