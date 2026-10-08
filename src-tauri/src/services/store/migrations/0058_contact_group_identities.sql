-- Google の連絡先グループ（＝「ラベル」）と Rondine のタグの対応表。
--
-- People API はグループを resourceName（'contactGroups/{id}'）で指し、所属の変更は
-- `contactGroups/{id}/members:modify` で行う。名前しか持っていないと ID を引けないので、
-- 取り込みのたびにこの表を Google の一覧で洗い替える。
--
-- Rondine 側のタグは名前が一意（tags.name UNIQUE）で、メールと共有している。ここに載っている
-- 名前だけを「Google が持っているラベル」とみなし、取り込みでの付け外しの対象にする。
-- 載っていない名前（利用者がアプリ内だけで付けたタグ）は Google 側の状態に関わらず触らない。
CREATE TABLE IF NOT EXISTS contact_group_identities (
    id INTEGER PRIMARY KEY,
    provider TEXT NOT NULL DEFAULT 'google',
    -- google_accounts.id（どの連携アカウントのラベルか）。
    account_id INTEGER NOT NULL,
    -- contactGroups/{id} の {id} 部分。
    external_id TEXT NOT NULL,
    -- ラベル名（Rondine 側の tags.name と突き合わせる鍵）。
    name TEXT NOT NULL,
    fetched_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(provider, account_id, external_id)
);

-- 名前から ID を引く（送信時）／アカウントの全ラベル名を引く（取り込みの付け外し）。
CREATE INDEX IF NOT EXISTS idx_contact_group_identities_name
    ON contact_group_identities(account_id, name);
