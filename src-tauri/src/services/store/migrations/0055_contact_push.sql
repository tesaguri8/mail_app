-- Google 連絡先の送信（push）。ローカルの変更を Google へ送るための印と、既定の振る舞い。
--
-- カレンダー（0041 の events.dirty）と同じ作法にそろえる: 利用者の操作で dirty=1 を立て、
-- 同期は push → pull の順で走り、送信に成功した時点で dirty を落とす。取り込み（pull）は
-- dirty=1 の連絡先を上書きしない（送信できていないローカルの変更を潰さないため）。

-- 未送信のローカル変更がある（1＝次の同期で Google へ送る）。
ALTER TABLE contacts ADD COLUMN dirty INTEGER NOT NULL DEFAULT 0;

-- 送信対象を素早く引くための部分索引。
CREATE INDEX IF NOT EXISTS idx_contacts_dirty ON contacts(dirty) WHERE dirty = 1;

-- Rondine で新しく作った連絡先（Google 由来でないもの）も Google 側に作るか。
--
-- 既定は 0（作らない）。住所録を Google へ上げるかどうかは利用者が決めることなので、
-- 明示的に有効にしたときだけ送る。有効でない間、ローカル生まれの連絡先は
-- contact_identities を持たないまま手元に留まる。
ALTER TABLE google_accounts ADD COLUMN push_new_contacts INTEGER NOT NULL DEFAULT 0;
