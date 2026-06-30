-- メールサーバーアカウント設定（接続＋ログイン）を正規化し、
-- メールアプリアカウント設定（accounts）から紐づける。複数アカウントで共有・再利用できる。
CREATE TABLE server_accounts (
    id INTEGER PRIMARY KEY,
    name TEXT,
    imap_host TEXT NOT NULL,
    imap_port INTEGER NOT NULL DEFAULT 993,
    imap_security TEXT DEFAULT 'ssl',
    smtp_host TEXT NOT NULL,
    smtp_port INTEGER NOT NULL DEFAULT 587,
    smtp_security TEXT DEFAULT 'starttls',
    username TEXT NOT NULL,
    created_at TEXT DEFAULT (datetime('now')),
    UNIQUE (imap_host, imap_port, username)
);

ALTER TABLE accounts ADD COLUMN server_account_id INTEGER REFERENCES server_accounts(id);

-- 既存アカウントからサーバー設定を生成して紐づける（後方互換の移行）。
INSERT OR IGNORE INTO server_accounts (name, imap_host, imap_port, smtp_host, smtp_port, username)
    SELECT DISTINCT imap_host, imap_host, imap_port, smtp_host, smtp_port,
           COALESCE(NULLIF(username, ''), email)
    FROM accounts;

UPDATE accounts SET server_account_id = (
    SELECT id FROM server_accounts s
    WHERE s.imap_host = accounts.imap_host AND s.imap_port = accounts.imap_port
      AND s.username = COALESCE(NULLIF(accounts.username, ''), accounts.email)
);
