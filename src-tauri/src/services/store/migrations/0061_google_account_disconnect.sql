-- Google 連携の「解除中」。
--
-- 解除で google_accounts の行を消すと、それを指す contact_identities（連絡先のつながり）・
-- calendars・events が宙に浮き、再接続で id が変わると古いつながりは孤立する（id は
-- INTEGER PRIMARY KEY で振り直されうる）。解除は既定で「解除中」にとどめ、行もつながりも
-- 未送信の変更（dirty）も残す。同じアカウントで連携し直すと印を消して使い直す。
-- 記録ごと消すのは「完全に解除」（store::purge_google_account）。
ALTER TABLE google_accounts ADD COLUMN disconnected_at TIMESTAMP;
