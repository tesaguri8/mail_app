-- まだ手元のメールと結び付いていない引用ブロックだけの索引（docs/THREADING.md §2）。
--
-- 新しいメールを取り込んだとき、そのメールを指している「まだ結び付いていない引用」を探す
-- （返信が先に取り込まれ、元のメールが後から届いた場合の逆向きの照合）。結び付いた行は
-- 探す必要が無いので、部分索引にして、探す範囲を未照合の行だけにする。
CREATE INDEX IF NOT EXISTS idx_quotes_unmatched ON message_quotes(email_id)
    WHERE matched_email_id IS NULL AND quoted_at IS NOT NULL;
