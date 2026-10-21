-- 差出人の行が無い「>」だけの引用の、返信の相手から推定した差出人（docs/THREADING.md §2）。
--
-- 読めた差出人（quoted_from）と区別するため、別の列に入れる（quoted_from は推定で上書きしない）。
-- 推定した引用は日時が分からない（このメールより前ということだけ）。
ALTER TABLE message_quotes ADD COLUMN guessed_from TEXT;
