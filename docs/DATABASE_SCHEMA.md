# データベース設計

**ステータス:** 計画（実装未着手）
**出典:** 旧 `README_plan.md` §4。
**実装:** Rust `rusqlite`（`bundled-sqlcipher` + FTS5）。マイグレーションは `src-tauri/src/services/store/` で自前のバージョン管理 SQL として適用する（Alembic は不採用）。

---

## 1. 主要テーブル

```sql
-- アカウント
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    email TEXT NOT NULL,
    display_name TEXT,
    imap_host TEXT NOT NULL,
    imap_port INTEGER DEFAULT 993,
    smtp_host TEXT NOT NULL,
    smtp_port INTEGER DEFAULT 587,
    auth_type TEXT DEFAULT 'password',   -- 'password' | 'oauth2'（将来）
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- メール
CREATE TABLE emails (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    message_id TEXT UNIQUE NOT NULL,
    thread_id TEXT,
    subject TEXT,
    from_address TEXT,
    to_addresses TEXT,
    cc_addresses TEXT,
    bcc_addresses TEXT,
    date TIMESTAMP,
    received_date TIMESTAMP,
    size INTEGER,
    has_attachments BOOLEAN DEFAULT FALSE,
    is_read BOOLEAN DEFAULT FALSE,
    is_flagged BOOLEAN DEFAULT FALSE,
    folder_id INTEGER,
    raw_headers TEXT,
    body_plain TEXT,
    body_html TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id)
);

-- スレッド
CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    subject TEXT,
    participants TEXT,
    last_activity TIMESTAMP,
    message_count INTEGER DEFAULT 0,
    unread_count INTEGER DEFAULT 0,
    has_attachments BOOLEAN DEFAULT FALSE
);

-- タグ
CREATE TABLE tags (
    id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL,
    color TEXT,
    parent_id INTEGER,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (parent_id) REFERENCES tags(id)
);

-- メール-タグ関連
CREATE TABLE email_tags (
    email_id INTEGER,
    tag_id INTEGER,
    assigned_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (email_id, tag_id),
    FOREIGN KEY (email_id) REFERENCES emails(id),
    FOREIGN KEY (tag_id) REFERENCES tags(id)
);

-- 添付ファイル
CREATE TABLE attachments (
    id INTEGER PRIMARY KEY,
    email_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT,
    size INTEGER,
    file_path TEXT,
    checksum TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (email_id) REFERENCES emails(id)
);

-- 連絡先（住所録）
CREATE TABLE contacts (
    id INTEGER PRIMARY KEY,
    display_name TEXT NOT NULL,
    name_kana TEXT,                 -- 読み（並び替え用）
    email TEXT,                     -- 主メールアドレス
    emails TEXT,                    -- 追加アドレス（JSON）
    phone TEXT,
    organization TEXT,
    address TEXT,
    birthday TEXT,                  -- 誕生日（ホーム/ウィジェット通知用）
    note TEXT,
    avatar_path TEXT,
    is_favorite BOOLEAN DEFAULT FALSE,
    source TEXT DEFAULT 'local',    -- 'local' | 'google' | 'icloud' | ...
    external_id TEXT,               -- 連携元のID（マージ・同期用）
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 連絡先グループ
CREATE TABLE contact_groups (
    id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL,
    color TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 連絡先-グループ関連
CREATE TABLE contact_group_members (
    contact_id INTEGER,
    group_id INTEGER,
    PRIMARY KEY (contact_id, group_id),
    FOREIGN KEY (contact_id) REFERENCES contacts(id),
    FOREIGN KEY (group_id) REFERENCES contact_groups(id)
);

-- カレンダー予定
CREATE TABLE events (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    location TEXT,
    start_at TIMESTAMP NOT NULL,
    end_at TIMESTAMP,
    all_day BOOLEAN DEFAULT FALSE,
    recurrence TEXT,                -- RRULE（iCal 形式）
    reminder_minutes INTEGER,       -- 開始何分前に通知
    color TEXT,
    source TEXT DEFAULT 'local',    -- 'local' | 'ics' | 'google' | 'caldav'
    external_id TEXT,               -- 連携元のID（同期用）
    related_email_id INTEGER,       -- メールから作成した場合の紐付け
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (related_email_id) REFERENCES emails(id)
);

-- 予定の参加者（連絡先と紐付け）
CREATE TABLE event_attendees (
    event_id INTEGER,
    contact_id INTEGER,
    response TEXT DEFAULT 'none',   -- 'accepted' | 'declined' | 'tentative' | 'none'
    PRIMARY KEY (event_id, contact_id),
    FOREIGN KEY (event_id) REFERENCES events(id),
    FOREIGN KEY (contact_id) REFERENCES contacts(id)
);

-- 検索インデックス（FTS5）
CREATE VIRTUAL TABLE email_fts USING fts5(
    subject,
    from_address,
    to_addresses,
    body_plain,
    content=emails,
    content_rowid=id
);

-- 連絡先の検索インデックス（任意。住所録が大きくなる場合）
CREATE VIRTUAL TABLE contact_fts USING fts5(
    display_name,
    name_kana,
    email,
    organization,
    content=contacts,
    content_rowid=id
);
```

---

## 2. インデックス戦略

```sql
CREATE INDEX idx_emails_thread_id      ON emails(thread_id);
CREATE INDEX idx_emails_date           ON emails(date DESC);
CREATE INDEX idx_emails_from           ON emails(from_address);
CREATE INDEX idx_emails_account_folder ON emails(account_id, folder_id);
CREATE INDEX idx_email_tags_tag_id     ON email_tags(tag_id);

-- 住所録・カレンダー
CREATE INDEX idx_contacts_name      ON contacts(name_kana, display_name);
CREATE INDEX idx_contacts_email     ON contacts(email);
CREATE INDEX idx_contacts_birthday  ON contacts(birthday);
CREATE INDEX idx_events_start       ON events(start_at);
CREATE INDEX idx_event_attendees_c  ON event_attendees(contact_id);
```

---

## 3. 実装上の注意

- **本文の保存**: 大きな本文・添付はファイルシステムへ退避し、DB には索引・メタデータを保持する設計も検討（[DATA_STORAGE.md](DATA_STORAGE.md) 参照）。
- **FTS5 同期**: `emails` への INSERT/UPDATE/DELETE 時に `email_fts` を更新（トリガまたはアプリ側で明示更新）。差分同期と整合させる。
- **暗号化**: SQLCipher により DB ファイル全体を暗号化。鍵は `keyring`（OS 金庫）で管理。
- **マイグレーション**: `user_version` プラグマ等でスキーマバージョンを管理し、起動時に未適用分を順次適用。
