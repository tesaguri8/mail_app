-- スレッド再構築の実データ化（docs/THREADING.md）。
-- 0001 で用意済みの logical_threads / message_quotes / emails のスレッド列を「実際に使う」ための追補。

-- References ヘッダ（祖先 Message-ID の連鎖。空白区切り・山括弧なし・古い順）。
-- 送信時の References チェーン積み直しと、受信の論理スレッド束ねに使う。
ALTER TABLE emails ADD COLUMN references_ids TEXT;

-- 論理スレッドをアカウント単位＋ヘッダ由来のルートキーで束ねる。
-- root_key: auto 割当の束ねキー（"mid:<root-message-id>" か "subj:<正規化件名>#<相手>"）。
--           手動で作った/切り出したスレッドは NULL（再解析で動かさない目印）。
ALTER TABLE logical_threads ADD COLUMN account_id INTEGER;
ALTER TABLE logical_threads ADD COLUMN root_key TEXT;

-- 割当・束ねの検索を速くする索引。
CREATE INDEX IF NOT EXISTS idx_emails_message_id ON emails(message_id);
CREATE INDEX IF NOT EXISTS idx_emails_thread_key ON emails(thread_id);
-- auto スレッドは (account, root_key) で一意。手動スレッドは root_key=NULL（SQLite は NULL を区別）。
CREATE UNIQUE INDEX IF NOT EXISTS idx_threads_root ON logical_threads(account_id, root_key);
