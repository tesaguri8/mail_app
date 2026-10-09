-- アカウントの一本化（docs/ACCOUNTS.md §2-3）。
--
-- 設定の「アカウント」をアドレスごとに 1 枚のカードにまとめ、その中でメール・連絡先・
-- カレンダーを選べるようにする。今の 3 表（server_accounts / accounts / google_accounts）は
-- 作り直さず、カードにあたる表を 1 つ足して accounts と google_accounts から指す。
-- contact_identities.account_id はこれまでどおり google_accounts.id を指す。
CREATE TABLE account_profiles (
    id INTEGER PRIMARY KEY,
    -- サービスの提供元。カードの見出しと、オンにするときに求める認証の種類を決める。
    provider TEXT NOT NULL CHECK (provider IN ('google', 'icloud', 'imap')),
    email TEXT NOT NULL,
    -- カードの呼び名（利用者が付ける。NULL ならアドレスで出す）。差出人名は accounts 側に残す。
    display_name TEXT,
    sort_order INTEGER,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
-- カードはアドレスごとに 1 枚（大文字小文字は区別しない）。
CREATE UNIQUE INDEX idx_account_profiles_email ON account_profiles (lower(email));

ALTER TABLE accounts ADD COLUMN profile_id INTEGER REFERENCES account_profiles(id);
ALTER TABLE google_accounts ADD COLUMN profile_id INTEGER REFERENCES account_profiles(id);

-- 既存のメールアカウントから、アドレスごとに 1 枚。同じアドレスが重複登録されていれば
-- 並び順の先頭の行を代表にする（min() の集約では、裸の列はその行の値になる）。
-- 提供元: Google 連携がある・Gmail のサーバー → google / iCloud のサーバー → icloud / 他は imap。
INSERT INTO account_profiles (provider, email, sort_order)
SELECT
    CASE
        WHEN EXISTS (SELECT 1 FROM google_accounts g
                     WHERE g.provider = 'google' AND lower(g.email) = lower(a.email))
            THEN 'google'
        WHEN lower(a.imap_host) IN ('imap.gmail.com', 'imap.googlemail.com') THEN 'google'
        WHEN lower(a.imap_host) = 'imap.mail.me.com' THEN 'icloud'
        WHEN lower(substr(a.email, instr(a.email, '@') + 1)) IN ('gmail.com', 'googlemail.com')
            THEN 'google'
        WHEN lower(substr(a.email, instr(a.email, '@') + 1)) IN ('icloud.com', 'me.com', 'mac.com')
            THEN 'icloud'
        ELSE 'imap'
    END,
    a.email,
    min(COALESCE(a.sort_order, a.id))
FROM accounts a
GROUP BY lower(a.email);

-- メールを使っていない Google 連携（連絡先・カレンダーだけ）は、メールのカードの後ろに並べる。
INSERT INTO account_profiles (provider, email, sort_order)
SELECT 'google', g.email,
       (SELECT COALESCE(max(sort_order), 0) FROM account_profiles) + g.id
FROM google_accounts g
WHERE g.provider = 'google'
  AND NOT EXISTS (SELECT 1 FROM account_profiles p WHERE lower(p.email) = lower(g.email));

UPDATE accounts SET profile_id =
    (SELECT p.id FROM account_profiles p WHERE lower(p.email) = lower(accounts.email));
UPDATE google_accounts SET profile_id =
    (SELECT p.id FROM account_profiles p WHERE lower(p.email) = lower(google_accounts.email))
WHERE provider = 'google';
