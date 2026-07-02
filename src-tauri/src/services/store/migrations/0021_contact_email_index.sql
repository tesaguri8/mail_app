-- 差出人メール↔連絡先メールの照合（is_known/is_vip・表示名解決）を高速化する
-- 小文字化の式インデックス。from_address は素のメールアドレスのため完全一致で引ける。
CREATE INDEX IF NOT EXISTS idx_contacts_email_lower ON contacts(lower(email));
CREATE INDEX IF NOT EXISTS idx_contact_emails_value_lower ON contact_emails(lower(value));
