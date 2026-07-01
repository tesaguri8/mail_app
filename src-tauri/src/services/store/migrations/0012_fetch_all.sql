-- 「取り込み範囲」の選択を廃止し、常に全期間を取り込む方針に統一する。
-- メールクライアントとして最新が欠けるのは致命的で、範囲を絞る意味が薄いため。
-- ローカル保存量は「添付を手元に残す期間」「テキスト全文を確実に残す期間」＋容量上限で管理する。
-- 既存アカウントも全期間へ（実際の追加取得は次回のフル再取得＝点検再取り込みで反映）。
UPDATE accounts SET sync_window = 'all';

-- body_window は「テキスト全文を確実に残す期間（保証）」に意味を変更。旧既定 'off'
-- （＝要約しない）は、新セマンティクスの 'all'（＝常に全文保証）へ寄せる。
UPDATE accounts SET body_window = 'all' WHERE body_window = 'off' OR body_window IS NULL;
