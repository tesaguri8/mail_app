-- rondine-id: 連絡先ごとの安定した正準 ID（UUIDv4 文字列）。
-- 再取り込み・統合・端末移行でも変わらないマスターキー。将来の複数プロバイダ同期
-- （Google/iCloud 等）で「同じ人」を 1 つに束ねる軸になる（docs/FEATURE_SPEC.md §2.4）。
-- 提供元IDの対応表 contact_identities は API 同期の実装時に追加する。
ALTER TABLE contacts ADD COLUMN uid TEXT;

-- 既存行に UUIDv4 を採番（randomblob は行ごとに評価され一意）。
UPDATE contacts
SET uid = lower(
    hex(randomblob(4)) || '-' ||
    hex(randomblob(2)) || '-4' ||
    substr(hex(randomblob(2)), 2) || '-' ||
    substr('89ab', abs(random()) % 4 + 1, 1) ||
    substr(hex(randomblob(2)), 2) || '-' ||
    hex(randomblob(6))
)
WHERE uid IS NULL;

-- 以降の INSERT で uid 未指定なら自動採番する。
CREATE TRIGGER IF NOT EXISTS contacts_assign_uid
AFTER INSERT ON contacts
WHEN NEW.uid IS NULL
BEGIN
    UPDATE contacts SET uid = lower(
        hex(randomblob(4)) || '-' ||
        hex(randomblob(2)) || '-4' ||
        substr(hex(randomblob(2)), 2) || '-' ||
        substr('89ab', abs(random()) % 4 + 1, 1) ||
        substr(hex(randomblob(2)), 2) || '-' ||
        hex(randomblob(6))
    )
    WHERE id = NEW.id;
END;

CREATE UNIQUE INDEX IF NOT EXISTS idx_contacts_uid ON contacts(uid);
