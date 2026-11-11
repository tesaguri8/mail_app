-- 会話の References / In-Reply-To にあって手元に無いメールを、サーバーへ探しに行った結果
-- （docs/THREADING.md §5「References で確かめ、欠けたメールを取りに行く」）。
--
-- 探すのは会話を開いたときに 1 回だけ。見つかったものは取り込み（state='found'）、見つからなければ
-- 'absent' と探した時刻を残す。'absent' は 30 日後に 1 回だけ探し直し（retried=1）、それでも無ければ
-- 二度と探さない。行が無い ID は「まだ探していない」。
CREATE TABLE IF NOT EXISTS ref_lookups (
  account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  -- 探した Message-ID（山括弧なし。emails.message_id と同じ形）。
  message_id TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('found', 'absent')),
  -- 見つかったフォルダのローカルタグ（inbox / sent / archive）。absent では NULL。
  folder TEXT,
  -- 探した時刻（UNIX 秒）。
  checked_at INTEGER NOT NULL,
  -- absent のあと、30 日後の探し直しを済ませたか。
  retried INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (account_id, message_id)
);
