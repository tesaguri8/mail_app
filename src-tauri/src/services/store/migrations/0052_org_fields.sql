-- 組織カードの連絡先情報（代表電話・FAX・代表メール・URL・所在地）。
-- 会社で共通の情報は組織レコード側に一本化し、個人の連絡先ではラベル表示（編集は組織カード）。
ALTER TABLE organizations ADD COLUMN phone TEXT;
ALTER TABLE organizations ADD COLUMN fax TEXT;
ALTER TABLE organizations ADD COLUMN email TEXT;
ALTER TABLE organizations ADD COLUMN url TEXT;
-- 所在地は連絡先の住所（contact_addresses）と同じ構成。組織は 1 か所（代表所在地）だけ持つ。
ALTER TABLE organizations ADD COLUMN postal TEXT;
ALTER TABLE organizations ADD COLUMN region TEXT;
ALTER TABLE organizations ADD COLUMN city TEXT;
ALTER TABLE organizations ADD COLUMN street TEXT;
ALTER TABLE organizations ADD COLUMN extended TEXT;
ALTER TABLE organizations ADD COLUMN country TEXT;
