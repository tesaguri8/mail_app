-- 任意タグ機能（docs/FILTERING.md のタグ層）。
-- tags / email_tags 本体は 0001_init.sql で作成済み。ここでは絞り込み用の索引を補う。
-- タグID でメールを引く（「このタグの付いたメール」一覧）ための索引。
-- email_tags の PK は (email_id, tag_id) のため tag_id 単独の検索が遅く、これを補完する。
CREATE INDEX IF NOT EXISTS idx_email_tags_tag ON email_tags(tag_id);
