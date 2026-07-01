-- 期間ベースの3ティア・ローカル保存（グラデーション）。docs/SYNC.md
-- 最新=フル → 過去に向かって段階的に軽くする。容量上限(storage_limit)は保険として維持。
--
-- full_window: この期間内は「フルデータ」（本文＋添付ファイル）を保持。
--   これより古いメールは添付ファイルをローカルから削除（メタは残し、必要時に再DL）。
--   'all' = 年齢では添付を消さない（既定＝非破壊）。
-- body_window: この期間より古いメールは本文を「要約保存」に落とす。
--   重い body_html_z / body_plain を破棄し、引用除去済みの clean_body だけ残す。
--   引用履歴はスレッドの他メールから再構成、全文はサーバーから再取得できる。
--   'off' = 要約しない（既定＝非破壊）。
-- どちらも既定は非破壊。ユーザーが期間を選んで初めて段階保存が働く。
ALTER TABLE accounts ADD COLUMN full_window TEXT DEFAULT 'all';
ALTER TABLE accounts ADD COLUMN body_window TEXT DEFAULT 'off';

-- 本文を要約保存に落とした印（clean_body のみ残っている）。表示で注記＋全文取得に使う。
ALTER TABLE emails ADD COLUMN body_compacted INTEGER DEFAULT 0;
