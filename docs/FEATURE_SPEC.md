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
3. スマートタグシステム
4. マルチアカウント対応
5. **住所録（アドレス帳）** — メールアプリの正式スコープとして実装
6. **カレンダー** — 予定管理。メール・連絡先と連携

> 本アプリは「メール / 住所録 / カレンダー」を束ねるパーソナルなホームを目指す。
> ホームの常駐・ウィジェット化を含む UI 方針は [UI_UX_DESIGN.md](UI_UX_DESIGN.md) を参照。

---

## 2. 詳細機能仕様

### 2.1 メール表示機能

#### チャット形式ビュー
- 同一スレッドのメールをチャット風に表示
- 送信者ごとにメッセージをグループ化
- タイムスタンプの自動グループ化（5分以内）
- インライン返信機能
- 既読/未読の視覚的表現
- 実装: 仮想スクロール（大量メール対応）、リアルタイム更新、アニメーション付き追加

#### 従来形式ビュー
- 標準的なメールスレッド表示
- 折りたたみ可能な返信履歴
- クイックプレビュー
- 一括操作対応

### 2.2 検索システム

**検索対象**: 件名・本文・送信者・受信者 / 添付ファイル名 / タグ・フラグ / 日付範囲 / サイズ

**検索方式**: 全文検索（FTS5）/ ファセット検索 / 自然言語クエリ（将来）/ 検索履歴とサジェスト

**インデックス戦略**: 非同期インデックス作成 / 差分更新 / 定期的な最適化 / メモリ効率的な実装

### 2.3 タグ・振り分けシステム

**タグ機能**: 手動タグ付け / 自動タグ付けルール / タグの階層構造 / カラーコーディング / タグベースのフィルタリング

**ルールエンジン条件**: 送信者ベース / 件名パターンマッチ / 本文キーワード / 添付有無 / 時間帯

**スマートフォルダ**: 動的フォルダ（検索条件保存）/ プリセット（未読・重要 等）/ カスタム

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

---

## 3. バックエンドインターフェース（Tauri コマンド）

> 旧計画の REST API（FastAPI）は廃止。フロント／バック間は Tauri の `#[tauri::command]` + `invoke()` で通信する。境界型は ts-rs で `src/bindings/` に生成。

旧 REST エンドポイントとの対応（実装時の目安）:

| ドメイン | コマンド（例） | 旧 REST 相当 |
|---------|--------------|-------------|
| アカウント | `account_add` / `account_list` / `account_update` / `account_remove` | `POST/GET/PUT/DELETE /api/accounts` |
| メール | `mail_list`（ページネーション）/ `mail_get` / `mail_send` / `mail_update`（既読・フラグ）/ `mail_delete` | `/api/emails*` |
| スレッド | `thread_list` / `thread_messages` | `/api/threads*` |
| 検索 | `search_run` / `search_suggest` | `/api/search*` |
| タグ | `tag_list` / `tag_create` / `tag_update` / `tag_delete` | `/api/tags*` |
| 同期 | `sync_start` / `sync_status` / `sync_stop` | `/api/sync*` |
| 住所録 | `contact_list` / `contact_get` / `contact_upsert` / `contact_delete` / `contact_group_list` | （新規） |
| カレンダー | `event_list`（期間指定）/ `event_get` / `event_upsert` / `event_delete` / `ics_import` | （新規） |
| ウィンドウ | `window_set_always_on_top` / `window_set_mode`（dashboard / widget） | （新規） |
| 通知 | Tauri イベント（`emit`/`listen`） | WebSocket `/ws/notifications` |

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
