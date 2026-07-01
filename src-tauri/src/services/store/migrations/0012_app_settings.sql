-- アプリ設定の汎用 key-value（docs/SPAM.md §9）。
-- 機密でない設定を Rust 側の単一ソースで保持する（資格情報は keyring、これは非機密）。
-- 既存の sync_window / storage_limit（accounts 列）と同じく DB を設定の単一ソースにする。
CREATE TABLE app_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
