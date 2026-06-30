-- 署名（signatures）を別管理し、メールアプリアカウント設定（accounts）から
-- 既定署名として紐づける。署名は使い回せるよう独立テーブルにする。
CREATE TABLE signatures (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    created_at TEXT DEFAULT (datetime('now'))
);

-- アカウントの既定署名（削除時は紐づけのみ解除）。
ALTER TABLE accounts ADD COLUMN signature_id INTEGER REFERENCES signatures(id) ON DELETE SET NULL;
