-- 添付の再分類（既存メール向け）。以前は Content-Disposition: inline や Content-ID が
-- あるだけで inline 扱いにしていたため、inline 指定の PDF 等が添付一覧から漏れていた。
-- 本文に cid: で埋め込む「画像」だけを inline とし、それ以外は本来の添付に戻す。
UPDATE attachments SET kind = 'attachment'
WHERE kind = 'inline'
  AND NOT (content_id IS NOT NULL AND lower(content_type) LIKE 'image/%');

-- 上記で添付が復活したメールの has_attachments を立て直す（一覧の 📎 と本文の添付欄用）。
UPDATE emails SET has_attachments = 1
WHERE has_attachments = 0
  AND EXISTS (SELECT 1 FROM attachments a WHERE a.email_id = emails.id AND a.kind = 'attachment');
