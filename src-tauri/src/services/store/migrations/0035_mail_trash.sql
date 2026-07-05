-- メールのゴミ箱（trash フォルダ）対応。削除＝ハード削除から「ゴミ箱へ移動」へ。
-- trashed_at: ゴミ箱へ移した時刻（保持期間による自動パージ判定に使う）。
-- prev_folder: 復元先（ゴミ箱へ移す前の folder）。復元時に元のフォルダへ戻す。
ALTER TABLE emails ADD COLUMN trashed_at  TEXT;
ALTER TABLE emails ADD COLUMN prev_folder TEXT;

-- 保持期間パージ（trashed_at で絞る）を軽くするための部分索引。
CREATE INDEX IF NOT EXISTS idx_emails_trashed_at ON emails(trashed_at) WHERE trashed_at IS NOT NULL;
