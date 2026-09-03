-- 「自分から送ったことがある相手」の索引（docs/FILTERING.md §2）。
-- 一覧の「返信歴あり」フィルタで差出人を O(1) で判定するために、送信済み(sent)メールの
-- To/Cc に現れたアドレスを小文字で 1 行ずつ持つ。emails.to_addresses を毎回 LIKE で
-- 舐めると一覧クエリが全走査になるため、索引テーブルとして切り出す。
--
-- 中身の投入は Rust 側（services/store/sent_addresses.rs）で行う。ヘッダのアドレス列
-- （"名前 <a@b>, c@d"）の分解は SQL では書けないため、既存分の再構築も Rust に任せる。
-- 履歴なので、メールをゴミ箱へ移しても行は消さない（送った事実は変わらない）。
CREATE TABLE IF NOT EXISTS sent_addresses (
    -- 小文字化した素のメールアドレス。
    address TEXT PRIMARY KEY,
    -- そのアドレス宛に送った通数（参考値。並べ替えの補助に使う）。
    sent_count INTEGER NOT NULL DEFAULT 0,
    -- 最後に送った日時（epoch 秒。emails.date_ts と同じ基準）。
    last_sent_ts INTEGER
);
