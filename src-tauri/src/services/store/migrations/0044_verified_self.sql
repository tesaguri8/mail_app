-- 「本物の自分から」検証（docs/SPAM.md）。送信時に X-Rondine-Self（Message-ID の HMAC）を
-- 付与し、受信時に同じ秘密で検証する。verified_self=1 のメールは（サーバーが迷惑に振り分けても）
-- 受信箱に出し、バッジを付ける。
ALTER TABLE emails ADD COLUMN verified_self INTEGER NOT NULL DEFAULT 0;

-- アカウント単位の検証用 HMAC 秘密（16 進）。資格情報ではなくマーク用の鍵で、これ単体では
-- 何のアクセス権も持たない。NULL は未生成（初回の自分宛送信時に生成する）。
ALTER TABLE accounts ADD COLUMN self_secret TEXT;
