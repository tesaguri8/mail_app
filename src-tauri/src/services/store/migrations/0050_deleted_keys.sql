-- 破棄した下書きの「墓標」（tombstone）。docs/COMPOSE.md §1。
-- 送信・破棄でローカルから消した下書きは、サーバー Drafts のコピーも削除するが、その削除は
-- best-effort（失敗しうる）で、しかも実行中の同期は削除前に取得した一覧を持っている。そのため
-- 「消したはずの下書きが次の同期で復活する」ことがあった。消した canonical_key を墓標として
-- 残し、取り込み側（insert_email）で弾く。remote_pending=1 はサーバー側のコピーがまだ消えて
-- いない印で、同期のたびに削除を再試行する。
CREATE TABLE IF NOT EXISTS deleted_keys (
    account_id     INTEGER NOT NULL,
    -- emails.canonical_key（フォルダ接頭辞つき。例: 'drafts:draft-1-...@example.com'）
    canonical_key  TEXT    NOT NULL,
    -- サーバー側の削除に使う Message-ID の中身（山括弧なし。無ければ NULL）
    message_id     TEXT,
    -- 消したときのローカルフォルダ（現状は 'drafts' のみ）
    folder         TEXT    NOT NULL,
    -- サーバー側のコピー削除がまだ済んでいない（1=次回同期で再試行）
    remote_pending INTEGER NOT NULL DEFAULT 0,
    -- 墓標を残した時刻（Unix 秒）。役目を終えたものは一定期間で掃除する。
    deleted_at     INTEGER NOT NULL,
    PRIMARY KEY (account_id, canonical_key)
);

-- 同期のたびに引く「再試行が要る墓標」の絞り込み用。
CREATE INDEX IF NOT EXISTS idx_deleted_keys_pending
    ON deleted_keys(account_id, remote_pending);
