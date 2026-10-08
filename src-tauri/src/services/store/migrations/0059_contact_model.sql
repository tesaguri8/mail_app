-- 連絡先モデルの作り直し（docs/CONTACT_MODEL.md §5）。
--
-- リリース前なので、継ぎ足しの跡（本体の主値列と子テーブルの二重持ち、使われていない
-- contact_groups）を残さずに作り直す。個人の連絡先は一度すべて消し、同期で入れ直す
-- （利用者が了承済み 2026-10-08）。組織カード（organizations）とタグ（tags）は残す。
--
-- 外部キー（PRAGMA foreign_keys=ON）のもとで DROP TABLE は暗黙の DELETE を伴うので、
-- 子テーブル → 本体の順に消す。event_attendees.contact_id は ON DELETE SET NULL なので、
-- 予定の参加者はメール/氏名だけのゲストとして残る。

DROP TABLE IF EXISTS contact_group_members;
DROP TABLE IF EXISTS contact_groups;
DROP TABLE IF EXISTS contact_group_identities;
DROP TABLE IF EXISTS contact_identities;
DROP TABLE IF EXISTS contact_tags;
DROP TABLE IF EXISTS contact_emails;
DROP TABLE IF EXISTS contact_phones;
DROP TABLE IF EXISTS contact_addresses;
DROP TRIGGER IF EXISTS contacts_assign_uid;
DROP TABLE IF EXISTS contacts;

-- 本体（1 人 1 行。複数値は持たない）。
CREATE TABLE contacts (
    id INTEGER PRIMARY KEY,
    display_name TEXT NOT NULL,
    name_prefix TEXT,
    family_name TEXT,
    middle_name TEXT,
    given_name TEXT,
    name_suffix TEXT,
    phonetic_family TEXT,
    phonetic_middle TEXT,
    phonetic_given TEXT,
    nickname TEXT,
    maiden_name TEXT,
    -- 並び替え用（よみ優先。保存時に組み立てる）。
    sort_name TEXT,
    -- 'YYYY-MM-DD' / 年なし '--MM-DD'。
    birthday TEXT,
    note TEXT,
    avatar_path TEXT,
    show_as_company INTEGER NOT NULL DEFAULT 0,
    is_favorite INTEGER NOT NULL DEFAULT 0,
    is_business INTEGER NOT NULL DEFAULT 0,
    allow_remote_images INTEGER NOT NULL DEFAULT 0,
    -- 未送信のローカル変更がある（つながり表の dirty のどれかが 1、または未連携の新規）。
    dirty INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TEXT
);
CREATE INDEX idx_contacts_sort ON contacts(sort_name, display_name);
CREATE INDEX idx_contacts_birthday ON contacts(birthday);
CREATE INDEX idx_contacts_deleted_at ON contacts(deleted_at);
CREATE INDEX idx_contacts_dirty ON contacts(dirty) WHERE dirty = 1;

-- 会社（複数可）。org_id は組織カード。カードにつながっていなければ NULL。
CREATE TABLE contact_organizations (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    org_id INTEGER REFERENCES organizations(id) ON DELETE SET NULL,
    name TEXT,
    phonetic_name TEXT,
    title TEXT,
    department TEXT
);
CREATE INDEX idx_contact_organizations_cid ON contact_organizations(contact_id, position);
CREATE INDEX idx_contact_organizations_org ON contact_organizations(org_id);

-- メール。is_shared は会社の共有アドレスの印（Rondine 固有。送らない）。
CREATE TABLE contact_emails (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    label TEXT,
    value TEXT NOT NULL,
    is_shared INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_contact_emails_cid ON contact_emails(contact_id, position);
-- 差出人 ↔ 連絡先の照合（知り合い/お気に入り/表示名の解決）を小文字の完全一致で引く。
CREATE INDEX idx_contact_emails_value_lower ON contact_emails(lower(value));

-- 電話。is_shared は代表電話・代表 FAX の印。
CREATE TABLE contact_phones (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    label TEXT,
    value TEXT NOT NULL,
    is_shared INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_contact_phones_cid ON contact_phones(contact_id, position);

-- 住所（構造化）。
CREATE TABLE contact_addresses (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    label TEXT,
    po_box TEXT,
    postal TEXT,
    region TEXT,
    city TEXT,
    street TEXT,
    extended TEXT,
    country TEXT,
    country_code TEXT
);
CREATE INDEX idx_contact_addresses_cid ON contact_addresses(contact_id, position);

CREATE TABLE contact_urls (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    label TEXT,
    value TEXT NOT NULL
);
CREATE INDEX idx_contact_urls_cid ON contact_urls(contact_id, position);

-- 記念日など（誕生日は本体）。
CREATE TABLE contact_dates (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    label TEXT,
    date TEXT NOT NULL
);
CREATE INDEX idx_contact_dates_cid ON contact_dates(contact_id, position);

CREATE TABLE contact_relations (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    label TEXT,
    name TEXT NOT NULL
);
CREATE INDEX idx_contact_relations_cid ON contact_relations(contact_id, position);

-- チャット（im）と SNS（social）のハンドル。
CREATE TABLE contact_handles (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    kind TEXT NOT NULL CHECK (kind IN ('im', 'social')),
    service TEXT,
    value TEXT NOT NULL,
    label TEXT
);
CREATE INDEX idx_contact_handles_cid ON contact_handles(contact_id, position);

-- カスタム項目（Google の userDefined）。
CREATE TABLE contact_custom_fields (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    key TEXT NOT NULL,
    value TEXT NOT NULL
);
CREATE INDEX idx_contact_custom_fields_cid ON contact_custom_fields(contact_id, position);

-- タグ（メールと共通の tags）。
CREATE TABLE contact_tags (
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (contact_id, tag_id)
);
CREATE INDEX idx_contact_tags_tag ON contact_tags(tag_id);

-- つながり表: 1 人の連絡先が複数のサービス（provider の違う行）に同時につながってよい。
-- contact_id が NULL の行は「向こうにあるが、まだ住所録の誰とも結び付いていない」（未照合）。
-- dirty はこのつながりへ未送信のローカル変更がある印（つながりごとに送るため）。
CREATE TABLE contact_identities (
    id INTEGER PRIMARY KEY,
    provider TEXT NOT NULL CHECK (provider IN ('google', 'icloud')),
    -- 連携アカウント（Google は google_accounts.id）。
    account_id INTEGER NOT NULL,
    -- 向こうの ID（Google は 'people/c…'、iCloud は vCard の URL/UID）。
    external_id TEXT NOT NULL,
    contact_id INTEGER REFERENCES contacts(id) ON DELETE SET NULL,
    etag TEXT,
    -- 取り込んだ内容（ContactFields の JSON）。照合と差分判定の材料。
    snapshot TEXT,
    dirty INTEGER NOT NULL DEFAULT 0,
    fetched_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (provider, account_id, external_id)
);
CREATE INDEX idx_contact_identities_contact ON contact_identities(contact_id);
CREATE INDEX idx_contact_identities_unlinked
    ON contact_identities(account_id) WHERE contact_id IS NULL;
CREATE INDEX idx_contact_identities_dirty
    ON contact_identities(provider, account_id) WHERE dirty = 1;

-- Google のラベル（contactGroups）とタグ名の対応表（0058 と同じ形）。
CREATE TABLE contact_group_identities (
    id INTEGER PRIMARY KEY,
    provider TEXT NOT NULL DEFAULT 'google',
    account_id INTEGER NOT NULL,
    external_id TEXT NOT NULL,
    name TEXT NOT NULL,
    fetched_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (provider, account_id, external_id)
);
CREATE INDEX idx_contact_group_identities_name
    ON contact_group_identities(account_id, name);

-- Google の連絡先同期の印を戻す（次の取り込みがフル同期になり、全件を入れ直す）。
UPDATE google_accounts SET contacts_sync_token = NULL, last_contacts_sync_at = NULL;
