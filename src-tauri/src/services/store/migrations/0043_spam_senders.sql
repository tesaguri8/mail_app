-- 迷惑差出人（docs/SPAM.md）。「このアドレスを迷惑にしたら、同じアドレスのメールも
-- （既存も今後も）迷惑（spam）扱いにする」ための、差出人アドレス単位の一覧。
-- ブロック（受信拒否）ではなく迷惑メール扱い（is_junk=1・学習と復帰が効く）に寄せる。
CREATE TABLE IF NOT EXISTS spam_senders (
    address    TEXT PRIMARY KEY,          -- 正規化済みの差出人アドレス（小文字・<>除去）
    created_at INTEGER NOT NULL DEFAULT 0  -- epoch 秒
);

-- 差出人一致の一括隔離/復帰を速くする（大文字小文字を無視して一致）。
CREATE INDEX IF NOT EXISTS idx_emails_from_address_nocase
    ON emails(from_address COLLATE NOCASE);
