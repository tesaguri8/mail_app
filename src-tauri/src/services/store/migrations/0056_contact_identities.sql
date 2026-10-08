-- Google 連絡先（People API）の取り込み台帳。0017 が「提供元 ID の対応表 contact_identities は
-- API 同期の実装時に追加する」と予告していたもの。
--
-- ローカル連絡先 1 件に対し、プロバイダごとの ID を 0..n 本ぶら下げる。contacts.external_id は
-- 1 プロバイダ分しか持てないので、同期の突き合わせはこちらを正とする。
--
-- contact_id が NULL の行は「Google 側にあるが、まだローカルの誰とも結び付いていない」状態。
-- 初回同期では全件がこの状態になる。取り込んだ内容をそのまま contacts へ入れないのは、
-- 既存の住所録と全件重複させないため（照合は後続フェーズで行う。docs/CALENDAR_SYNC.md）。
CREATE TABLE IF NOT EXISTS contact_identities (
    id INTEGER PRIMARY KEY,
    provider TEXT NOT NULL DEFAULT 'google',
    -- google_accounts.id（どの連携アカウント由来か）。
    account_id INTEGER NOT NULL,
    -- People API の resourceName（'people/c1234567890'）。
    external_id TEXT NOT NULL,
    -- 紐付いたローカル連絡先。NULL＝未照合。
    contact_id INTEGER,
    -- People API は更新時に etag 必須（読んだ版の etag を送らないと弾かれる）。
    etag TEXT,
    -- 取り込んだ内容（ImportedContact の JSON）。照合と差分判定の材料。
    snapshot TEXT,
    -- Google 側で削除された（増分同期の metadata.deleted）。
    remote_deleted INTEGER NOT NULL DEFAULT 0,
    fetched_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(provider, account_id, external_id),
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_contact_identities_contact ON contact_identities(contact_id);
-- 照合フェーズが引く「未照合の取り込み分」。
CREATE INDEX IF NOT EXISTS idx_contact_identities_unlinked
    ON contact_identities(account_id) WHERE contact_id IS NULL;

-- People API の増分同期トークンはアカウント単位（connections.list が返す nextSyncToken）。
-- カレンダーがカレンダー単位で持つのとは異なる。
ALTER TABLE google_accounts ADD COLUMN contacts_sync_token TEXT;
