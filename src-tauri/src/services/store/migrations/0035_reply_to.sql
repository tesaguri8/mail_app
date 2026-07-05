-- 受信メールの Reply-To（返信先指定）。差出人が返信先を明示している場合、返信の宛先を
-- From ではなく Reply-To にするために保持する（ML・no-reply＋実返信先・代表アドレス等）。
-- "名前 <addr>, ..." の表示用文字列。指定が無ければ NULL。
ALTER TABLE emails ADD COLUMN reply_to TEXT;
