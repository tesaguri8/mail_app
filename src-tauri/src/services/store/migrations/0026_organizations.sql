-- 会社・組織を独立レコード化（docs/FEATURE_SPEC.md §2.4 組織）。
-- 連絡先は org_id で組織を参照（照合はID）。連絡先の organization 文字列は
-- 表示・後方互換・重複判定のため、紐づく組織名と同期して残す。
CREATE TABLE IF NOT EXISTS organizations (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    name_kana TEXT,
    note TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

ALTER TABLE contacts ADD COLUMN org_id INTEGER REFERENCES organizations(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_contacts_org_id ON contacts(org_id);

-- 既存の組織名（文字列）から組織レコードを作成し、連絡先を紐づける。
-- ここでは完全一致でグループ化する（「株式会社◯◯」と「(株)◯◯」の統一は重複整理で行う）。
INSERT INTO organizations (name)
    SELECT DISTINCT trim(organization) FROM contacts
    WHERE organization IS NOT NULL AND trim(organization) <> '';

UPDATE contacts SET org_id = (
    SELECT o.id FROM organizations o WHERE o.name = trim(contacts.organization)
) WHERE organization IS NOT NULL AND trim(organization) <> '';
