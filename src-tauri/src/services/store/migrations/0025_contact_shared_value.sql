-- 「複数名で共有するアドレス」（会社の代表メール info@… / 代表電話 / 代表FAX 等）を
-- 人単位の重複判定の手掛かりから除外するためのフラグ。メール・電話（FAX含む）の両方に付与する。
-- 共有指定された値は「組織 ＋ 値 ＋ 共有件数」として集計でき、誤検知を抑える。
ALTER TABLE contact_emails ADD COLUMN is_shared INTEGER NOT NULL DEFAULT 0;
ALTER TABLE contact_phones ADD COLUMN is_shared INTEGER NOT NULL DEFAULT 0;
