-- 住所録（アドレス帳）。docs/FEATURE_SPEC.md §2.4 / docs/DATABASE_SCHEMA.md。
-- ローカル連絡先を基本とし、Google/iCloud 連携は後続（source/external_id で前方互換）。
CREATE TABLE IF NOT EXISTS contacts (
    id INTEGER PRIMARY KEY,
    display_name TEXT NOT NULL,
    name_kana TEXT,                             -- 読み（並び替え用）
    email TEXT,                                 -- 主メールアドレス
    emails TEXT,                                -- 追加アドレス（JSON。将来用）
    phone TEXT,
    organization TEXT,
    address TEXT,
    birthday TEXT,                              -- 誕生日（ホーム/ウィジェット通知用）
    note TEXT,
    avatar_path TEXT,
    is_favorite INTEGER NOT NULL DEFAULT 0,
    is_business INTEGER NOT NULL DEFAULT 0,     -- 取引先（docs/FILTERING.md）
    allow_remote_images INTEGER NOT NULL DEFAULT 0,  -- 外部画像許可（docs/MAIL_SECURITY.md）
    source TEXT NOT NULL DEFAULT 'local',       -- 'local' | 'google' | 'icloud' | ...
    external_id TEXT,                           -- 連携元のID（マージ・同期用）
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 連絡先グループ（編集 UI は後続。テーブルと一覧のみ先に用意）。
CREATE TABLE IF NOT EXISTS contact_groups (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    color TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 連絡先-グループ関連。
CREATE TABLE IF NOT EXISTS contact_group_members (
    contact_id INTEGER NOT NULL,
    group_id INTEGER NOT NULL,
    PRIMARY KEY (contact_id, group_id),
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE,
    FOREIGN KEY (group_id) REFERENCES contact_groups(id) ON DELETE CASCADE
);

-- 並び替え・絞り込み用の索引（docs/DATABASE_SCHEMA.md）。
CREATE INDEX IF NOT EXISTS idx_contacts_name     ON contacts(name_kana, display_name);
CREATE INDEX IF NOT EXISTS idx_contacts_email    ON contacts(email);
CREATE INDEX IF NOT EXISTS idx_contacts_birthday ON contacts(birthday);
CREATE INDEX IF NOT EXISTS idx_contacts_business ON contacts(is_business) WHERE is_business = 1;
CREATE INDEX IF NOT EXISTS idx_contact_group_members_g ON contact_group_members(group_id);
