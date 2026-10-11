-- 組織カード（organizations）を連絡先に統合する（docs/ORG_CONTACTS.md §2-1）。
--
-- 組織も連絡先の 1 件（contacts.kind = 'organization'）にする。組織の名前とよみは、その連絡先の
-- contact_organizations の position = 0 の行（自分の会社の行。org_id = NULL）に持ち、代表電話・FAX・
-- 代表メール・URL・所在地は、いまある子テーブルへ写す。人から組織へのつながり（org_id）は組織の
-- 連絡先を指すよう付け替え、organizations は表ごと消す（二重に持たない）。
--
-- 同じ名前の「会社名だけの連絡先」とまとめるのは、正規化（dedupe::normalize_org）が SQL で
-- 書けないので、起動時に 1 回だけ Rust で行う（store/org_contacts.rs）。同期の印は戻さない。

-- 1. 種別。
ALTER TABLE contacts ADD COLUMN kind TEXT NOT NULL DEFAULT 'person'
    CHECK (kind IN ('person', 'organization'));

-- 2. 組織カード 1 枚ごとに組織の連絡先を作る（ゴミ箱のカードはゴミ箱のまま）。
--    新旧の id の対応は作業用の列に控え、最後に消す。uid はトリガー（0064）が振る。
ALTER TABLE contacts ADD COLUMN legacy_org_id INTEGER;
INSERT INTO contacts (display_name, sort_name, note, kind, legacy_org_id,
                      created_at, updated_at, deleted_at)
    SELECT trim(name),
           COALESCE(NULLIF(trim(name_kana), ''), trim(name)),
           NULLIF(trim(note), ''),
           'organization', id, created_at, updated_at, deleted_at
    FROM organizations;

-- 3. 代表の情報を子テーブルへ写す（ラベルは docs/ORG_CONTACTS.md §1-3）。
INSERT INTO contact_phones (contact_id, position, label, value)
    SELECT c.id, 0, '代表', trim(o.phone)
    FROM contacts c JOIN organizations o ON o.id = c.legacy_org_id
    WHERE COALESCE(trim(o.phone), '') <> '';
INSERT INTO contact_phones (contact_id, position, label, value)
    SELECT c.id, CASE WHEN COALESCE(trim(o.phone), '') <> '' THEN 1 ELSE 0 END, 'FAX', trim(o.fax)
    FROM contacts c JOIN organizations o ON o.id = c.legacy_org_id
    WHERE COALESCE(trim(o.fax), '') <> '';
INSERT INTO contact_emails (contact_id, position, label, value)
    SELECT c.id, 0, '職場', trim(o.email)
    FROM contacts c JOIN organizations o ON o.id = c.legacy_org_id
    WHERE COALESCE(trim(o.email), '') <> '';
INSERT INTO contact_urls (contact_id, position, label, value)
    SELECT c.id, 0, 'ホームページ', trim(o.url)
    FROM contacts c JOIN organizations o ON o.id = c.legacy_org_id
    WHERE COALESCE(trim(o.url), '') <> '';
INSERT INTO contact_addresses (contact_id, position, label, postal, region, city, street,
                               extended, country)
    SELECT c.id, 0, '職場', NULLIF(trim(o.postal), ''), NULLIF(trim(o.region), ''),
           NULLIF(trim(o.city), ''), NULLIF(trim(o.street), ''), NULLIF(trim(o.extended), ''),
           NULLIF(trim(o.country), '')
    FROM contacts c JOIN organizations o ON o.id = c.legacy_org_id
    WHERE COALESCE(trim(o.postal), '') || COALESCE(trim(o.region), '') ||
          COALESCE(trim(o.city), '') || COALESCE(trim(o.street), '') ||
          COALESCE(trim(o.extended), '') || COALESCE(trim(o.country), '') <> '';

-- 4. contact_organizations を作り直す（org_id は組織の連絡先を指す）。
--    人の行は新しい id へ付け替え、組織の連絡先には自分の会社の行（名前・よみ）を足す。
CREATE TABLE contact_organizations_new (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    -- つながっている組織の連絡先（kind = 'organization'）。組織の自分の会社の行では NULL。
    org_id INTEGER REFERENCES contacts(id) ON DELETE SET NULL,
    name TEXT,
    phonetic_name TEXT,
    title TEXT,
    department TEXT
);
INSERT INTO contact_organizations_new (id, contact_id, position, org_id, name, phonetic_name,
                                       title, department)
    SELECT co.id, co.contact_id, co.position,
           (SELECT c.id FROM contacts c WHERE c.legacy_org_id = co.org_id),
           co.name, co.phonetic_name, co.title, co.department
    FROM contact_organizations co;
INSERT INTO contact_organizations_new (contact_id, position, org_id, name, phonetic_name)
    SELECT c.id, 0, NULL, trim(o.name), NULLIF(trim(o.name_kana), '')
    FROM contacts c JOIN organizations o ON o.id = c.legacy_org_id;
DROP TABLE contact_organizations;
ALTER TABLE contact_organizations_new RENAME TO contact_organizations;
CREATE INDEX idx_contact_organizations_cid ON contact_organizations(contact_id, position);
CREATE INDEX idx_contact_organizations_org ON contact_organizations(org_id);

-- 5. 「会社として表示」（iCloud の X-ABShowAs:COMPANY）は種別に一本化する。
--    姓名が空で印のある人は組織に切り替え、姓名のある人は印を落とす（要判断 A）。
UPDATE contacts SET kind = 'organization'
    WHERE show_as_company = 1
      AND COALESCE(trim(family_name), '') || COALESCE(trim(middle_name), '') ||
          COALESCE(trim(given_name), '') = '';
ALTER TABLE contacts DROP COLUMN show_as_company;
-- 組織になった連絡先の自分の会社の行は、どこも指さない（自分や同名の組織を指さない）。
UPDATE contact_organizations SET org_id = NULL
    WHERE position = 0
      AND contact_id IN (SELECT id FROM contacts WHERE kind = 'organization');

-- 6. 片付け。
ALTER TABLE contacts DROP COLUMN legacy_org_id;
DROP TABLE organizations;
CREATE INDEX idx_contacts_kind ON contacts(kind) WHERE kind = 'organization';
