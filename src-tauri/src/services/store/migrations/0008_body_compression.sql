-- 本文 HTML を zstd 圧縮して保存するための BLOB 列。
-- 新規メールは body_html_z にだけ書き、TEXT の body_html は NULL にする。
-- 既存行は起動時に一度だけ圧縮して body_html_z へ移し、body_html を NULL にする。
-- 検索対象の clean_body は非圧縮のまま（FTS・プレビューで使うため）。
ALTER TABLE emails ADD COLUMN body_html_z BLOB;
