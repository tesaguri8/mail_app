-- 下書きに手入力した Bcc を保存できるようにする列。返信などで自動設定はしない
-- （Bcc は受信側から見えないため引き継げない）。ユーザーが自分で足した Bcc の保持用。
-- 通常の受信メールでは NULL のままでよい。
ALTER TABLE emails ADD COLUMN bcc_addresses TEXT;
