-- Google 連携アカウントを「カレンダー専用」から「サービス共通」へ一般化する。
--
-- 連絡先同期（People API）を足すとき、アカウント表をサービスごとに分けると同じ Google
-- アカウントを二重に連携させることになる（同意 2 回・refresh_token 2 本・UI に重複表示）。
-- アカウント 1 件 = refresh_token 1 本を崩さないため、ここで表を共通化しておく。
-- keyring 側のキー（gcal:refresh:{email} → google:refresh:{email}）はコードで読み替える。

ALTER TABLE calendar_accounts RENAME TO google_accounts;

-- 許可済みスコープ（スペース区切り）。カレンダーだけで連携済みのアカウントに連絡先を
-- 追加する際、refresh_token に contacts スコープが無いため再同意が要る。その判定材料。
-- NULL は「不明」（0054 以前に連携した旧レコード）＝カレンダーのみと見なす。
ALTER TABLE google_accounts ADD COLUMN granted_scopes TEXT;

-- サービスごとの同期有効フラグ。既存レコードはカレンダー連携済みなので calendar=1。
ALTER TABLE google_accounts ADD COLUMN sync_calendar INTEGER NOT NULL DEFAULT 1;
ALTER TABLE google_accounts ADD COLUMN sync_contacts INTEGER NOT NULL DEFAULT 0;

-- 最終同期時刻はサービス別に持つ。既存 last_sync_at はカレンダーの実績なので引き継ぐ。
ALTER TABLE google_accounts ADD COLUMN last_calendar_sync_at TIMESTAMP;
ALTER TABLE google_accounts ADD COLUMN last_contacts_sync_at TIMESTAMP;
UPDATE google_accounts SET last_calendar_sync_at = last_sync_at;
ALTER TABLE google_accounts DROP COLUMN last_sync_at;
