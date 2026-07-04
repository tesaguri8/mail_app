-- スレッド一覧を「全体走査」せず索引で引くための代表フラグ（docs/THREADING.md §5）。
-- 各 (論理スレッド, フォルダ) の最新メールだけ is_folder_rep=1。未割当(NULL)は各自 1
-- （＝従来のフラット一覧と同じ挙動）。取り込み時に 1 通ずつ立て直すので、一覧は
-- 「WHERE folder=? AND is_folder_rep=1 ORDER BY date_ts DESC LIMIT N」で即引ける。
ALTER TABLE emails ADD COLUMN is_folder_rep INTEGER DEFAULT 1;

-- 既存データ: 割当済みスレッドは各 (スレッド, フォルダ) の最新以外を 0 にする。
-- （未割当 NULL はそのまま各自 1。）
UPDATE emails SET is_folder_rep = 0
WHERE logical_thread_id IS NOT NULL
  AND id NOT IN (
    SELECT id FROM (
      SELECT id, ROW_NUMBER() OVER (
        PARTITION BY logical_thread_id, folder ORDER BY date_ts DESC, id DESC) AS rn
      FROM emails WHERE logical_thread_id IS NOT NULL
    ) WHERE rn = 1
  );

-- 代表だけの部分索引（フォルダ内・日付順）。一覧はこれで先頭 N 件を索引で即引く。
CREATE INDEX IF NOT EXISTS idx_emails_folder_rep
  ON emails(folder, date_ts) WHERE is_folder_rep = 1;
