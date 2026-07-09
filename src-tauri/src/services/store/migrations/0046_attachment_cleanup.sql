-- 添付の誤登録を掃除する。mail_parser の .attachments() が本文の代替パート（text/x-amp-html 等）や
-- multipart コンテナを返すことがあり、それらが「添付」として登録されていた（本文バックフィルで
-- 既存メールにも大量に混入）。本来の添付は「実ファイル名を持つ」か「cid: 埋め込み(Content-ID)」の
-- どちらかなので、それ以外（合成名 attachment-N・cid なし・拡張子なし）を削除する。
DELETE FROM attachments
WHERE content_id IS NULL
  AND filename LIKE 'attachment-%'
  AND filename NOT LIKE '%.%';

-- 残った本来の添付から has_attachments を立て直す（誤登録で立っていたフラグを下ろす）。
UPDATE emails SET has_attachments = CASE
    WHEN EXISTS (SELECT 1 FROM attachments a WHERE a.email_id = emails.id AND a.kind = 'attachment')
    THEN 1 ELSE 0 END
WHERE has_attachments = 1;
