-- 迷惑メールのローカル学習（docs/SPAM.md §4 / §7.6）。
-- emails.spam_score / is_junk は 0001_init.sql で既存のため、ここでは学習方向の記録列と
-- トークン統計・総数カウンタのみを足す。

-- 各メールの「最後に学習した向き」。再マーク訂正で旧カウントを打ち消すのに使う（§4.2 / §7.3）。
ALTER TABLE emails ADD COLUMN spam_learned INTEGER DEFAULT 0; -- -1=ham学習 / 0=未学習 / 1=spam学習

-- トークン別の spam/ham 出現メール数（同一メール内の重複は dedup 後に1カウント。§4.3）。
CREATE TABLE spam_tokens (
    token      TEXT PRIMARY KEY,   -- 名前空間付き: "w:無料" / "ng:振込" / "url:example.com" / "from:example.com" ...
    spam_count INTEGER DEFAULT 0,
    ham_count  INTEGER DEFAULT 0,
    updated_at INTEGER DEFAULT 0   -- epoch 秒。古い語の刈り込み（vacuum）判断に使用（§4.3）
);

-- 学習メタ（総数カウンタ等）。1行 key-value でスキーマ追加に強くする。
CREATE TABLE spam_meta (
    key   TEXT PRIMARY KEY,        -- "n_spam" / "n_ham" / "model_version"
    value INTEGER NOT NULL         -- スコア計算（ラプラス平滑化。§7.2）に必須の学習メール総数
);
INSERT OR IGNORE INTO spam_meta(key, value) VALUES ('n_spam', 0), ('n_ham', 0);

-- 迷惑フォルダ一覧の絞り込み用（is_junk = 1 の部分索引）。
CREATE INDEX idx_emails_junk ON emails(is_junk) WHERE is_junk = 1;
