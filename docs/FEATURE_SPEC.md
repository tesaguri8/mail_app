# 機能仕様

**ステータス:** 計画（実装未着手）
**出典:** 旧 `README_plan.md` §1, §3, §5〜§10 を整理。スタック依存部分（REST API / Pydantic / safeStorage 等）は新スタック（Tauri + Rust）に合わせて反映。

関連: [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md)（スタック・フェーズ）/ [DATABASE_SCHEMA.md](DATABASE_SCHEMA.md) / [UI_UX_DESIGN.md](UI_UX_DESIGN.md) / [DATA_STORAGE.md](DATA_STORAGE.md) / [I18N.md](I18N.md)

---

## 1. プロジェクト概要

### ビジョン
- **目的**: 既存のメールクライアントにない、モダンで直感的なユーザー体験を提供
- **差別化要素**: チャット形式の会話ビュー、高速検索、スマートな振り分け
- **ターゲットユーザー**: 効率的なメール管理を求める個人ユーザー

### 主要機能
1. チャット形式のメール表示
2. 高速メタデータ検索
3. スマートタグ・フィルタリング（ブックマーク/要再確認/知り合い/取引実績/グループ/カテゴリ）
4. マルチアカウント対応
5. **住所録（アドレス帳）** — メールアプリの正式スコープとして実装
6. **カレンダー** — 予定管理。メール・連絡先と連携
7. **AI 活用** — 件名/本文生成・スレッド要約・返信提案・分類（オプトイン／クラウド既定／ローカル選択可）
8. **SNS 統合（メッセージハブ）** — LINE / Instagram / Messenger / WhatsApp の DM・コメントを統合インボックスに集約

> 本アプリは「メール / 住所録 / カレンダー / SNS」を束ねる**メッセージハブ**を目指す。
> 複数 SNS のやり取りを 1 つのチャット形式インボックスに集約し、取りこぼしを防ぐ（特に宿泊施設の問い合わせ対応）。
> ホームの常駐・ウィジェット化を含む UI 方針は [UI_UX_DESIGN.md](UI_UX_DESIGN.md)、SNS 統合の詳細は [SNS_INTEGRATION.md](SNS_INTEGRATION.md) を参照。

---

## 2. 詳細機能仕様

### 2.1 メール表示機能

#### チャット形式ビュー
- 同一スレッドのメールをチャット風に表示
- 送信者ごとにメッセージをグループ化
- タイムスタンプの自動グループ化（5分以内）
- インライン返信機能
- 既読/未読の視覚的表現
- **引用を解析して剥がし、新しく書かれた部分だけ**をバブル表示（長い引用のストレスを解消）
- 実装: 仮想スクロール（大量メール対応）、リアルタイム更新、アニメーション付き追加

#### スレッド再構築・アプリ内整理（コア機能 → [THREADING.md](THREADING.md)）
- **独自スレッド再構築**: ヘッダ（`Message-ID`/`References`/`Thread-Index`）＋**引用解析（差出人+時刻・引用本文）**の多層シグナルで会話を再構築。
- **同件名・別内容の自動分割**: 「返信で別件」を検出して別スレッドに分割（**自動＋手動上書き**）。
- **アプリ内で再件名**: 論理スレッドに自分用のタイトルを付け直し、整理しやすくする（元件名は保持）。
- **ヘッダメタデータの活用**: `List-Id`（メルマガ/ML 判定）・`Delivered-To`（宛先エイリアス）・`Authentication-Results`（なりすまし検知）・`X-Mailer`（引用形式推定）等を仕分け・信頼表示・解析に利用。

#### 従来形式ビュー
- 標準的なメールスレッド表示
- 折りたたみ可能な返信履歴
- クイックプレビュー
- 一括操作対応

#### メール作成モード（返信 / 新規）
メールを開いた状態から、用途に応じて 2 つの作成手段を提供する。

- **返信**: スレッドを引き継ぐ（`In-Reply-To` / `References` 付き）。
- **このアドレスへ新規メール**: 同じ相手宛に、**新しい `Message-ID`・参照ヘッダなし・新件名**の**別案件**として作成（＝新しい論理スレッド）。返信での“別件送信”を避け、相手のスレッドを汚さない。
- 新規メールは件名が空になりがちなため、**AI による件名生成**（[AI_FEATURES.md](AI_FEATURES.md)）を導線に置く。

### 2.2 検索システム

**検索対象**: 件名・本文・送信者・受信者 / 添付ファイル名 / タグ・フラグ / 日付範囲 / サイズ

**検索方式**: 全文検索（FTS5）/ ファセット検索 / 自然言語クエリ（将来）/ 検索履歴とサジェスト

**インデックス戦略**: 非同期インデックス作成 / 差分更新 / 定期的な最適化 / メモリ効率的な実装

**同期範囲との関係**: ローカル検索は同期ウィンドウ内（or メタデータ保持分）。ウィンドウ外はサーバー検索（IMAP SEARCH）を任意で実行（[SYNC.md](SYNC.md)）。

### 2.3 タグ・振り分け・フィルタリング（[FILTERING.md](FILTERING.md)）

**タグ/カテゴリ**: 手動タグ付け / 自動タグ付けルール / 階層構造 / カラー / カテゴリ（`kind='category'`、AI 自動分類可）

**状態フラグ**: ブックマーク / 要再確認（フォローアップ・期限）/ フラグ / 未読・既読

**相手で絞り込み**: 知り合い（住所録に存在）/ 取引実績（双方向履歴の自動判定＋「取引先」手動フラグ）/ お気に入り・VIP / アドレスグループ

**種別で絞り込み**: メルマガ/一括（`List-Id`/`Precedence`）/ 宛先エイリアス（`Delivered-To`）/（将来）SNS チャネル

**ルールエンジン条件**: 送信者 / 件名パターン / 本文キーワード / `List-Id` / 添付有無 / 時間帯 → フラグ・カテゴリ・タグを自動付与

**保存フィルタ（スマートフォルダ）**: ファセットの AND/OR を名前付き保存（例「要対応」「常連ゲスト」「メルマガ」）。動的評価。サイドバーに固定。

### 2.4 住所録（アドレス帳）機能

**連携対象**: ローカルアドレス帳（基本）/ Google Contacts（OAuth2）/ iCloud Contacts（macOS）/ LDAP（企業向け・将来）

**機能詳細**: 自動補完 / 連絡先のマージ / グループ管理 / 最近の連絡先 / お気に入り / 誕生日・記念日（ホーム/ウィジェットに通知）

**メール・カレンダー連携**: 連絡先からワンクリックでメール作成 / 連絡先に紐づく予定の表示 / 受信メールの差出人を連絡先へ登録

### 2.5 カレンダー機能

**ビュー**: 月 / 週 / 日 / 予定一覧。ホームには「次の予定」、ウィジェットには直近予定を表示。

**予定管理**: 作成・編集・削除 / 終日・繰り返し予定 / リマインダー（通知）/ カラー分類（タグと共通の色体系）

**連携**:
- **メール**: 招待メール（iCal/.ics）の取り込み・出欠、メールから予定化
- **住所録**: 参加者を連絡先から選択、誕生日・記念日の自動表示
- **外部カレンダー（将来）**: Google Calendar（OAuth2）/ CalDAV 同期

**ローカル方針**: まずローカル予定 + .ics 取り込みに対応し、外部カレンダー双方向同期は後続フェーズ。

### 2.6 SNS 統合（メッセージハブ）※後続ステップ

> **着手は後続。** まずコア機能（メール＋住所録＋カレンダー）を安定させてから実装する。

複数 SNS の DM・コメントを共通スキーマに正規化し、メールと同じチャット形式の**統合インボックス**で一元管理する。詳細は [SNS_INTEGRATION.md](SNS_INTEGRATION.md)。

**対応チャネル（初期）**: LINE 公式アカウント / Instagram（DM + コメント）/ Facebook Messenger（DM + コメント）/ WhatsApp Business

**主な機能**: 統合インボックス（チャネル横断一覧）/ 未読・対応状態管理 / 各チャネルへの返信 / 新着通知（重要キーワード強調）/ 全チャネル横断検索（FTS5）/ ホーム・ウィジェットへの合算未読表示

**アーキテクチャ要点**: SNS API は Webhook 型のため**クラウド中継サービス**（受信・正規化・配信、トークンはサーバー保管）を前提とする。メール本体はローカル完結を維持。tesaguri-tech バックエンド基盤に相乗り。

**スコープ外**: TikTok の DM（公開 API なし）/ Airbnb 個人ホスト窓口（公開 API なし）/ X DM（有料・高コストで将来判断）。**公式 API のみ使用**（スクレイピングは行わない）。

### 2.7 ホーム背景画像（取り込み・選択）

ホーム/ウィジェットの全面ビジュアル背景を、**アプリ同梱画像**と**ユーザー取り込み画像**から選べる（UI 詳細は [UI_UX_DESIGN.md](UI_UX_DESIGN.md) 背景写真システム）。

- **インポート**: ファイル選択 → 形式/サイズ検証（jpg/png/webp）→ アプリ保存領域（`media/backgrounds/`）へコピー → サムネ生成 → DB 登録。
- **選択**: ギャラリーから固定表示、またはローテーション対象に追加。
- **既定の表示**: **自動ローテーション（時間帯／日替わり）**。設定で固定/時間/日替わり/季節/ランダムへ切替可。
- **保存先**: `%APPDATA%\tesaguri.comfortmail.dev\media\backgrounds\`（[DATA_STORAGE.md](DATA_STORAGE.md)）。

### 2.8 保護領域（プライバシー伏字）※提案型 [PROTECTED_REGIONS.md](PROTECTED_REGIONS.md)

機密部分を本文では伏字（`/////name/////`）で送り、実値は**どのメールクライアントでも開ける暗証番号付きPDF**（AES-256）で届ける。相手側の**不用意なクラウドAI処理にプライバシーデータを自動で渡さない**ための仕組み。

- **核ルール**: 保護領域は「人には実値・AIには伏字」。Comfort Mail の全AI機能はAIへ渡す前に伏字へ置換。
- **表示トグル（伏字⇄実値）**: 本文を伏字／実値で切替。**送信前に「相手・AIに渡る姿（伏字）」を目視確認**でき、漏れなく安心して送信できる。
- **受信**: 対応クライアントは復号インライン表示／非対応は PDF フォールバックをパスワードで開ける。
- **位置づけ**: 業界の不用意なAI露出を減らす**オープンな提案型**（将来の標準化のきっかけを狙う）。

### 2.9 AI 活用（[AI_FEATURES.md](AI_FEATURES.md)）

メール作成・整理を AI で支援。Primadoc の **マルチモデル（Claude / GPT / Gemini）＋ ローカル Ollama** 基盤を流用。

- **件名の自動生成**（本文から）/ **本文ドラフト・リライト・トーン調整** / **スレッド要約** / **返信候補の提案** / **自動分類・タグ提案**
- **方針**: AI は**オプトイン**。既定はクラウド（Claude）、**機密データはローカル（Ollama）を選択可**。生成物は**人が確認・編集してから送信**。
- 軽量用途（件名・分類）は Haiku 4.5、生成・要約は Sonnet 4.6 / Opus 4.8 を目安。

---

## 3. バックエンドインターフェース（Tauri コマンド）

> 旧計画の REST API（FastAPI）は廃止。フロント／バック間は Tauri の `#[tauri::command]` + `invoke()` で通信する。境界型は ts-rs で `src/bindings/` に生成。

旧 REST エンドポイントとの対応（実装時の目安）:

| ドメイン | コマンド（例） | 旧 REST 相当 |
|---------|--------------|-------------|
| アカウント | `account_add` / `account_list` / `account_update` / `account_remove` | `POST/GET/PUT/DELETE /api/accounts` |
| メール | `mail_list`（ページネーション）/ `mail_get` / `mail_compose`（reply / fresh）/ `mail_send` / `mail_update`（既読・フラグ）/ `mail_delete` | `/api/emails*` |
| AI | `ai_generate_subject` / `ai_draft_body` / `ai_summarize_thread` / `ai_suggest_reply` / `ai_classify` / `ai_settings_get` / `ai_settings_set` | （新規） |
| スレッド | `thread_list` / `thread_messages` / `thread_split` / `thread_merge` / `thread_rename` / `message_reassign` / `thread_rebuild` | `/api/threads*`（＋再構築系を新規） |
| 検索 | `search_run` / `search_suggest` | `/api/search*` |
| タグ | `tag_list` / `tag_create` / `tag_update` / `tag_delete` | `/api/tags*` |
| フィルタ | `inbox_filter`（ファセット）/ `message_set_flag`（ブックマーク・要再確認）/ `filter_save` / `filter_list` / `filter_delete` / `contact_set_business` / `category_list` / `category_assign` | （新規。[FILTERING.md](FILTERING.md)） |
| 同期 | `sync_start` / `sync_status` / `sync_stop` / `account_set_sync_window` / `account_set_retention` / `message_fetch_body` / `attachment_download` / `server_search` | `/api/sync*`（＋範囲設定。[SYNC.md](SYNC.md)） |
| 住所録 | `contact_list` / `contact_get` / `contact_upsert` / `contact_delete` / `contact_group_list` | （新規） |
| カレンダー | `event_list`（期間指定）/ `event_get` / `event_upsert` / `event_delete` / `ics_import` | （新規） |
| ウィンドウ | `window_set_always_on_top` / `window_set_mode`（dashboard / widget） | （新規） |
| 背景画像 | `background_list` / `background_import` / `background_remove` / `background_set_active` / `background_set_mode` | （新規） |
| SNS統合 | `channel_list` / `channel_connect` / `inbox_list`（横断）/ `message_send` / `message_mark` | （新規・中継サービス経由） |
| 通知 | Tauri イベント（`emit`/`listen`）。SNS は中継サービスからの WebSocket 配信 | WebSocket `/ws/notifications` |

データモデル例（ts-rs で Rust→TS 生成）:

```rust
// src-tauri 側の境界型（#[derive(TS)] を付与し src/bindings/ へ export）
pub struct MailSummary {
    pub id: i64,
    pub message_id: String,
    pub thread_id: String,
    pub subject: String,
    pub from_address: String,
    pub to_addresses: Vec<String>,
    pub date: String,        // ISO8601
    pub preview: String,
    pub is_read: bool,
    pub is_flagged: bool,
    pub has_attachments: bool,
    pub tags: Vec<Tag>,
}
```

---

## 4. セキュリティ設計

### 認証情報の保護
- 資格情報・OAuth トークンは **`keyring`（OS 金庫）** に保存（旧計画の Electron safeStorage の代替）
- 復号は Rust バックエンドでのみ実施、メモリ上の保持は最小限
- トークンの定期ローテーション

### 通信・実行セキュリティ
- Tauri `capabilities/` による最小権限付与
- CSP（`tauri.conf.json`）設定、XSS 対策
- SQL は `rusqlite` のプレースホルダでインジェクション対策
- IMAP/SMTP は TLS/SSL 必須

### データ保護
- SQLite 暗号化（SQLCipher）
- 添付ファイルの暗号化保存（AES-256 / `aes-gcm`）
- 安全な一時ファイル処理、定期的なデータクリーンアップ

---

## 5. パフォーマンス最適化

### フロントエンド
- `React.memo` / `useMemo` の適切な使用
- 仮想スクロール
- 画像の遅延読み込み
- Web Worker 活用（重い整形処理など。検索・暗号化は Rust 側へ寄せる）

### バックエンド（Rust）
- `tokio` による非同期処理の徹底
- DB クエリ最適化（インデックス・FTS5）
- キャッシュ戦略、バッチ処理

---

## 6. テスト戦略

| 種別 | ツール |
|------|--------|
| 単体（フロント） | Vitest |
| 単体（バック） | `cargo test` |
| 統合 | Vitest + Tauri（モック invoke）/ Rust 統合テスト |
| E2E | Playwright（または tauri-driver / WebDriver） |
| パフォーマンス | 計測スクリプト |

**カバレッジ目標**: コアロジック 90%以上 / インターフェース層 80%以上 / UI 70%以上

---

## 7. 将来の拡張計画

| 期間 | 内容 |
|------|------|
| 短期（3〜6ヶ月） | モバイルアプリ、追加メールプロバイダ対応、プラグインシステム |
| 中期（6〜12ヶ月） | AI 機能（スマート返信・分類）、チーム共有、高度な自動化 |
| 長期（1年以上） | 多言語拡張、エンタープライズ機能、SaaS 版 |
