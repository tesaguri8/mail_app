-- 複数フォルダ同期。受信箱に加えて送信済・下書き・ゴミ箱・迷惑メールを個別に取り込む。
-- 既存の受信メールは INBOX として明示する（folder 未設定を 'inbox' に）。
UPDATE emails SET folder = 'inbox' WHERE folder IS NULL OR folder = '';

-- フォルダごとの同期状態（uid_validity / last_uid をフォルダ単位で持つ）。
-- これまで accounts.uid_validity / last_uid に持っていた INBOX の状態をここへ移す。
CREATE TABLE folder_sync (
    account_id INTEGER NOT NULL,
    folder TEXT NOT NULL,          -- 'inbox' | 'sent' | 'drafts' | 'trash' | 'spam'
    uid_validity INTEGER,
    last_uid INTEGER,
    PRIMARY KEY (account_id, folder),
    FOREIGN KEY (account_id) REFERENCES accounts(id)
);

-- 既存アカウントの INBOX 同期状態を folder_sync へ移行（増分同期を継続できるように）。
INSERT OR IGNORE INTO folder_sync (account_id, folder, uid_validity, last_uid)
    SELECT id, 'inbox', uid_validity, last_uid FROM accounts;

-- フォルダ絞り込み（一覧表示）用インデックス。
CREATE INDEX IF NOT EXISTS idx_emails_folder ON emails(account_id, folder, date);
