-- ログイン用サーバーユーザー名をメールアドレス（差出人）と分離する。
-- 例: メール suematsu@sng-design.com / ログイン suematsu@sngdesign.sakura.ne.jp
-- NULL の場合はログイン時に email をユーザー名として用いる。
ALTER TABLE accounts ADD COLUMN username TEXT;
