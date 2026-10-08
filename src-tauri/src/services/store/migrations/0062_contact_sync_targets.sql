-- 連絡先ごとの同期先（docs/CONTACT_MODEL.md §3「同期先は 1 人ずつ選ぶ」）。
--
-- 以前はアカウント単位の一括スイッチ（google_accounts.push_new_contacts）が、どこにも
-- つながっていない連絡先を全部 Google に作っていた（メールの相手まで電話帳に流れ込む）。
-- これからは利用者が 1 人ずつ選び、push_new_contacts は「新規作成時の既定のチェック」になる。

-- 作成待ち: 「Google（このアカウント）にも保存」を選んだが、まだ作っていない。
-- 次の同期の push で作成し、できたら contact_identities に行を作ってここを消す。
-- contact_identities.external_id は NOT NULL（読み手の多くが「つながり＝向こうの ID がある」
-- 前提）なので、作成待ちは別の表に置く。主キーで同じ人・同じアカウントの二重を防ぐ。
CREATE TABLE contact_create_requests (
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    provider TEXT NOT NULL DEFAULT 'google' CHECK (provider IN ('google', 'icloud')),
    account_id INTEGER NOT NULL,
    requested_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (contact_id, provider, account_id)
);

-- 同期をやめて、向こうの連絡先も消す（次の同期で削除を送り、送れたら行を消す）。
-- 「向こうは残す」ほうは行をすぐ消すので印は要らない。
ALTER TABLE contact_identities ADD COLUMN unlink_requested INTEGER NOT NULL DEFAULT 0;
