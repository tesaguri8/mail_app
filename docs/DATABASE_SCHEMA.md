# データベース設計

**ステータス:** 計画（実装未着手）
**出典:** 旧 `README_plan.md` §4。
**実装:** Rust `rusqlite`（`bundled-sqlcipher` + FTS5）。マイグレーションは `src-tauri/src/services/store/` で自前のバージョン管理 SQL として適用する（Alembic は不採用）。

---

## 1. 主要テーブル

```sql
-- アカウント
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    email TEXT NOT NULL,
    display_name TEXT,
    imap_host TEXT NOT NULL,
    imap_port INTEGER DEFAULT 993,
    smtp_host TEXT NOT NULL,
    smtp_port INTEGER DEFAULT 587,
    auth_type TEXT DEFAULT 'password',   -- 'password' | 'oauth2'（将来）
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- メール
CREATE TABLE emails (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    message_id TEXT UNIQUE NOT NULL,
    thread_id TEXT,
    subject TEXT,
    from_address TEXT,
    to_addresses TEXT,
    cc_addresses TEXT,
    bcc_addresses TEXT,
    date TIMESTAMP,
    received_date TIMESTAMP,
    size INTEGER,
    has_attachments BOOLEAN DEFAULT FALSE,
    is_read BOOLEAN DEFAULT FALSE,
    is_flagged BOOLEAN DEFAULT FALSE,
    folder_id INTEGER,
    raw_headers TEXT,
    body_plain TEXT,
    body_html TEXT,
    -- スレッド再構築（docs/THREADING.md）
    clean_body TEXT,                       -- 引用・署名を除去した新規本文（表示・FTS用）
    body_fingerprint TEXT,                 -- clean_body の正規化ハッシュ
    logical_thread_id INTEGER,             -- アプリが再構築した論理スレッド（threads とは別）
    thread_assignment TEXT DEFAULT 'auto', -- 'auto' | 'manual'（手動上書きは保持）
    -- 活用ヘッダ（スレッド化・仕分け・信頼・解析ヒント）
    thread_index TEXT,                     -- Outlook/Exchange Thread-Index
    list_id TEXT,                          -- メルマガ/ML 判定（List-Id）
    delivered_to TEXT,                     -- 受信した自分のアドレス/エイリアス
    auth_result TEXT,                      -- SPF/DKIM/DMARC 認証結果サマリ
    precedence TEXT,                       -- bulk/list 等
    x_mailer TEXT,                         -- 送信クライアント（引用形式の推定に利用）
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
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
    last_activity TIMESTAMP,
    message_count INTEGER DEFAULT 0,
    unread_count INTEGER DEFAULT 0,
    is_user_renamed BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 引用ブロック（1メール内に複数あり得る。属性行から from+時刻を抽出して突合）
CREATE TABLE message_quotes (
    id INTEGER PRIMARY KEY,
    email_id INTEGER NOT NULL,
    block_order INTEGER,           -- 入れ子・並び順
    quoted_from TEXT,              -- 属性行から抽出した差出人
    quoted_at TIMESTAMP,           -- 属性行から抽出した時刻
    fingerprint TEXT,              -- 引用本文の正規化ハッシュ
    matched_email_id INTEGER,      -- 突合できた元メール（任意）
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (email_id) REFERENCES emails(id),
    FOREIGN KEY (matched_email_id) REFERENCES emails(id)
);

-- スレッド
CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    subject TEXT,
    participants TEXT,
    last_activity TIMESTAMP,
    message_count INTEGER DEFAULT 0,
    unread_count INTEGER DEFAULT 0,
    has_attachments BOOLEAN DEFAULT FALSE
);

-- タグ
CREATE TABLE tags (
    id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL,
    color TEXT,
    parent_id INTEGER,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (parent_id) REFERENCES tags(id)
);

-- メール-タグ関連
CREATE TABLE email_tags (
    email_id INTEGER,
    tag_id INTEGER,
    assigned_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (email_id, tag_id),
    FOREIGN KEY (email_id) REFERENCES emails(id),
    FOREIGN KEY (tag_id) REFERENCES tags(id)
);

-- 添付ファイル
CREATE TABLE attachments (
    id INTEGER PRIMARY KEY,
    email_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT,
    size INTEGER,
    file_path TEXT,
    checksum TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (email_id) REFERENCES emails(id)
);

-- 連絡先（住所録）
CREATE TABLE contacts (
    id INTEGER PRIMARY KEY,
    display_name TEXT NOT NULL,
    name_kana TEXT,                 -- 読み（並び替え用）
    email TEXT,                     -- 主メールアドレス
    emails TEXT,                    -- 追加アドレス（JSON）
    phone TEXT,
    organization TEXT,
    address TEXT,
    birthday TEXT,                  -- 誕生日（ホーム/ウィジェット通知用）
    note TEXT,
    avatar_path TEXT,
    is_favorite BOOLEAN DEFAULT FALSE,
    source TEXT DEFAULT 'local',    -- 'local' | 'google' | 'icloud' | ...
    external_id TEXT,               -- 連携元のID（マージ・同期用）
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 連絡先グループ
CREATE TABLE contact_groups (
    id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL,
    color TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 連絡先-グループ関連
CREATE TABLE contact_group_members (
    contact_id INTEGER,
    group_id INTEGER,
    PRIMARY KEY (contact_id, group_id),
    FOREIGN KEY (contact_id) REFERENCES contacts(id),
    FOREIGN KEY (group_id) REFERENCES contact_groups(id)
);

-- カレンダー予定
CREATE TABLE events (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    location TEXT,
    start_at TIMESTAMP NOT NULL,
    end_at TIMESTAMP,
    all_day BOOLEAN DEFAULT FALSE,
    recurrence TEXT,                -- RRULE（iCal 形式）
    reminder_minutes INTEGER,       -- 開始何分前に通知
    color TEXT,
    source TEXT DEFAULT 'local',    -- 'local' | 'ics' | 'google' | 'caldav'
    external_id TEXT,               -- 連携元のID（同期用）
    related_email_id INTEGER,       -- メールから作成した場合の紐付け
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (related_email_id) REFERENCES emails(id)
);

-- 予定の参加者（連絡先と紐付け）
CREATE TABLE event_attendees (
    event_id INTEGER,
    contact_id INTEGER,
    response TEXT DEFAULT 'none',   -- 'accepted' | 'declined' | 'tentative' | 'none'
    PRIMARY KEY (event_id, contact_id),
    FOREIGN KEY (event_id) REFERENCES events(id),
    FOREIGN KEY (contact_id) REFERENCES contacts(id)
);

-- ───────────────────────────────────────────────
-- SNS 統合（メッセージハブ）: ローカルキャッシュ
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

-- ───────────────────────────────────────────────
-- AI 注釈（docs/AI_FEATURES.md）
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

-- 検索インデックス（FTS5）
CREATE VIRTUAL TABLE email_fts USING fts5(
    subject,
    from_address,
    to_addresses,
    clean_body,                 -- 引用除去後の本文を索引（重複ヒットを減らし精度向上）
    content=emails,
    content_rowid=id
);

-- 連絡先の検索インデックス（任意。住所録が大きくなる場合）
CREATE VIRTUAL TABLE contact_fts USING fts5(
    display_name,
    name_kana,
    email,
    organization,
    content=contacts,
    content_rowid=id
);

-- SNS メッセージの全文検索（統合インボックスの横断検索用）
CREATE VIRTUAL TABLE sns_message_fts USING fts5(
    body_text,
    sender_name,
    content=sns_messages,
    content_rowid=id
);
```

> **統合インボックスの一覧**: メール（`emails`/`threads`）と SNS（`sns_conversations`/`sns_messages`）は
> ソース固有テーブルに保持しつつ、ホーム/統合一覧では両者を時刻順にマージして表示する
> （アプリ側で UNION、または将来 `inbox_items` ビューを用意）。横断検索は各 FTS5 を束ねて集約する。

---

## 2. インデックス戦略

```sql
CREATE INDEX idx_emails_thread_id      ON emails(thread_id);
CREATE INDEX idx_emails_date           ON emails(date DESC);
CREATE INDEX idx_emails_from           ON emails(from_address);
CREATE INDEX idx_emails_account_folder ON emails(account_id, folder_id);
CREATE INDEX idx_email_tags_tag_id     ON email_tags(tag_id);

-- スレッド再構築（docs/THREADING.md）
CREATE INDEX idx_emails_logical_thread ON emails(logical_thread_id, date);
CREATE INDEX idx_emails_list_id        ON emails(list_id);
CREATE INDEX idx_quotes_email          ON message_quotes(email_id);
CREATE INDEX idx_quotes_match          ON message_quotes(quoted_from, quoted_at);

-- 住所録・カレンダー
CREATE INDEX idx_contacts_name      ON contacts(name_kana, display_name);
CREATE INDEX idx_contacts_email     ON contacts(email);
CREATE INDEX idx_contacts_birthday  ON contacts(birthday);
CREATE INDEX idx_events_start       ON events(start_at);
CREATE INDEX idx_event_attendees_c  ON event_attendees(contact_id);

-- SNS 統合
CREATE INDEX idx_sns_conv_channel   ON sns_conversations(channel_id, last_activity DESC);
CREATE INDEX idx_sns_conv_contact   ON sns_conversations(contact_id);
CREATE INDEX idx_sns_msg_conv       ON sns_messages(conversation_id, timestamp DESC);

-- AI 注釈
CREATE INDEX idx_ai_annotations_target ON ai_annotations(target_type, target_id, kind);
```

---

## 3. 実装上の注意

- **本文の保存**: 大きな本文・添付はファイルシステムへ退避し、DB には索引・メタデータを保持する設計も検討（[DATA_STORAGE.md](DATA_STORAGE.md) 参照）。
- **FTS5 同期**: `emails` への INSERT/UPDATE/DELETE 時に `email_fts` を更新（トリガまたはアプリ側で明示更新）。差分同期と整合させる。
- **暗号化**: SQLCipher により DB ファイル全体を暗号化。鍵は `keyring`（OS 金庫）で管理。
- **マイグレーション**: `user_version` プラグマ等でスキーマバージョンを管理し、起動時に未適用分を順次適用。
- **JSON の方針**: メール本体はリレーショナル＋FTS5 で保持し、**保存形式として JSON は不要**。JSON を使うのは限定的な役割のみ —— ① AI / IPC へ渡すシリアライズ（serde/ts-rs で自動）、② AI 注釈など可変構造（`ai_annotations.content_json`）、③ 真に可変な少数フィールド（添付一覧・追加アドレス等）、④ エクスポート/バックアップ（JSONL）。リレーショナルの核を JSON で置き換えない（[AI_FEATURES.md](AI_FEATURES.md) §4）。
