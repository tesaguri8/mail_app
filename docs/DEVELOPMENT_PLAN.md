# 開発計画 — SNGDesign メールアプリ

**ステータス:** 計画策定中（実装未着手 / コード 0 行）
**作成日:** 2026-06-30
**対象:** SNGDesign メールアプリ（mail_app）

---

## 0. この計画の位置づけ

mail_app はまだ**実装コードが 1 行も存在しない**、純粋な計画段階のプロジェクトである。
本ドキュメントは、実装を始める前に **採用する技術スタック・構成・段階計画** を定義するものである。

スタックは **Primadoc と同等（Tauri 2 + Rust）** を採用する。Primadoc / Doculator で確立済みのパターンをそのまま土台にできるため、構成・規約・ツールチェーンを流用する。

参照する確立済み実装:

- `C:\Users\shingo\dev\primadoc`（Tauri 2 本体）
- `primadoc/docs/ARCHITECTURE/`（構成ドキュメント群）
- `primadoc/docs/CODING_GUIDELINES.md`（コーディング規約）
- `C:\Users\shingo\dev\doculator`（Tauri 構成の参考実装。Rust バックエンド ~57行）

---

## 1. 採用スタック（Primadoc 同等）

| レイヤ | 採用技術 |
|--------|---------|
| アプリ基盤 | **Tauri 2**（Rust + WebView） |
| バックエンド言語 | **Rust**（Tauri バックエンドに統合。別プロセスは持たない） |
| フロント/バック通信 | `#[tauri::command]` + `invoke()`（HTTP API 層は持たない） |
| UI | React 18 + TypeScript 5（`any` 原則禁止） |
| ビルド | Vite（`tauri dev` / `tauri build`） |
| スタイル | TailwindCSS 4（@tailwindcss/postcss） |
| 状態管理 | Zustand |
| データ取得 | invoke ラッパー + Zustand（React Query は不採用） |
| 多言語 | i18next / react-i18next（ja / en） |
| 型共有 | ts-rs（Rust→TS 型を `src/bindings/` に自動生成） |
| DB | SQLite + SQLCipher + FTS5（Rust `rusqlite`） |
| 資格情報 | keyring（OS 金庫） |
| テスト | Vitest（フロント）/ `cargo test`（Rust） |
| Lint/Format | ESLint + Prettier + Husky + lint-staged + markdownlint-cli2 |
| 配布 | tauri build（nsis/dmg/deb/appimage）+ tauri-plugin-updater |

**特徴:** Electron のように別プロセスのバックエンドランタイムを同梱せず、Tauri の単一 Rust バックエンドにすべてを統合する。これによりバンドルサイズは小さく（Doculator 実績で ~10–15MB）、ファイル I/O・暗号化・全文検索を Rust ネイティブで高速に処理できる。

---

## 2. メールアプリ固有の技術選定

Primadoc はドキュメントエディタであり、**IMAP/SMTP・大量メールの全文検索・MIME 解析** といった
メール固有の要件は持たない。この部分は本プロジェクトで新規に選定する。すべて Rust クレートで完結させる。

| 要件 | 採用候補クレート | 備考 |
|------|----------------|------|
| IMAP 受信・同期 | `async-imap` + `tokio` | 非同期。IDLE による push 同期も視野 |
| SMTP 送信 | `lettre` | 添付・TLS・認証対応 |
| MIME 解析 | `mail-parser`（stalwartlabs） | 本文/添付/ヘッダ抽出 |
| メール組み立て | `mail-builder` / `lettre` ビルダ | 送信メール生成 |
| メタデータ/索引 DB | `rusqlite`（`bundled-sqlcipher` + FTS5） | 暗号化 + 全文検索を一体で |
| 資格情報保存 | `keyring` | OAuth トークン/パスワード |
| OAuth2（Gmail等） | `oauth2` + `reqwest` | 後続フェーズ。まずアプリパスワード対応 |
| 本文/添付の暗号化 | `aes-gcm` / `ring` | Primadoc と共通パターン |

### 確定した方針

- **全文検索は SQLite FTS5 を採用**（Primadoc の JSON メタデータストア方式ではなく）。メールは件数が数千〜数万に達するため、FTS5 のインデックス検索が適切。
- **認証はまずアプリパスワード/基本認証**から対応。OAuth2（Gmail/Outlook）は後続フェーズ（Phase 3 範囲）。
- **データ取得は invoke + Zustand**（React Query は不採用）。
- **スレッドは独自再構築**: ヘッダ＋引用解析の多層シグナルで論理スレッドを構築し、同件名・別内容を**自動分割＋手動上書き**。アプリ内で**再件名**して整理。コア機能（詳細: [THREADING.md](THREADING.md)）。
- **スコープにメール＋住所録＋カレンダーを含む**（Phase 8 / 9）。
- **ウィンドウはフレームレス全面ビジュアル**。ダッシュボード⇔ウィジェットは**同一ウィンドウのリサイズ連動**で切替（別ウィンドウは持たない）。
- **カレンダーはローカル予定 + .ics 取り込みから**。Google Calendar / CalDAV 双方向同期は後続。
- **SNS 統合（メッセージハブ）は後続ステップ**。まず**コア機能（メール＋住所録＋カレンダー）を安定させてから**着手する（Phase 1〜9 完了が前提）。SNS は Webhook 型のため**クラウド中継サービスを前提**とし、メール本体のローカル完結方針からの意図的な例外とする（詳細: [SNS_INTEGRATION.md](SNS_INTEGRATION.md)）。公式 API のみ使用。

---

## 3. ディレクトリ構造（Primadoc 準拠）

```
mail_app/
├── src/                        # フロントエンド（React レンダラー）
│   ├── bindings/               # ts-rs が生成する Rust→TS 型（手書き禁止）
│   ├── renderer/
│   │   ├── components/         # UI コンポーネント
│   │   ├── hooks/
│   │   ├── stores/             # Zustand ストア
│   │   ├── services/           # invoke() ラッパー（コマンド呼び出し）
│   │   ├── locales/ja, en/     # i18next 翻訳リソース
│   │   ├── config/             # 定数（ハードコード排除）
│   │   ├── contexts/
│   │   ├── styles/
│   │   ├── types/
│   │   ├── utils/
│   │   ├── i18n.ts
│   │   ├── App.tsx
│   │   └── index.tsx
│   └── shared/                 # フロント/バック共有のロジック
├── src-tauri/                  # Rust バックエンド
│   ├── src/
│   │   ├── commands/           # #[tauri::command]: account, mail, thread, search,
│   │   │                       #   tag, attachment, sync, contact, event, settings, window
│   │   ├── services/           # imap/, smtp/, parser/（引用・署名分離）, threading/（論理スレッド再構築）,
│   │   │                       #   store/, search/, contacts/, calendar/（ics 含む）, crypto.rs, account.rs, sync/
│   │   ├── error.rs
│   │   ├── ids.rs
│   │   ├── lib.rs
│   │   └── main.rs
│   ├── capabilities/           # 権限定義（宣言的）
│   ├── icons/
│   ├── Cargo.toml
│   └── tauri.conf.json
├── docs/                       # 本ドキュメント群
├── packages/                   # （任意）npm workspaces で共有パッケージ
├── package.json
├── vite.config.ts
├── tailwind.config.js
├── postcss.config.js
├── tsconfig.json
└── vitest.config.ts
```

---

## 4. アーキテクチャ方針（Primadoc CODING_GUIDELINES 準拠）

- **単一責任 / DRY**：共通 UI パーツは ~100 行以内。
- **ハードコード排除**：定数は `config/`、文字列は i18next リソースへ。
- **型安全**：TS の `any` 原則禁止、Rust は `unwrap()` より `?`/パターンマッチ。
- **境界型は ts-rs で生成**：Rust 構造体に `#[derive(TS)]` を付与し `src/bindings/` へ export。
- **IPC は invoke パターン**：`renderer/services/` に薄いラッパーを置き、コンポーネントから直接 invoke しない。
- **権限は capabilities で宣言**：最小権限。

---

## 5. フェーズ計画

### Phase 0 — 計画策定（本ドキュメント）✅
- 採用スタック・構成・段階計画の定義。`CLAUDE.md` への反映。

### Phase 1 — プロジェクト基盤
- Tauri 2 + React + TS + Vite 雛形を作成。
- TailwindCSS 4 / PostCSS、ESLint / Prettier / Husky / lint-staged / markdownlint-cli2 を Primadoc から流用。
- i18next + react-i18next（ja/en）セットアップ。
- ts-rs 配線（Rust→`src/bindings/`）。
- `tauri.conf.json`（productName, identifier, CSP, updater）整備。
- **フレームレスウィンドウ**（`decorations: false`）+ 自作タイトルバー（`data-tauri-drag-region`）。全面ビジュアル/ウィジェット化の土台（[UI_UX_DESIGN.md](UI_UX_DESIGN.md) §1.5）。

### Phase 2 — データ層
- `services/store/`：`rusqlite`（SQLCipher + FTS5）で DB 初期化・自前マイグレーション。
- スキーマ設計：accounts / emails / threads / tags / email_tags / attachments / email_fts（詳細は [DATABASE_SCHEMA.md](DATABASE_SCHEMA.md)）。
- 境界型（Message, Thread, Account 等）を ts-rs で生成。

### Phase 3 — アカウント・認証
- `services/account.rs` + `keyring`：資格情報を OS 金庫に保存。
- IMAP/SMTP 接続設定（手動設定 → 主要プロバイダ自動判定）。
- まずアプリパスワード方式、OAuth2（Gmail/Outlook）は後続。

### Phase 4 — メール同期・受信＋スレッド解析基盤
- `services/imap/`（`async-imap`）＋ `services/parser/`（`mail-parser`）。
- 差分同期（UIDVALIDITY / UIDNEXT 管理）、添付の遅延取得。
- **スレッド再構築の解析基盤**（[THREADING.md](THREADING.md)）: 引用・署名分離 → `clean_body` 生成、引用属性の (from+時刻) 抽出・fingerprint、活用ヘッダ（`Thread-Index`/`List-Id`/`Delivered-To`/`Authentication-Results` 等）抽出、論理スレッド割当。
- FTS5（`clean_body`）への索引投入。IDLE による準リアルタイム更新は任意。

### Phase 5 — UI（ホーム・表示）
- **ホーム（ダッシュボード）**: 全面背景画像 + 概要パネル + ナビゲーション。時計・日付表示。
- **ウィジェット（コンパクト）モード**: リサイズ連動で時計・日付ウィジェット化。always-on-top トグル。
- チャット形式の会話ビュー（`clean_body` 表示・引用折りたたみ・論理スレッド単位）/ 従来スレッドビュー、メールリスト（仮想スクロール）。
- Zustand ストア + `services/` invoke ラッパー。

### Phase 6 — 送信
- `services/smtp/`（`lettre`）＋作成画面（宛先/件名/本文/添付、下書き、返信引用）。

### Phase 7 — 検索・タグ・スレッド整理
- FTS5 検索 UI（件名/`clean_body`/差出人/添付名）、ファセット、検索履歴。
- 手動/自動タグ、振り分けルールエンジン（`List-Id` 等のヘッダ活用）。
- **スレッド整理 UI**: 自動分割の精緻化、手動の分割/結合/**再件名**、論理スレッドのラベル付け（[THREADING.md](THREADING.md)）。

### Phase 8 — 住所録（アドレス帳）
- `services/contacts/` + `contact_*` コマンド。ローカル連絡先 CRUD・グループ・お気に入り・誕生日。
- メール作成への連絡先補完、差出人の連絡先登録。Google/iCloud 連携は後続。

### Phase 9 — カレンダー
- `services/calendar/`（ローカル予定 + `.ics` 取り込み）+ `event_*` コマンド。
- 月/週/日ビュー、リマインダー通知、ホーム/ウィジェットへの「次の予定」表示。
- メール招待（iCal）連携、参加者を連絡先と紐付け。Google Calendar/CalDAV 同期は後続。

### Phase 10 — ビルド・配布
- `tauri build`（Windows nsis を最優先、将来 dmg/deb/appimage）。
- 署名、`tauri-plugin-updater` 配信。

### SNS 統合トラック（後続ステップ）— メッセージハブ
**前提: コア機能（メール＋住所録＋カレンダー, Phase 1〜9）が安定してから着手する。** まず基本機能の安定を最優先とし、SNS はその次の段階として進める。クラウド中継を伴うため別トラックとして実装（詳細: [SNS_INTEGRATION.md](SNS_INTEGRATION.md)）。

- **S1 基盤**: 共通スキーマ、クラウド中継サービス雛形（tesaguri 基盤に相乗り）、アプリ⇄中継の認証付き WebSocket、ローカルキャッシュ DB（`channels` / `sns_conversations` / `sns_messages`）。
- **S2 LINE**: 受信・返信・通知まで一気通貫（最優先）。
- **S3 Meta**: Instagram + Messenger の DM + コメント。アプリ審査・ビジネス認証を並行申請。
- **S4 WhatsApp Business**: 海外ゲスト向け。テンプレート審査対応。
- **S5 運用強化**: 対応状態・キーワード強調・複数施設管理。（将来）Booking.com / X を検討。

---

## 6. リスクと確認事項

| 項目 | 内容 | 対応 |
|------|------|------|
| Rust メールエコシステムの成熟度 | `async-imap` 等は Python `imaplib` ほど枯れていない | 早期に同期 PoC を実施（Phase 4 前倒し検証） |
| SQLCipher ビルド | `rusqlite` の SQLCipher 同梱はクロスビルドで詰まりやすい | Phase 2 で Windows ビルド確立を最優先 |
| OAuth2 対応 | Gmail/Outlook は最終的に OAuth 必須 | Phase 3 はアプリパスワード、OAuth は別フェーズ |

---

## 7. 関連ドキュメント

機能・設計の詳細は以下に分割（[docs/README.md](README.md) がインデックス）。

- [FEATURE_SPEC.md](FEATURE_SPEC.md) — 機能仕様・Tauri コマンド・セキュリティ・テスト・将来拡張
- [THREADING.md](THREADING.md) — スレッド再構築エンジン（引用解析・論理スレッド・ヘッダ活用）
- [DATABASE_SCHEMA.md](DATABASE_SCHEMA.md) — SQLite スキーマ
- [UI_UX_DESIGN.md](UI_UX_DESIGN.md) — UI/UX 設計
- [DATA_STORAGE.md](DATA_STORAGE.md) — データ保存場所設計
- [I18N.md](I18N.md) — 多言語対応（i18next）
- [SNS_INTEGRATION.md](SNS_INTEGRATION.md) — SNS 統合（メッセージハブ）・クラウド中継アーキテクチャ

## 8. 次アクション

1. 本計画・各設計ドキュメントのレビュー・確定。
2. Phase 1 着手（Tauri 雛形作成）。

---

最終更新日: 2026-06-30
