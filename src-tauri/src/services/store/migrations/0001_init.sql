-- Rondine 初期スキーマ（コア。docs/DATABASE_SCHEMA.md）
-- 連絡先/カレンダー/SNS/AI 等のテーブルは後続マイグレーションで追加する。

-- アカウント（同期範囲・保持の設定も保持。docs/SYNC.md）
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    email TEXT NOT NULL,
    display_name TEXT,
    imap_host TEXT NOT NULL,
    imap_port INTEGER DEFAULT 993,
    smtp_host TEXT NOT NULL,
    smtp_port INTEGER DEFAULT 587,
    auth_type TEXT DEFAULT 'password',
    sync_window TEXT DEFAULT '6m',
    body_fetch TEXT DEFAULT 'window',
    attachment_fetch TEXT DEFAULT 'on_demand',
    retention TEXT DEFAULT 'window',
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

-- 論理スレッド（アプリが引用解析で再構築。端末ローカル。docs/THREADING.md）
CREATE TABLE logical_threads (
    id INTEGER PRIMARY KEY,
    title TEXT,
    auto_title TEXT,
    participants TEXT,
    last_activity TEXT,
    message_count INTEGER DEFAULT 0,
    unread_count INTEGER DEFAULT 0,
    is_user_renamed INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now'))
);

-- メール本体。canonical_key で重複排除（docs/CROSS_CUTTING.md #1）
CREATE TABLE emails (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    message_id TEXT,
    canonical_key TEXT NOT NULL,
    thread_id TEXT,
    subject TEXT,
    from_address TEXT,
    to_addresses TEXT,
    cc_addresses TEXT,
    date TEXT,
    received_date TEXT,
    size INTEGER,
    has_attachments INTEGER DEFAULT 0,
    is_read INTEGER DEFAULT 0,
    is_flagged INTEGER DEFAULT 0,
    is_bookmarked INTEGER DEFAULT 0,
    needs_review INTEGER DEFAULT 0,
    follow_up_at TEXT,
    snooze_until TEXT,
    spam_score REAL,
    is_junk INTEGER DEFAULT 0,
    folder TEXT,
    raw_headers TEXT,
    body_plain TEXT,
    body_html TEXT,
    clean_body TEXT,
    body_fingerprint TEXT,
    logical_thread_id INTEGER,
    thread_assignment TEXT DEFAULT 'auto',
    thread_index TEXT,
    list_id TEXT,
    delivered_to TEXT,
    auth_result TEXT,
    created_at TEXT DEFAULT (datetime('now')),
    UNIQUE (account_id, canonical_key),
    FOREIGN KEY (account_id) REFERENCES accounts(id),
    FOREIGN KEY (logical_thread_id) REFERENCES logical_threads(id)
);

-- 添付
CREATE TABLE attachments (
    id INTEGER PRIMARY KEY,
    email_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT,
    size INTEGER,
    file_path TEXT,
    checksum TEXT,
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (email_id) REFERENCES emails(id)
);

-- 引用ブロック（属性行から from+時刻 抽出。docs/THREADING.md）
CREATE TABLE message_quotes (
    id INTEGER PRIMARY KEY,
    email_id INTEGER NOT NULL,
    block_order INTEGER,
    quoted_from TEXT,
    quoted_at TEXT,
    fingerprint TEXT,
    matched_email_id INTEGER,
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (email_id) REFERENCES emails(id)
);

-- タグ / カテゴリ（kind で区別。docs/FILTERING.md）
CREATE TABLE tags (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    kind TEXT DEFAULT 'tag',
    color TEXT,
    parent_id INTEGER,
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE email_tags (
    email_id INTEGER,
    tag_id INTEGER,
    assigned_at TEXT DEFAULT (datetime('now')),
    PRIMARY KEY (email_id, tag_id)
);

-- 保存フィルタ（スマートフォルダ。docs/FILTERING.md）
CREATE TABLE saved_filters (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    definition_json TEXT NOT NULL,
    is_pinned INTEGER DEFAULT 0,
    sort_order INTEGER,
    created_at TEXT DEFAULT (datetime('now'))
);

-- インデックス
CREATE INDEX idx_emails_account ON emails(account_id, date);
CREATE INDEX idx_emails_logical_thread ON emails(logical_thread_id, date);
CREATE INDEX idx_emails_from ON emails(from_address);
CREATE INDEX idx_emails_list_id ON emails(list_id);
CREATE INDEX idx_emails_bookmarked ON emails(is_bookmarked) WHERE is_bookmarked = 1;
CREATE INDEX idx_emails_review ON emails(needs_review, follow_up_at) WHERE needs_review = 1;
CREATE INDEX idx_quotes_match ON message_quotes(quoted_from, quoted_at);

-- 全文検索（FTS5。索引対象は clean_body＝引用除去後の本文。rowid = emails.id を入れる運用）
CREATE VIRTUAL TABLE email_fts USING fts5(subject, from_address, clean_body);
