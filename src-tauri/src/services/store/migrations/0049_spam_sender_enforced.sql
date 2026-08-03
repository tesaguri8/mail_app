-- 迷惑差出人の「このまま迷惑（強制適用）」フラグ（docs/SPAM.md §8.5）。
-- 住所録に居るなどの信頼シグナルと矛盾する差出人をユーザーに知らせた上で、
-- 「このまま迷惑」を選んだ差出人は enforced=1 とし、信頼シグナルより迷惑登録を優先する
-- （＝以後も自動隔離し、注意喚起にも再掲しない）。
ALTER TABLE spam_senders ADD COLUMN enforced INTEGER NOT NULL DEFAULT 0;
