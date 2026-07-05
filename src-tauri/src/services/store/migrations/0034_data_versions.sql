-- 「再構築」の要否判断に使うデータ形式バージョン（services/dataver.rs が単一ソース）。
-- ingest_version: サーバー取り込み形式。取り込みが保存する内容が変わったら定数を上げる
--   → 記録が古いアカウントは全体再取り込みが必要と判定される。
-- parse_version: ローカル解析（引用分離・スレッド束ね）。パーサ改良で定数を上げる
--   → 記録が古くても保存済み本文からのローカル再解析だけで反映できる。
-- 既存アカウントは 0（＝形式不明）。最初の「再構築」で点検を兼ねた全体再取り込みになる。
ALTER TABLE accounts ADD COLUMN ingest_version INTEGER NOT NULL DEFAULT 0;
ALTER TABLE accounts ADD COLUMN parse_version INTEGER NOT NULL DEFAULT 0;
