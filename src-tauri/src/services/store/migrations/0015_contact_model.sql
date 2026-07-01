-- 連絡先モデルを Apple/Google 準拠へ拡張（docs/FEATURE_SPEC.md §2.4）。
-- 姓/名・よみ姓/よみ名・組織の役職/部署を構造化し、メール/電話/住所は
-- ラベル付きの複数値（子テーブル）にする。表示名(display_name)は FN として残す。

-- 名前の構造化＋組織詳細。
ALTER TABLE contacts ADD COLUMN family_name TEXT;
ALTER TABLE contacts ADD COLUMN given_name TEXT;
ALTER TABLE contacts ADD COLUMN phonetic_family TEXT;
ALTER TABLE contacts ADD COLUMN phonetic_given TEXT;
ALTER TABLE contacts ADD COLUMN org_title TEXT;       -- 役職
ALTER TABLE contacts ADD COLUMN org_department TEXT;  -- 部署

-- ラベル付き複数メール。
CREATE TABLE IF NOT EXISTS contact_emails (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL,
    label TEXT,                                 -- 自宅/職場/カスタム（会社名など）
    value TEXT NOT NULL,
    is_primary INTEGER NOT NULL DEFAULT 0,
    position INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
);

-- ラベル付き複数電話（携帯/自宅/職場(固定)/FAX 等）。
CREATE TABLE IF NOT EXISTS contact_phones (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL,
    label TEXT,
    value TEXT NOT NULL,
    is_primary INTEGER NOT NULL DEFAULT 0,
    position INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
);

-- ラベル付き複数住所（構造化）。
CREATE TABLE IF NOT EXISTS contact_addresses (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL,
    label TEXT,
    postal TEXT,        -- 郵便番号
    region TEXT,        -- 都道府県
    city TEXT,          -- 市区町村
    street TEXT,        -- 番地・建物
    extended TEXT,      -- 補足
    country TEXT,
    is_primary INTEGER NOT NULL DEFAULT 0,
    position INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_contact_emails_cid ON contact_emails(contact_id);
CREATE INDEX IF NOT EXISTS idx_contact_emails_val ON contact_emails(value);
CREATE INDEX IF NOT EXISTS idx_contact_phones_cid ON contact_phones(contact_id);
CREATE INDEX IF NOT EXISTS idx_contact_phones_val ON contact_phones(value);
CREATE INDEX IF NOT EXISTS idx_contact_addresses_cid ON contact_addresses(contact_id);

-- 既存の単一値を主(primary)として子テーブルへ移す。
INSERT INTO contact_emails (contact_id, value, is_primary, position)
    SELECT id, email, 1, 0 FROM contacts WHERE email IS NOT NULL AND trim(email) <> '';
INSERT INTO contact_phones (contact_id, value, is_primary, position)
    SELECT id, phone, 1, 0 FROM contacts WHERE phone IS NOT NULL AND trim(phone) <> '';
INSERT INTO contact_addresses (contact_id, street, is_primary, position)
    SELECT id, address, 1, 0 FROM contacts WHERE address IS NOT NULL AND trim(address) <> '';
