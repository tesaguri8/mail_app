-- 連絡先・組織の「論理削除（ゴミ箱）」。削除は即時に消さず deleted_at を立てて非表示にし、
-- 保持期間（設定・既定 7 日）を過ぎたら完全削除する。統合は削除に当たらない（対象を実削除）。
ALTER TABLE contacts ADD COLUMN deleted_at TEXT;
ALTER TABLE organizations ADD COLUMN deleted_at TEXT;
CREATE INDEX IF NOT EXISTS idx_contacts_deleted_at ON contacts(deleted_at);
CREATE INDEX IF NOT EXISTS idx_organizations_deleted_at ON organizations(deleted_at);
