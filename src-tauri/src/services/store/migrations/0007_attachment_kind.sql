-- 添付を「本来の添付ファイル」と「本文埋め込み画像（inline asset）」に分類する。
-- kind: 'attachment'（ユーザーが添付したファイル）| 'inline'（HTML本文の cid: 埋め込み）。
-- content_id: cid: 参照の解決に使う Content-ID（前後の山括弧 <> は除去して保存）。
ALTER TABLE attachments ADD COLUMN kind TEXT NOT NULL DEFAULT 'attachment';
ALTER TABLE attachments ADD COLUMN content_id TEXT;
