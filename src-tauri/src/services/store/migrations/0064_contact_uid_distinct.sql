-- 連絡先の uid と「別人」の記録（利用者の判断 2026-10-10・docs/CONTACT_MODEL.md §1-1・§1-5）。
--
-- uid: 端末をまたいで同じ人を指す ID（UUID v4・小文字ハイフン区切り）。行の id は DB の中だけの
-- 番号で、入れ直しや書き出し/取り込みで変わるので、人を指す鍵には uid を使う。統合では残る側の
-- uid を使い、消える側の uid は捨てる。
--
-- 既存の表に NOT NULL の列は後から足せない（作り直しは外部キーが多く危うい）ので、列は NULL を
-- 許し、一意の索引と「空なら振る」トリガーで、どの作成経路でも必ず振られるようにする
-- （新規作成・取り込み・Google からの起こし・試験の直書き）。
ALTER TABLE contacts ADD COLUMN uid TEXT;

UPDATE contacts SET uid =
    lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' ||
    substr(lower(hex(randomblob(2))), 2) || '-' ||
    substr('89ab', 1 + (abs(random()) % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' ||
    lower(hex(randomblob(6)))
WHERE uid IS NULL;

CREATE UNIQUE INDEX idx_contacts_uid ON contacts(uid);

CREATE TRIGGER contacts_uid_assign AFTER INSERT ON contacts
WHEN NEW.uid IS NULL
BEGIN
    UPDATE contacts SET uid =
        lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' ||
        substr(lower(hex(randomblob(2))), 2) || '-' ||
        substr('89ab', 1 + (abs(random()) % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' ||
        lower(hex(randomblob(6)))
    WHERE id = NEW.id;
END;

-- 「別人」の対: 重複の整理で「別人（統合しない）」を押した組・統合でチェックを外した人。
-- 学習ではなく判断の記録で、重複の検出とまとめての統合がこれに従う（同じ組にしない）。
-- 連絡先が消えたら対も消える（CASCADE）。統合では、消える側の対を残る側へ付け替えてから消す。
CREATE TABLE contact_distinct_pairs (
    uid_a TEXT NOT NULL REFERENCES contacts(uid) ON DELETE CASCADE ON UPDATE CASCADE,
    uid_b TEXT NOT NULL REFERENCES contacts(uid) ON DELETE CASCADE ON UPDATE CASCADE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (uid_a, uid_b),
    CHECK (uid_a < uid_b)
);
CREATE INDEX idx_contact_distinct_pairs_b ON contact_distinct_pairs(uid_b);
