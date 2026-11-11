-- 送信済みの控えに推定して入れた Message-ID の出所（docs/THREADING.md §2）。
--
-- 0.1.0-alpha.34 までの送信は Message-ID を付けておらず、相手に届いた版にだけ中継が ID を付けた。
-- 相手の返信の In-Reply-To（X）が手元のどのメールにも無く、差出人・件名・References の並び（と、
-- 読めれば引用の見出しの日時）が ID の無い控え（C）1 通だけと合うとき、C の emails.message_id に
-- X を入れる。ここにはその出所を残し、取り消せるようにする。
CREATE TABLE IF NOT EXISTS inferred_message_ids (
  account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  -- 控えの emails.canonical_key（フォルダ接頭辞つき。Message-ID を入れても変わらない）。
  canonical_key TEXT NOT NULL,
  -- 'linked': message_id を入れた / 'conflict': 別の X が 2 つ以上来たので結ばない /
  -- 'reverted': 取り消した。'linked' 以外の控えは、以後も推定しない。
  state TEXT NOT NULL CHECK (state IN ('linked', 'conflict', 'reverted')),
  -- 入れた Message-ID（山括弧なし）。conflict では NULL、reverted では取り消す前の値。
  message_id TEXT,
  -- 手がかりにした相手の返信（emails.id）。後で消えても記録は残すので、外部キーにしない。
  source_email_id INTEGER,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (account_id, canonical_key)
);
