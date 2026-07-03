-- グリーンドメイン（ユーザーが認めた安全な差出人ドメイン）と警告ドメイン（意図的に除外）。
-- 判定 is_green は「差出人が住所録本人(is_known)」または「差出人ドメインがグリーン
-- （手動認定 ∪ 住所録由来・フリーメール除く）かつ警告リストに無い」。docs/GREEN_DOMAINS.md。
CREATE TABLE IF NOT EXISTS green_domains (
    domain TEXT PRIMARY KEY,
    note TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
-- グリーンから意図的に外したドメイン。住所録由来の自動グリーンを上書き除外し、再登録を防ぐ。
CREATE TABLE IF NOT EXISTS warning_domains (
    domain TEXT PRIMARY KEY,
    note TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
