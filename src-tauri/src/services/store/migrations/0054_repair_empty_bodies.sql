-- 本文が空なのに「取得済み」になっている行を、取り直し対象へ戻す。
--
-- alpha.13 の「メタ先行・本文後追い」で、text/plain のメールをヘッダだけ取り込むと
-- mail_parser が空本文から `<html><body></body></html>` を合成していた。これを本文と
-- 数えていたため body_state='present' になり、開いても本文を取りに行かず永久に空のまま
-- だった（実データで発生。services/store/emails.rs の has_html_body）。
--
-- 読める本文が 1 文字も無い行を 'absent' に戻すと、次に開いたときの自動取得が走る。
-- 画像だけの HTML メール（タグを剥がすと文字が残らない）も巻き込むが、取り直しが 1 回
-- 走って 'present' に戻るだけで害は無い。
UPDATE emails
   SET body_state = 'absent'
 WHERE body_state = 'present'
   AND COALESCE(TRIM(body_plain), '') = ''
   AND COALESCE(TRIM(clean_body), '') = '';
