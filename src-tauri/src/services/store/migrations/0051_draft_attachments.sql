-- 下書きに紐づく添付（docs/COMPOSE.md §1）。
-- 転送では元メールの添付をそのまま引き継ぐため、書きかけの状態でも添付を覚えておく必要がある
-- （ビュー切替や再起動で下書きから復元したときに添付が黙って消えないように）。
-- path はローカルの実ファイル（ユーザーが選んだ/退避したファイル、または取得済みの添付）。
-- 転送元の添付がまだ手元に無い場合は path を NULL のまま source_attachment_id だけ持ち、
-- 送信時に attachments からサーバー経由で取り直す。
CREATE TABLE IF NOT EXISTS draft_attachments (
    draft_id             INTEGER NOT NULL,  -- emails.id（folder='drafts'）
    ord                  INTEGER NOT NULL,  -- 作成画面での並び順
    path                 TEXT,              -- ローカル実ファイル（未取得なら NULL）
    source_attachment_id INTEGER,           -- 転送元 attachments.id（未取得のときの取り直し用）
    filename             TEXT NOT NULL,
    size                 INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (draft_id, ord),
    FOREIGN KEY (draft_id) REFERENCES emails(id) ON DELETE CASCADE
);
