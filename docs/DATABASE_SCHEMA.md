# データベース設計

**ステータス:** 一部実装済み（コア＝メール/連絡先/タグ/迷惑/グリーンは稼働。カレンダー/SNS/AI 等は計画）。
**出典:** 旧 `README_plan.md` §4。実装の正は `src-tauri/src/services/store/migrations/*.sql`（本稿は 0001〜0037 の 37 本＝v35 欠番、に整合させて記載）。
**実装:** Rust `rusqlite`（現状は `features = ["bundled"]`＝**素の SQLite** + FTS5）。マイグレーションは `src-tauri/src/services/store/migrations/` の連番 SQL を `migrations.rs` が `PRAGMA user_version` で順次適用する（Alembic は不採用）。

> **暗号化は計画（未導入・後続）**: 本稿で「SQLCipher で暗号化」と記す箇所は**現状未実装**。`Cargo.toml` は `bundled-sqlcipher` ではなく `bundled`（平文 SQLite）を使う。DB 全体暗号化（SQLCipher 化・鍵は keyring）は後続で導入する計画。
>
> **バージョン管理**: スキーマ版は `PRAGMA user_version`。**v35 は意図的な欠番**（dev の「ゴミ箱」枝と feature の「Reply-To」枝が別々に 35 を使って衝突したため、双方を 36 以降へ振り直した。`migrations.rs` の `is_already_applied` で既存列を許容し冪等化）。欠落ではなく既知の意図的スキップ。
>
> **本稿の DDL の読み方**: 下記 `emails` 等は 0001 の初期 DDL に後続マイグレーションの `ALTER TABLE` 追加列を**累積合成**した「現在の実効スキーマ」で示す。実 SQL では列は複数ファイルに分かれて追加される（各列に追加元の migration 番号を注記）。

---

## 1. 主要テーブル

```sql
-- アカウント（0001。後続の ALTER を累積した現在の実効スキーマ）
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    email TEXT NOT NULL,
    display_name TEXT,
    imap_host TEXT NOT NULL,
    imap_port INTEGER DEFAULT 993,
    smtp_host TEXT NOT NULL,
    smtp_port INTEGER DEFAULT 587,
    auth_type TEXT DEFAULT 'password',      -- 'password' | 'oauth2'（将来）
    -- 同期範囲・保持（docs/SYNC.md。0012 で sync_window は全期間 'all' に統一）
    sync_window TEXT DEFAULT '6m',          -- 実際は 0012 で 'all' へ更新
    body_fetch TEXT DEFAULT 'window',
    attachment_fetch TEXT DEFAULT 'on_demand',
    retention TEXT DEFAULT 'window',
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now')),
    -- 後続マイグレーションで追加された列
    username TEXT,                          -- 0002: ログイン名（差出人メールと分離）
    server_account_id INTEGER REFERENCES server_accounts(id),  -- 0003
    uid_validity INTEGER,                   -- 0004（0015 で folder_sync へ移行）
    last_uid INTEGER,                       -- 0004（0015 で folder_sync へ移行）
    signature_id INTEGER REFERENCES signatures(id) ON DELETE SET NULL,  -- 0005
    storage_limit INTEGER DEFAULT 2147483648,  -- 0009: ローカル保存上限（既定 2GB）
    full_window TEXT DEFAULT 'all',         -- 0011: フルデータ（本文＋添付）保持期間
    body_window TEXT DEFAULT 'off',         -- 0011（0012 で 'all' へ）: 全文保証期間
    sort_order INTEGER,                     -- 0023: 並び順
    ingest_version INTEGER NOT NULL DEFAULT 0,  -- 0034: 取り込み形式バージョン
    parse_version  INTEGER NOT NULL DEFAULT 0   -- 0034: ローカル解析バージョン
);

-- メール（0001 の初期 DDL に後続 ALTER を累積した現在の実効スキーマ）
-- 重複排除は message_id ではなく canonical_key で行う（0001。message_id は NULL 可・非 UNIQUE）。
CREATE TABLE emails (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    message_id TEXT,                       -- 0001: NULL 可・UNIQUE ではない（欠落・重複あり得る）
    canonical_key TEXT NOT NULL,           -- 0001: 重複排除キー。UNIQUE は (account_id, canonical_key)
    thread_id TEXT,
    subject TEXT,
    from_address TEXT,
    to_addresses TEXT,
    cc_addresses TEXT,
    date TEXT,                             -- 文字列（rfc3339/ISO8601）。並び替えは date_ts を使う
    received_date TEXT,
    size INTEGER,
    has_attachments INTEGER DEFAULT 0,     -- 真偽は INTEGER（0/1）。BOOLEAN 型は使わない
    is_read INTEGER DEFAULT 0,
    is_flagged INTEGER DEFAULT 0,
    -- フィルタリング用の状態フラグ（docs/FILTERING.md）
    is_bookmarked INTEGER DEFAULT 0,       -- ブックマーク
    needs_review INTEGER DEFAULT 0,        -- 要再確認（フォローアップ）
    follow_up_at TEXT,                     -- 要再確認の期限（任意）
    snooze_until TEXT,                     -- スヌーズ（docs/COMPOSE.md）
    -- 迷惑メール（0001 に spam_score/is_junk、spam_learned は 0013 で追加。docs/SPAM.md）
    spam_score REAL,                       -- 0–1
    is_junk INTEGER DEFAULT 0,             -- 迷惑として隔離
    folder TEXT,                           -- 0001: フォルダ名（'inbox'|'sent'|… ／ folder_id ではない）
    raw_headers TEXT,
    body_plain TEXT,
    body_html TEXT,                        -- 0008 以降は body_html_z（zstd BLOB）へ寄せ、TEXT は NULL 化
    -- スレッド再構築（docs/THREADING.md）
    clean_body TEXT,                       -- 引用・署名を除去した新規本文（表示・FTS用）
    body_fingerprint TEXT,                 -- clean_body の正規化ハッシュ
    logical_thread_id INTEGER,             -- アプリが再構築した論理スレッド（logical_threads）
    thread_assignment TEXT DEFAULT 'auto', -- 'auto' | 'manual'（手動上書きは保持）
    -- 活用ヘッダ（スレッド化・仕分け・信頼・解析ヒント）
    thread_index TEXT,                     -- Outlook/Exchange Thread-Index
    list_id TEXT,                          -- メルマガ/ML 判定（List-Id）
    delivered_to TEXT,                     -- 受信した自分のアドレス/エイリアス
    auth_result TEXT,                      -- SPF/DKIM/DMARC 認証結果サマリ
    created_at TEXT DEFAULT (datetime('now')),
    -- ── 以下は後続マイグレーションで ALTER TABLE 追加された列 ──
    spam_learned INTEGER DEFAULT 0,        -- 0013: 最後に学習した向き（-1=ham / 0=未学習 / 1=spam）
    uid INTEGER,                           -- 0006: IMAP UID（添付等の再取得キー）
    body_html_z BLOB,                      -- 0008: 本文 HTML を zstd 圧縮して保持
    body_compacted INTEGER DEFAULT 0,      -- 0011: 本文を要約保存に落とした印
    from_name TEXT,                        -- 0020: 差出人表示名
    to_name TEXT,                          -- 0020: 宛先表示名
    date_ts INTEGER,                       -- 0022: 並び替え用の epoch 秒
    in_reply_to TEXT,                      -- 0029: 下書き返信の In-Reply-To
    bcc_addresses TEXT,                    -- 0030: 下書きの手入力 Bcc（受信では NULL）
    references_ids TEXT,                   -- 0031: References（祖先 Message-ID 連鎖）
    is_folder_rep INTEGER DEFAULT 1,       -- 0032: (論理スレッド,フォルダ) の代表フラグ
    trashed_at TEXT,                       -- 0036: ゴミ箱へ移した時刻
    prev_folder TEXT,                      -- 0036: 復元先（ゴミ箱移動前の folder）
    reply_to TEXT,                         -- 0037: 受信メールの Reply-To（返信先指定）
    UNIQUE (account_id, canonical_key),
    FOREIGN KEY (account_id) REFERENCES accounts(id),
    FOREIGN KEY (logical_thread_id) REFERENCES logical_threads(id)
);

-- 論理スレッド（アプリが引用解析で再構築する会話単位。ヘッダの threads とは独立）
-- 詳細: docs/THREADING.md
CREATE TABLE logical_threads (
    id INTEGER PRIMARY KEY,
    title TEXT,                     -- アプリ独自タイトル（リネーム可）
    auto_title TEXT,               -- 元の件名（正規化）
    participants TEXT,             -- 参加者（JSON）
    last_activity TEXT,
    message_count INTEGER DEFAULT 0,
    unread_count INTEGER DEFAULT 0,
    is_user_renamed INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now')),
    -- 0031 で追加（ヘッダ由来のルートキーで自動束ね）
    account_id INTEGER,            -- 0031: アカウント単位で束ねる
    root_key TEXT                  -- 0031: auto 割当の束ねキー。手動スレッドは NULL
);
-- auto スレッドは (account_id, root_key) で一意（0031。手動＝root_key NULL は区別される）

-- 引用ブロック（1メール内に複数あり得る。属性行から from+時刻を抽出して突合）
CREATE TABLE message_quotes (
    id INTEGER PRIMARY KEY,
    email_id INTEGER NOT NULL,
    block_order INTEGER,           -- 入れ子・並び順
    quoted_from TEXT,              -- 属性行から抽出した差出人
    quoted_at TEXT,                -- 属性行から抽出した時刻
    fingerprint TEXT,              -- 引用本文の正規化ハッシュ
    matched_email_id INTEGER,      -- 突合できた元メール（任意）
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (email_id) REFERENCES emails(id)
);

-- スレッド（この `threads`(id TEXT) 変種は【計画（マイグレーション未作成・実在しない）】。
-- 実際のスレッド単位は上の logical_threads（id INTEGER）。emails.thread_id は
-- ヘッダ由来の文字列キーとして残っているが、独立した threads テーブルは作られていない。）
-- CREATE TABLE threads (
--     id TEXT PRIMARY KEY,
--     subject TEXT,
--     participants TEXT,
--     last_activity TEXT,
--     message_count INTEGER DEFAULT 0,
--     unread_count INTEGER DEFAULT 0,
--     has_attachments INTEGER DEFAULT 0
-- );

-- タグ / カテゴリ（kind で区別。0001。メール・連絡先で共有＝0019）
CREATE TABLE tags (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    kind TEXT DEFAULT 'tag',        -- 'tag'（複数付与）| 'category'（分類）
    color TEXT,
    parent_id INTEGER,              -- 階層可（FK 明示宣言はなし）
    created_at TEXT DEFAULT (datetime('now'))
);

-- メール-タグ関連（0001。FK 句は宣言されていない。tag_id 単独索引は 0010）
CREATE TABLE email_tags (
    email_id INTEGER,
    tag_id INTEGER,
    assigned_at TEXT DEFAULT (datetime('now')),
    PRIMARY KEY (email_id, tag_id)
);
-- 連絡先-タグ関連（0019。tags を共有）
CREATE TABLE contact_tags (
    contact_id INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (contact_id, tag_id),
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE,
    FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
);

-- 添付ファイル（0001 + 後続 ALTER）
CREATE TABLE attachments (
    id INTEGER PRIMARY KEY,
    email_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT,
    size INTEGER,
    file_path TEXT,
    checksum TEXT,
    created_at TEXT DEFAULT (datetime('now')),
    part_index INTEGER,                            -- 0006: mail-parser の attachment 序数（再取得キー）
    kind TEXT NOT NULL DEFAULT 'attachment',       -- 0007: 'attachment' | 'inline'（cid: 埋め込み）
    content_id TEXT,                               -- 0007: cid: 解決用 Content-ID
    accessed_at TEXT,                              -- 0009: 最終アクセス時刻（LRU エビクション基準）
    FOREIGN KEY (email_id) REFERENCES emails(id)
);

-- 連絡先（住所録。0016 + 後続 ALTER）
CREATE TABLE contacts (
    id INTEGER PRIMARY KEY,
    display_name TEXT NOT NULL,     -- FN（表示名）
    name_kana TEXT,                 -- 読み（並び替え用）
    email TEXT,                     -- 主メールアドレス（複数値は contact_emails へ移行＝0018）
    emails TEXT,                    -- 追加アドレス（JSON。将来用）
    phone TEXT,
    organization TEXT,              -- 文字列（0026 で org_id と同期）
    address TEXT,
    birthday TEXT,                  -- 誕生日（ホーム/ウィジェット通知用）
    note TEXT,
    avatar_path TEXT,
    is_favorite INTEGER NOT NULL DEFAULT 0,
    is_business INTEGER NOT NULL DEFAULT 0,      -- 取引先（docs/FILTERING.md）
    allow_remote_images INTEGER NOT NULL DEFAULT 0,  -- 外部画像許可（docs/MAIL_SECURITY.md）
    source TEXT NOT NULL DEFAULT 'local',        -- 'local' | 'google' | 'icloud' | ...
    external_id TEXT,               -- 連携元のID（マージ・同期用）
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    -- 後続 ALTER
    uid TEXT,                       -- 0017: 安定した正準 ID（UUIDv4。UNIQUE 索引・自動採番トリガあり）
    family_name TEXT,               -- 0018: 姓
    given_name TEXT,                -- 0018: 名
    phonetic_family TEXT,           -- 0018: よみ姓
    phonetic_given TEXT,            -- 0018: よみ名
    org_title TEXT,                 -- 0018: 役職
    org_department TEXT,            -- 0018: 部署
    org_id INTEGER REFERENCES organizations(id) ON DELETE SET NULL,  -- 0026
    deleted_at TEXT                 -- 0027: 論理削除（ゴミ箱）
);

-- 連絡先グループ（0016）。0019 でタグ機構（tags/contact_tags）へ統合され、以後は補助的。
CREATE TABLE contact_groups (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    color TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 連絡先-グループ関連（0016。CASCADE 削除）
CREATE TABLE contact_group_members (
    contact_id INTEGER NOT NULL,
    group_id INTEGER NOT NULL,
    PRIMARY KEY (contact_id, group_id),
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE,
    FOREIGN KEY (group_id) REFERENCES contact_groups(id) ON DELETE CASCADE
);

-- ラベル付き複数メール（0018。0025 で is_shared 追加＝共有代表アドレス）
CREATE TABLE contact_emails (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL,
    label TEXT,                     -- 自宅/職場/カスタム
    value TEXT NOT NULL,
    is_primary INTEGER NOT NULL DEFAULT 0,
    position INTEGER NOT NULL DEFAULT 0,
    is_shared INTEGER NOT NULL DEFAULT 0,   -- 0025: 複数名共有アドレス（info@… 等）
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
);

-- ラベル付き複数電話（0018。0025 で is_shared 追加）
CREATE TABLE contact_phones (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL,
    label TEXT,
    value TEXT NOT NULL,
    is_primary INTEGER NOT NULL DEFAULT 0,
    position INTEGER NOT NULL DEFAULT 0,
    is_shared INTEGER NOT NULL DEFAULT 0,   -- 0025
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
);

-- ラベル付き複数住所（構造化。0018）
CREATE TABLE contact_addresses (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL,
    label TEXT,
    postal TEXT,                    -- 郵便番号
    region TEXT,                    -- 都道府県
    city TEXT,                      -- 市区町村
    street TEXT,                    -- 番地・建物
    extended TEXT,                  -- 補足
    country TEXT,
    is_primary INTEGER NOT NULL DEFAULT 0,
    position INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
);

-- 会社・組織（0026。連絡先は org_id で参照。0027 で deleted_at 追加）
CREATE TABLE organizations (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    name_kana TEXT,
    note TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TEXT                 -- 0027: 論理削除（ゴミ箱）
);

-- カレンダー予定（実装: migrations/0038_calendar.sql / 0039_calendar_meta.sql）
-- 日時は端末ローカルの素の ISO8601 文字列で保持（終日='YYYY-MM-DD' / 時間指定='YYYY-MM-DDTHH:MM'）。
-- ゼロ詰め ISO なら辞書順比較＝時刻順になり、範囲抽出を単純な文字列比較で行える。
-- 0039 で calendar_id（複数カレンダー）/ availability（busy|free）/ visibility（default|public|private）を追加。
CREATE TABLE events (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    location TEXT,
    start_at TEXT NOT NULL,         -- 開始（終日は日付のみ）
    end_at TEXT,                    -- 終了（任意。終日の複数日は最終日を含む）
    all_day BOOLEAN DEFAULT FALSE,
    recurrence TEXT,                -- RRULE（iCal 形式。後続段階）
    reminder_minutes INTEGER,       -- 開始何分前に通知（後続段階）
    color TEXT,
    source TEXT DEFAULT 'local',    -- 'local' | 'ics' | 'google' | 'caldav'
    external_id TEXT,               -- 連携元のID（同期用）
    related_email_id INTEGER,       -- メールから作成した場合の紐付け（後続段階）
    deleted_at TIMESTAMP,           -- 論理削除（ゴミ箱）。非 null＝削除済み（保持期間後に完全削除）
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (related_email_id) REFERENCES emails(id) ON DELETE SET NULL
);

-- 予定の参加者（連絡先/メールで紐付け。0039 で email/name 追加・id 主キー化。UI は管理・表示）
CREATE TABLE event_attendees (
    event_id INTEGER,
    contact_id INTEGER,
    response TEXT DEFAULT 'none',   -- 'accepted' | 'declined' | 'tentative' | 'none'
    PRIMARY KEY (event_id, contact_id),
    FOREIGN KEY (event_id) REFERENCES events(id) ON DELETE CASCADE,
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
);

-- ───────────────────────────────────────────────
-- SNS 統合（メッセージハブ）: ローカルキャッシュ
-- 【計画（マイグレーション未作成・未実装）】以下 channels / sns_conversations / sns_messages は設計案。
-- 正規化済みメッセージを中継サービスから受信して保持する。
-- 詳細方針は docs/SNS_INTEGRATION.md を参照。
-- ───────────────────────────────────────────────

-- 接続チャネル（プラットフォームのアカウント単位）
CREATE TABLE channels (
    id INTEGER PRIMARY KEY,
    platform TEXT NOT NULL,         -- 'line' | 'instagram' | 'messenger' | 'whatsapp'
    display_name TEXT,              -- 表示名（例: ふくぎリビング公式LINE）
    external_account_id TEXT,       -- プラットフォーム側のアカウントID
    is_enabled BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    -- 注: アクセストークン等の機密は端末DBに保存しない（中継サービス側で集中管理）
);

-- 会話（DM スレッド / コメントの投稿単位）
CREATE TABLE sns_conversations (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL,
    kind TEXT DEFAULT 'dm',         -- 'dm' | 'comment'
    conversation_key TEXT NOT NULL, -- チャネル内の会話識別子（相手ユーザーID / 投稿ID 等）
    title TEXT,                     -- 相手名や投稿の要約
    contact_id INTEGER,             -- 住所録との突き合わせ（任意）
    last_activity TIMESTAMP,
    unread_count INTEGER DEFAULT 0,
    status TEXT DEFAULT 'open',     -- 'open' | 'handled'
    UNIQUE (channel_id, conversation_key),
    FOREIGN KEY (channel_id) REFERENCES channels(id),
    FOREIGN KEY (contact_id) REFERENCES contacts(id)
);

-- 正規化メッセージ
CREATE TABLE sns_messages (
    id INTEGER PRIMARY KEY,
    conversation_id INTEGER NOT NULL,
    external_message_id TEXT,       -- プラットフォームのメッセージID
    direction TEXT NOT NULL,        -- 'inbound' | 'outbound'
    sender_name TEXT,
    sender_handle TEXT,
    body_text TEXT,
    attachments TEXT,               -- JSON（type/url_or_ref）
    timestamp TIMESTAMP,
    status TEXT DEFAULT 'unread',   -- 'unread' | 'read' | 'replied'
    raw_ref TEXT,                   -- 中継側の元ペイロード参照（監査・再処理用）
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (conversation_id, external_message_id),
    FOREIGN KEY (conversation_id) REFERENCES sns_conversations(id)
);

-- 背景画像（ホーム/ウィジェットの全面ビジュアル。アプリ同梱＋ユーザー取り込み）
-- 【計画（マイグレーション未作成・未実装）】以下は設計案の DDL。
-- 表示モード（fixed/time/daily/season/random）はアプリ設定(tauri-plugin-store)で保持。
CREATE TABLE background_images (
    id INTEGER PRIMARY KEY,
    source TEXT NOT NULL,           -- 'app'（同梱）| 'user'（取り込み）
    file_path TEXT,                 -- user: media/backgrounds/ のコピー先
    resource_key TEXT,              -- app: 同梱リソース識別子
    thumbnail_path TEXT,            -- cache/thumbnails/
    width INTEGER,
    height INTEGER,
    time_of_day TEXT,               -- 任意: 'morning'|'afternoon'|'evening'|'night'
    season TEXT,                    -- 任意: 'spring'|'summer'|'autumn'|'winter'
    in_rotation BOOLEAN DEFAULT TRUE,  -- ローテーション対象に含めるか
    is_active BOOLEAN DEFAULT FALSE,   -- mode='fixed' で選択中
    added_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 署名（0005。実スキーマは id/name/body のみ。account_id 側に既定署名 signature_id を持つ）
CREATE TABLE signatures (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    created_at TEXT DEFAULT (datetime('now'))
);
-- 既定署名の紐付けは accounts.signature_id（0005。ON DELETE SET NULL）で持つ。

-- テンプレート（定型文。docs/COMPOSE.md）【計画（マイグレーション未作成・未実装）】以下は設計案。
CREATE TABLE templates (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    subject TEXT,
    body TEXT,            -- Markdown
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 迷惑メールのローカル学習（0013_spam.sql。docs/SPAM.md）。名前空間付きトークンで衝突回避。
CREATE TABLE spam_tokens (
    token TEXT PRIMARY KEY,             -- 名前空間付き: "w:無料" / "ng:振込" / "url:example.com" / "hdr:spf_fail" ...
    spam_count INTEGER DEFAULT 0,       -- 同一メール内の重複は dedup 後に1カウント
    ham_count INTEGER DEFAULT 0,
    updated_at INTEGER DEFAULT 0        -- epoch秒。古い語の刈り込み（vacuum）判断に使用
);

-- 迷惑メール学習のメタ（総数カウンタ等。0013_spam.sql）。1行 key-value でスキーマ追加に強くする（docs/SPAM.md §4.2）
CREATE TABLE spam_meta (
    key TEXT PRIMARY KEY,              -- "n_spam" / "n_ham" / "model_version" ...
    value INTEGER NOT NULL             -- スコア計算（ラプラス平滑化）に必須の学習メール総数
);

-- 保存フィルタ（スマートフォルダ。0001。docs/FILTERING.md）
-- 【テーブルのみ・コマンド未配線】0001 で作成済みだが、保存フィルタを読み書きする Tauri
-- コマンドはまだ無い（UI 未配線）。条件は可変構造のため JSON で保持する設計。
CREATE TABLE saved_filters (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    definition_json TEXT NOT NULL,  -- 例: {"all":[{"needs_review":true},{"is_read":false}]}
    is_pinned INTEGER DEFAULT 0,
    sort_order INTEGER,
    created_at TEXT DEFAULT (datetime('now'))
);

-- ───────────────────────────────────────────────
-- 実装済みだが上に載っていなかったテーブル（0003/0004/0014/0015/0028）
-- ───────────────────────────────────────────────

-- メールサーバー接続設定を正規化（0003）。複数アカウントで共有・再利用（accounts.server_account_id で参照）。
CREATE TABLE server_accounts (
    id INTEGER PRIMARY KEY,
    name TEXT,
    imap_host TEXT NOT NULL,
    imap_port INTEGER NOT NULL DEFAULT 993,
    imap_security TEXT DEFAULT 'ssl',
    smtp_host TEXT NOT NULL,
    smtp_port INTEGER NOT NULL DEFAULT 587,
    smtp_security TEXT DEFAULT 'starttls',
    username TEXT NOT NULL,
    created_at TEXT DEFAULT (datetime('now')),
    UNIQUE (imap_host, imap_port, username)
);

-- 差分同期の状態（0004 では accounts に uid_validity/last_uid を追加。0015 でフォルダ単位へ移行）。
-- フォルダごとの同期状態（0015）。
CREATE TABLE folder_sync (
    account_id INTEGER NOT NULL,
    folder TEXT NOT NULL,          -- 'inbox' | 'sent' | 'drafts' | 'trash' | 'spam'
    uid_validity INTEGER,
    last_uid INTEGER,
    PRIMARY KEY (account_id, folder),
    FOREIGN KEY (account_id) REFERENCES accounts(id)
);

-- 破棄済み下書きの墓標（0050。docs/COMPOSE.md §1）。送信・破棄でローカルから消した下書きの
-- canonical_key を残し、サーバー Drafts に残ったコピーからの「復活」を取り込み側で弾く。
-- remote_pending=1 はサーバー側コピーの削除が未完了＝同期のたびに削除を再試行する印。
CREATE TABLE deleted_keys (
    account_id     INTEGER NOT NULL,
    canonical_key  TEXT NOT NULL,  -- emails.canonical_key（例 'drafts:draft-1-...@example.com'）
    message_id     TEXT,           -- サーバー削除に使う Message-ID の中身（山括弧なし）
    folder         TEXT NOT NULL,  -- 消したときのフォルダ（現状 'drafts' のみ）
    remote_pending INTEGER NOT NULL DEFAULT 0,
    deleted_at     INTEGER NOT NULL, -- Unix 秒。サーバー削除済みのものは 30 日で掃除。
    PRIMARY KEY (account_id, canonical_key)
);

-- アプリ設定の汎用 key-value（0014。docs/SPAM.md §9）。非機密設定の単一ソース（資格情報は keyring）。
CREATE TABLE app_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- グリーンドメイン／警告ドメイン（0028。docs/GREEN_DOMAINS.md）。
CREATE TABLE green_domains (
    domain TEXT PRIMARY KEY,
    note TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE warning_domains (        -- グリーンから意図的に外したドメイン（自動再グリーン化を防ぐ）
    domain TEXT PRIMARY KEY,
    note TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ───────────────────────────────────────────────
-- AI 注釈（docs/AI_FEATURES.md）【計画（マイグレーション未作成・未実装）】以下は設計案。
-- メール本体はリレーショナルで保持（JSON 不要）。AI 生成物のみ可変構造のため JSON 列に格納。
-- ───────────────────────────────────────────────
CREATE TABLE ai_annotations (
    id INTEGER PRIMARY KEY,
    target_type TEXT NOT NULL,      -- 'email' | 'thread'
    target_id INTEGER NOT NULL,
    kind TEXT NOT NULL,             -- 'summary' | 'subject_suggest' | 'reply_suggest' | 'category'
    content_json TEXT,              -- 生成物（可変構造のため JSON）
    model TEXT,                     -- 使用モデル（監査・再現用）
    is_local BOOLEAN DEFAULT FALSE, -- ローカル(Ollama)生成か
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 検索インデックス（FTS5。0001）。
-- 実装は「外部コンテンツ（content=/content_rowid）」ではなく素の fts5。rowid=emails.id を
-- アプリ側が明示 INSERT/UPDATE/DELETE して同期する運用（列は subject/from_address/clean_body の 3 つ）。
CREATE VIRTUAL TABLE email_fts USING fts5(subject, from_address, clean_body);

-- 連絡先の検索インデックス【計画（未作成）】。住所録が大きくなった場合に追加予定。
-- CREATE VIRTUAL TABLE contact_fts USING fts5(display_name, name_kana, email, organization);

-- SNS メッセージの全文検索【計画（未作成）】。SNS 統合（メッセージハブ）実装時に追加予定。
-- CREATE VIRTUAL TABLE sns_message_fts USING fts5(body_text, sender_name);
```

> **統合インボックスの一覧**: メール（`emails`/`threads`）と SNS（`sns_conversations`/`sns_messages`）は
> ソース固有テーブルに保持しつつ、ホーム/統合一覧では両者を時刻順にマージして表示する
> （アプリ側で UNION、または将来 `inbox_items` ビューを用意）。横断検索は各 FTS5 を束ねて集約する。

---

## 2. インデックス戦略

実装済み（真偽の部分索引は `= 1`。日付順は関数索引が効かないため epoch 秒の `date_ts` を使う）:

```sql
-- メール（0001 / 0022 / 0024 / 0031〜0033）
CREATE INDEX idx_emails_account        ON emails(account_id, date);                  -- 0001
CREATE INDEX idx_emails_logical_thread ON emails(logical_thread_id, date);           -- 0001
CREATE INDEX idx_emails_from           ON emails(from_address);                      -- 0001
CREATE INDEX idx_emails_list_id        ON emails(list_id);                           -- 0001
CREATE INDEX idx_emails_bookmarked     ON emails(is_bookmarked) WHERE is_bookmarked = 1;  -- 0001
CREATE INDEX idx_emails_review         ON emails(needs_review, follow_up_at) WHERE needs_review = 1;  -- 0001
CREATE INDEX idx_quotes_match          ON message_quotes(quoted_from, quoted_at);    -- 0001
CREATE INDEX idx_emails_junk           ON emails(is_junk) WHERE is_junk = 1;         -- 0013
CREATE INDEX idx_emails_folder         ON emails(account_id, folder, date);          -- 0015
CREATE INDEX idx_emails_message_id     ON emails(message_id);                        -- 0031
CREATE INDEX idx_emails_thread_key     ON emails(thread_id);                         -- 0031
CREATE INDEX idx_emails_list           ON emails(account_id, folder, date_ts DESC, id DESC);  -- 0022（一覧）
CREATE INDEX idx_emails_folder_date    ON emails(folder, date_ts DESC, id DESC);     -- 0024（全アカウント横断）
CREATE INDEX idx_emails_folder_rep     ON emails(folder, date_ts) WHERE is_folder_rep = 1;   -- 0032（スレッド代表）
CREATE INDEX idx_emails_thread_folder  ON emails(logical_thread_id, folder);         -- 0033
CREATE INDEX idx_emails_trashed_at     ON emails(trashed_at) WHERE trashed_at IS NOT NULL;   -- 0036
CREATE UNIQUE INDEX idx_threads_root   ON logical_threads(account_id, root_key);     -- 0031

-- タグ（0010）
CREATE INDEX idx_email_tags_tag        ON email_tags(tag_id);                        -- 0010（名前は _tag。_tag_id ではない）
CREATE INDEX idx_contact_tags_tag      ON contact_tags(tag_id);                      -- 0019

-- 添付（0006）
CREATE INDEX idx_attachments_email     ON attachments(email_id);                     -- 0006

-- 住所録・組織（0016 / 0018 / 0021 / 0026 / 0027）
CREATE INDEX idx_contacts_name         ON contacts(name_kana, display_name);         -- 0016
CREATE INDEX idx_contacts_email        ON contacts(email);                           -- 0016
CREATE INDEX idx_contacts_birthday     ON contacts(birthday);                        -- 0016
CREATE INDEX idx_contacts_business     ON contacts(is_business) WHERE is_business = 1;-- 0016
CREATE UNIQUE INDEX idx_contacts_uid   ON contacts(uid);                             -- 0017
CREATE INDEX idx_contacts_email_lower  ON contacts(lower(email));                    -- 0021
CREATE INDEX idx_contact_emails_value_lower ON contact_emails(lower(value));         -- 0021
CREATE INDEX idx_contacts_org_id       ON contacts(org_id);                          -- 0026
CREATE INDEX idx_contacts_deleted_at   ON contacts(deleted_at);                      -- 0027
-- （contact_emails/phones/addresses の cid・値索引は 0018、org 論理削除索引は 0027 も参照）
```

【計画（マイグレーション未作成）】カレンダー・SNS・AI 注釈の索引は対応テーブルと同時に追加予定:

```sql
-- CREATE INDEX idx_events_start       ON events(start_at);
-- CREATE INDEX idx_event_attendees_c  ON event_attendees(contact_id);
-- CREATE INDEX idx_sns_conv_channel   ON sns_conversations(channel_id, last_activity DESC);
-- CREATE INDEX idx_sns_conv_contact   ON sns_conversations(contact_id);
-- CREATE INDEX idx_sns_msg_conv       ON sns_messages(conversation_id, timestamp DESC);
-- CREATE INDEX idx_ai_annotations_target ON ai_annotations(target_type, target_id, kind);
```

---

## 3. 実装上の注意

- **本文の保存**: 大きな本文（HTML）は 0008 以降 `body_html_z`（zstd 圧縮 BLOB）に保持。添付はファイルシステムへ退避し、容量上限で LRU エビクション（[DATA_STORAGE.md](DATA_STORAGE.md) 参照）。
- **FTS5 同期**: `email_fts` は外部コンテンツではなく素の fts5。`emails` への INSERT/UPDATE/DELETE 時に**アプリ側が `rowid=emails.id` で明示同期**する（トリガは使っていない）。差分同期と整合させる。
- **暗号化**: 【計画（未導入・後続）】現状は素の SQLite（`bundled`）で DB は平文。将来 SQLCipher で DB ファイル全体を暗号化し、鍵は `keyring`（OS 金庫）で管理する予定。
- **マイグレーション**: `PRAGMA user_version` でスキーマバージョンを管理し、起動時に未適用分を順次適用（`migrations.rs`）。**v35 は意図的な欠番**（別枝衝突。冒頭注記参照）。既存列と重複する ALTER は `is_already_applied` で許容してバージョンだけ進める。
- **JSON の方針**: メール本体はリレーショナル＋FTS5 で保持し、**保存形式として JSON は不要**。JSON を使うのは限定的な役割のみ —— ① AI / IPC へ渡すシリアライズ（serde/ts-rs で自動）、② AI 注釈など可変構造（`ai_annotations.content_json`）、③ 真に可変な少数フィールド（添付一覧・追加アドレス等）、④ エクスポート/バックアップ（JSONL）。リレーショナルの核を JSON で置き換えない（[AI_FEATURES.md](AI_FEATURES.md) §4）。
