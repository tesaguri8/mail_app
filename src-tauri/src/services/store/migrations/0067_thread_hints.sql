-- 返信のスレッドの手がかり（docs/THREADING.md §2・docs/COMPOSE.md）。
--
-- 自分の送信メールのうち、Sent の控えに Message-ID が無いもの（0.1.0-alpha.34 まで、通常の送信は
-- Message-ID を付けていなかった）へ返信すると、返信に In-Reply-To を付けられない。References も
-- 祖先が無ければ空になり、ヘッダだけでは親のスレッドへ戻れない。送信のときに、返信の Message-ID と
-- 親の行（canonical_key）をここに残し、スレッドの割当（取り込み・再構築）で親と同じスレッドに載せる。
CREATE TABLE IF NOT EXISTS thread_hints (
  account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  -- 返信の Message-ID（山括弧なし。emails.message_id と同じ形）。
  message_id TEXT NOT NULL,
  -- 親の行の emails.canonical_key（フォルダ接頭辞つき。Message-ID の無い控えでも再同期で変わらない）。
  parent_key TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (account_id, message_id)
);
