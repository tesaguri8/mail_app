-- 連絡先タグをメールと共通の tags に統合（案A）。
-- tags(name UNIQUE, kind, color, parent_id) を両者で共有し、連絡先↔タグを contact_tags で結ぶ。
-- タグは階層可（tags.parent_id。ブックマークのようにフォルダ整理できる）。
CREATE TABLE IF NOT EXISTS contact_tags (
    contact_id INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (contact_id, tag_id),
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE,
    FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_contact_tags_tag ON contact_tags(tag_id);

-- 旧 contact_groups の所属を tags/contact_tags へ移行（存在すれば）。
INSERT OR IGNORE INTO tags (name, kind, color)
    SELECT name, 'tag', color FROM contact_groups;
INSERT OR IGNORE INTO contact_tags (contact_id, tag_id)
    SELECT m.contact_id, t.id
    FROM contact_group_members m
    JOIN contact_groups g ON g.id = m.group_id
    JOIN tags t ON t.name = g.name;
