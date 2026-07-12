# Rondine 開発プロジェクト

## プロジェクト概要

**Rondine** は、既存のメールクライアントにない、モダンで直感的なユーザー体験を提供するデスクトップメールアプリケーションです（コンセプトは**「SNS のように気持ちよく使えるメール」**＝見た目と操作感は SNS、中身は**記録性（手元に恒久的に残る証跡）とオープン性（IMAP/SMTP・特定アプリに囲い込まれない）**を持つメール）。チャット形式の会話ビュー、高速検索、スマートな振り分け機能を特徴とします。さらに**住所録・カレンダー**、および**SNS（LINE・Instagram・Messenger・WhatsApp）の DM・コメント**を統合し、すべてを 1 つのチャット形式インボックスに束ねる**メッセージハブ**を目指します（特に宿泊施設などでの問い合わせ取りこぼし防止）。

ホームはウィンドウ全面を美しい画像が覆うフレームレスのパネルで、普段は小さくして**時計・日付ウィジェット**のようにデスクトップへ常駐できます。UI 方針の詳細は [docs/UI_UX_DESIGN.md](docs/UI_UX_DESIGN.md) を参照。

> 看板メッセージ：**「Windows で唯一の、軽くて美しくて、AIに勝手に中身を渡さない、所有したくなるメール」**。フリーウェアで TSG One アプリ群の一員（AIトークン利用可）。プロダクトの北極星は [docs/POSITIONING.md](docs/POSITIONING.md)。

## 技術スタック

> **Primadoc 同等スタック（Tauri 2 + Rust）を採用。** 構成・段階計画は [docs/DEVELOPMENT_PLAN.md](docs/DEVELOPMENT_PLAN.md) を参照。ステータス: 実装中（アルファ・0.1.0-alpha.1）。

### アプリ基盤
- **Tauri 2** - デスクトップアプリケーション基盤（Rust + WebView）
- **Expo / React Native** - モバイル版（iOS / Android）。デスクトップとは別アプリ、TS ロジックを `packages/` で共有（[docs/CROSS_PLATFORM.md](docs/CROSS_PLATFORM.md)）

### フロントエンド
- **React 18.x** - UIライブラリ
- **TypeScript 5.x** - 型安全な開発（`any` 原則禁止）
- **Vite** - 高速ビルドツール
- **TailwindCSS 4.x** - ユーティリティファーストCSS（@tailwindcss/postcss）
- **Zustand** - 軽量状態管理
- **i18next / react-i18next** - 多言語対応
- データ取得は `invoke()` ラッパー + Zustand で実装（React Query は不採用）

### バックエンド（Rust / Tauri）
- **Rust** - Tauri バックエンドに統合（別プロセス Python/FastAPI は不採用）
- **`#[tauri::command]` + invoke** - フロント/バック間通信（FastAPI 不要）
- **ts-rs** - Rust→TS 境界型の自動生成（`src/bindings/`）
- **imap（ブロッキング）+ native-tls** - IMAP 受信・同期（`spawn_blocking` で実行。async-imap/tokio-imap は不採用）
- **lettre** - SMTP 送信
- **mail-parser** - MIME 解析
- **keyring** - 資格情報を OS 金庫に保存（Electron safeStorage の代替）

### データベース
- **SQLite**（Rust `rusqlite`・`bundled`）- メタデータ管理・索引
- **FTS5** - 全文検索（メール大量件数向けにインデックス検索）
- **SQLCipher** - データベース暗号化は後続（未導入。現状は `bundled` プレーン SQLite）
- マイグレーションは Rust 側で自前 SQL 管理（Alembic 不採用。現在 0001〜0043、0035 は意図的に欠番）

## 主な機能

1. **美しいホーム画面** - 全面ビジュアル背景（アプリ同梱＋ユーザー取り込み画像、時間帯/日替わりで自動切替）＋概要ダッシュボード。小さくすると時計・日付ウィジェット化し常駐
2. **チャット形式メール表示** - 手紙風の温かい会話体験。引用を剥がし新規部分のみ表示
3. **独自スレッド再構築** - 引用解析＋ヘッダで論理スレッド化。「同件名・別内容」を自動分割し、アプリ内で再件名して整理（[docs/THREADING.md](docs/THREADING.md)）
4. **プライバシー安全表示** - リモート画像/トラッキングの既定ブロック・なりすまし/危険警告UI（[docs/MAIL_SECURITY.md](docs/MAIL_SECURITY.md)）
5. **迷惑メール対策** - ローカル学習＋（オプトイン）TSG One 共有シグナル。本文は送らない（[docs/SPAM.md](docs/SPAM.md)）
6. **高速検索** - SQLiteベースの軽快な全文検索
7. **スマートタグ・フィルタリング** - 自動振り分け／手動タグ。ブックマーク・要再確認・知り合い・取引実績・アドレスグループ・カテゴリで絞り込み（[docs/FILTERING.md](docs/FILTERING.md)）
8. **住所録（アドレス帳）** - 連絡先・グループ・誕生日。メール／カレンダーと連携
9. **カレンダー** - 予定管理（月／週／日）。メール招待・連絡先と連携
10. **AI 活用** - 件名/本文生成・スレッド要約・返信提案・分類。オプトイン／クラウド既定／ローカル(Ollama)選択可（[docs/AI_FEATURES.md](docs/AI_FEATURES.md)）
11. **保護領域（プライバシー伏字）** - 機密は伏字＋暗証PDF（どのクライアントでも開ける）で送り、AIには伏字で渡す。不用意なAI露出を防ぐオープン提案型（[docs/PROTECTED_REGIONS.md](docs/PROTECTED_REGIONS.md)）
12. **メール作成（返信／新規）** - 「このアドレスへ新規メール」で別案件を正しく新規送信し、相手のスレッドを汚さない。下書き/送信取消/予約/署名/定型文/スヌーズ（[docs/COMPOSE.md](docs/COMPOSE.md)）
13. **移行・可搬性** - .eml/.mbox/Thunderbird/Outlook からのインポート、エクスポート（[docs/IMPORT_EXPORT.md](docs/IMPORT_EXPORT.md)）
14. **SNS 統合（メッセージハブ）** ※後続ステップ - LINE／Instagram／Messenger／WhatsApp の DM・コメントを統合インボックスに集約（クラウド中継経由）。**コア機能の安定後に着手**
15. **多言語対応** - 日本語・英語（将来的に拡張）
16. **マルチアカウント** - 複数のメールアカウント統合管理

## ディレクトリ構造

```
mail_app/
├── src/                # フロントエンド（React レンダラー）
│   ├── bindings/       # ts-rs 生成の Rust→TS 型（手書き禁止・約45型）
│   ├── renderer/       # components/ hooks/ stores/ services/ locales/ config/ ...
│   └── shared/         # フロント/バック共有ロジック（未作成）
├── src-tauri/          # Rust バックエンド
│   ├── src/
│   │   ├── commands.rs # #[tauri::command]（単一ファイル・105 ハンドラ）
│   │   ├── models.rs   # 境界型・データモデル
│   │   ├── services/   # フラット構成: imap_sync.rs smtp.rs parser.rs quotes.rs（引用解析）
│   │   │                #   autoconfig.rs vcard.rs gcsv.rs dedupe.rs compress.rs datadir.rs
│   │   │                #   dataver.rs media.rs + spam/ store/（threads.rs=スレッド再構築 等）
│   │   ├── lib.rs / main.rs
│   ├── capabilities/   # 権限定義（宣言的）
│   └── tauri.conf.json
├── mobile/             # モバイル版（Expo / React Native）（未作成・計画）
├── packages/           # 共有 TS（mail-core/types/i18n/utils）（未作成・計画。引用解析/スレッド再構築は Rust services 側で実装済み）
├── config/             # 定数の単一ソース（app-identity.json 等。ハードコード排除）
├── scripts/            # 開発ツール（sync-app-identity.mjs 等）
├── spec/               # 公開仕様（保護領域の相互運用。コードは非公開、仕様のみ公開）
└── docs/               # ドキュメント（非公開・内部設計）
```

> **公開方針**: アプリ本体コードは非公開（フリーウェア）。セキュリティ提案（保護領域）の**相互運用仕様だけを `spec/` で公開**（ベンダー中立）。テレメトリは最小・透明・オプトイン（コンテンツは送らない）。詳細: [docs/POSITIONING.md](docs/POSITIONING.md)。

## データ保存場所

アプリ識別子（identifier）規則: **`tesaguri.<app_name>.app`**（Tesaguri アプリ共通）。
**暫定値: `tesaguri.rondine.dev`**（**Rondine**、`.dev` は暫定。正式確定時に `tesaguri.<確定名>.app` へ）。
データディレクトリはこの identifier をフォルダ名として各 OS 標準場所に配置（詳細: [docs/DATA_STORAGE.md](docs/DATA_STORAGE.md)）。

> **ハードコード排除**: 製品名・identifier は `config/app-identity.json`（単一ソース）に集約し、`tauri.conf.json` / TS / Expo へ生成・実行時参照で配る。直書きしない（詳細: [docs/APP_IDENTITY.md](docs/APP_IDENTITY.md)）。

### Windows
```
C:\Users\{username}\AppData\Roaming\tesaguri.rondine.dev\
```

### macOS
```
~/Library/Application Support/tesaguri.rondine.dev/
```

### Linux
```
~/.local/share/tesaguri.rondine.dev/
```

## 開発コマンド

### 初期セットアップ
```bash
# 依存関係（フロント + Tauri CLI）
npm install

# Rust ツールチェーン（未導入の場合）: https://rustup.rs
```

### 開発サーバー起動
```bash
# Tauri 開発（Rust バックエンド + Vite レンダラーを同時起動）
npm run tauri dev

# レンダラーのみ（UI 単体確認）
npm run dev:renderer
```

### ビルド
```bash
# Tauri アプリのビルド・パッケージング（nsis/dmg/deb/appimage）
npm run tauri build
```

### Rust→TS 型生成
```bash
# ts-rs で境界型を src/bindings/ に出力
npm run gen:bindings
```

### テスト
```bash
# フロントエンドテスト（Vitest）
npm test

# Rust テスト
cd src-tauri && cargo test
```

### リント・フォーマット
```bash
# TypeScript/React
npm run lint
npm run format

# Rust
cd src-tauri && cargo fmt && cargo clippy
```

## 翻訳管理

i18next / react-i18next を使用。文字列は必ず翻訳リソースで管理し、ハードコードしない。

### 翻訳ファイルの場所
```
src/renderer/locales/
├── ja/      # 日本語リソース（common, mail, settings, search, tags ...）
└── en/      # 英語リソース
```

## データベース管理

SQLite（`rusqlite` `bundled` + FTS5。SQLCipher 暗号化は後続・未導入）を Rust バックエンドで管理。マイグレーションは
Alembic ではなく、`src-tauri/src/services/store/migrations/` 内で自前のバージョン管理 SQL として実装する（現在 0001〜0043、0035 は意図的に欠番）。

- スキーマ例: accounts / mailboxes / messages / threads / attachments / tags / messages_fts
- 起動時に現在のスキーマバージョンを確認し、未適用のマイグレーションを順次適用

## 環境変数

アプリ設定の大半は Tauri の設定ストア（`tauri-plugin-store`）で管理し、機密情報は OS 金庫
（`keyring`）に保存する。開発時に必要な環境変数（例: `RUST_LOG`）は `cross-env` 等で渡す。

```bash
# ログレベル（Rust 側）
RUST_LOG=info

# メール設定（既定値の例。実値はアプリ設定で管理）
MAIL_SYNC_INTERVAL=300    # 同期間隔（秒）
MAX_ATTACHMENT_SIZE=25MB  # 添付上限
```

> DB 暗号化キーやアカウント資格情報は環境変数・平文ファイルに置かず、必ず `keyring`（OS 金庫）に保存する。

## セキュリティ

- アカウント認証情報: `keyring`（OS 金庫: Win=Credential Manager / mac=Keychain / Linux=Secret Service）
- 認証方式: 基本メールは普通のクライアント同様の手動 IMAP/SMTP 設定（OAuth 不要）。OAuth は AI・TSG One 連携時のみ（[docs/POSITIONING.md](docs/POSITIONING.md) §5）
- データベース: SQLCipher 暗号化（後続・未導入。現状は `bundled` プレーン SQLite）
- 通信: TLS/SSL 必須（IMAP/SMTP）
- 権限: Tauri `capabilities/` で宣言的に最小権限を付与
- ファイルアクセス: 適切なパーミッション設定

## パフォーマンス最適化

### フロントエンド
- Virtual Scrolling実装
- React.memo適切使用
- 画像遅延読み込み
- Web Worker活用

### バックエンド
- 非同期処理徹底
- データベースクエリ最適化
- 適切なキャッシュ戦略

## トラブルシューティング

### よくある問題

1. **フロントのビルドエラー**
   ```bash
   # node_modules削除して再インストール
   rm -rf node_modules && npm install
   ```

2. **Rust / Tauri のビルドエラー**
   ```bash
   # 依存の再取得・キャッシュクリア
   cd src-tauri && cargo clean && cargo build
   ```

3. **SQLCipher のビルドで詰まる場合**（※現状は `bundled` プレーン SQLite を使用。SQLCipher は後続で導入予定）
   - 導入時は `rusqlite` の `bundled-sqlcipher` フィーチャと、Windows ビルドツール（MSVC）の導入を確認

## 貢献方法

1. Issueを作成して機能要求やバグを報告
2. フォークしてフィーチャーブランチ作成
3. コミットメッセージは日本語または英語で明確に
4. プルリクエスト作成前にテスト実行
5. コードレビューを経てマージ

## ライセンス

このプロジェクトは SNGDesign の所有物です。

## 連絡先

開発に関する質問やサポートが必要な場合は、プロジェクトのIssueを作成してください。

## コード修正

コーディング・アーキテクチャは、Tauriらしい理想形を目指すこと。

具体的には:

- 非Tauri的な設計（プロセス分離を前提とした過剰なIPC設計、Web/Node 由来の旧来パス処理等）を持ち込まず、Tauriの作法（`#[tauri::command]` + `invoke`、`Emitter`/`listen`、`AppHandle`引き回し）に揃える
- Rust側コマンドは薄く保ち、ロジックは`src-tauri/src/services/`に集約する（`commands.rs` にビジネスロジックを溜めない）
- 「動けばよい」ではなく、責務分離・命名・型安全性を優先する（TypeScript側は`any`型を避け、境界型は `src/bindings/` の ts-rs 生成型を使う）
- 同じ目的のコードが複数箇所に散らないようDRY原則を徹底する（**ただし将来のモバイル版との関係は例外**: 下記参照）

### モバイル版（`mobile/`）の構造的方針（着手時の指針）

> **現状 `mobile/` と `packages/`（`.gitkeep` のみ）は未作成・計画段階。** モバイル版の全体計画は [docs/CROSS_PLATFORM.md](docs/CROSS_PLATFORM.md) を参照。本節は**モバイル版に着手する時点での指針**であり、現時点のコードには対象が存在しない。

Rondine のモバイル版は **Expo / React Native** で、デスクトップとは別アプリとして構築する予定である。技術スタック節では「TS ロジックを `packages/` で共有」を掲げているが、**着手時には次の Primadoc の教訓を踏まえて共有方式を再評価すること**:

- Expo + React Native + npm workspaces を素朴に組み合わせると、root と mobile の React バージョン重複で起動時 TurboModule init が SIGABRT する
- Metro の `extraNodeModules` / `blockList` による React 二重ロード抑止は綿密な回避策が必要で破綻しやすい
- babel-preset-expo / Metro の hoisting が非対称になり Codegen の前提が崩れる

このため Primadoc では最終的に **mobile/ を自己完結型 Expo プロジェクトとし、共有ロジックはワークスペース参照ではなく意図的に複製**する方針に落ち着いた。Rondine でモバイル版を立ち上げる際は、`packages/` によるワークスペース共有を採用するか、Primadoc 方式（自己完結型＋複製）を採るかを、上記リスクを踏まえて**最初に確定**させ、確定後は本節と技術スタック節の記述を実態に合わせて更新すること。

**着手時の運用ルール（Primadoc 方式を採る場合の想定）:**

- 共有ロジックを複製する場合は、複製元を編集したら対応する `mobile/` 側も**手動で同期**する（同期漏れを防ぐため対応表を README 化する）
- mobile に `@rondine/*` 等のワークスペース import や `../../../../../src/...` 形式の相対パス参照を**追加しない**（コードレビューで弾く）
- mobile 側の tsconfig / metro.config に monorepo 配線（`../packages/*`、`extraNodeModules`、`watchFolders` でリポジトリルートを指す等）を安易に**復活させない**

### Rustコードの作法（厳格）

Rust側コード（`src-tauri/`配下）は、以下を**例外なく**遵守すること。「動けばよい」コードは Rust では長期メンテナンス時に必ず破綻するため、最初から Rust らしく書く。

**エラー処理:**

- `unwrap()` / `expect()` を本番経路で使わない。テストコードと、論理的に絶対に失敗しないことが証明できる箇所のみ許容（`expect()`の場合はメッセージに理由を明記）
- エラーは `thiserror` で**型付きエラー列挙型**として定義する。`String`エラーで握り潰さない
- `?` 演算子でエラーを伝播させる。`match` で都度ハンドリングするのは境界・分岐が必要な場面のみ
- `Result<T, String>` への変換は **`#[tauri::command]` 境界でのみ** 行う（フロントエンドに渡す最後の段階）。サービス層は型付きエラーのまま受け渡す
- `panic!()` / `unreachable!()` / `todo!()` を残さない。残す場合は理由をコメントで明記

**所有権・型:**

- `.clone()` を安易に使わない。借用（`&T` / `&mut T`）で済むなら借用、所有権移動で済むなら move。clone は本当に必要な箇所のみ
- `String` と `&str`、`Vec<T>` と `&[T]`、`PathBuf` と `&Path` を使い分ける。引数は基本 borrowed 型を取る
- `Option`/`Result` を `if let` / `match` / コンビネータ（`map`/`and_then`/`ok_or`等）で扱う。`is_some()` → `unwrap()` の連鎖は禁止
- 型エイリアスやnewtypeで意味を持たせる（例: `pub type Guid = String;` ではなく `pub struct Guid(String);`）

**並行性:**

- `Arc<Mutex<T>>` と `Arc<RwLock<T>>` を読み書きパターンで使い分ける（読み多数なら RwLock）
- ロックのスコープは最小限に。`MutexGuard` を保持したまま `.await` しない（デッドロック原因）
- `tokio::spawn` で投げたタスクの `JoinHandle` は適切に管理（fire-and-forget の場合はコメントで明示）
- IMAP はブロッキング実装のため、同期処理は `spawn_blocking` で実行し、非同期ランタイムを塞がない

**スタイル・構成:**

- イテレータと関数型スタイルを優先（手書き `for` ループより `iter().map().filter().collect()`）
- 命名規約: モジュール・関数・変数は `snake_case`、型・トレイト・列挙子は `UpperCamelCase`、定数は `SCREAMING_SNAKE_CASE`
- モジュール構成は `mod.rs` ではなく**ファイル名一致**（Rust 2018+ 推奨形式: `services/imap_sync.rs` のように `services/<module>.rs` + `services/<module>/<sub>.rs`）
- 1ファイル500行を超えたら責務分割を検討。1関数50行を超えたら抽出を検討（`commands.rs` は単一ファイルで肥大化しやすいので特に注意）

**lint・警告:**

- `cargo clippy --all-targets -- -D warnings` がゼロ警告で通ること
- `#[allow(...)]` を使う場合は**直上に理由コメント必須**。`#[allow(dead_code)]` で未使用コードを温存しない（消すか、`#[cfg(test)]` 等で意味づけする）
- `cargo fmt` 準拠（rustfmt のデフォルト設定）。ただし本リポジトリはローカル rustfmt でクリーンではないため、**クレート全体への `cargo fmt` は実行しない**（無関係な巨大差分が出る）。整形は編集した範囲に限定する

**ドキュメンテーション:**

- 公開関数（`pub fn`）には `///` doc コメントで「何をするか・引数・戻り値・エラー条件」を記載
- `unsafe` ブロックは原則禁止。やむを得ず使う場合は `// SAFETY:` コメントで安全性の根拠を明記

### 編集

- 会話、コード内コメント記載は、日本語とする。

### 新しいセッション作成時

以下のドキュメントを熟読してから作業を行うこと（作業領域に応じて）

- [docs/SYNC.md](docs/SYNC.md) / [docs/CALENDAR_SYNC.md](docs/CALENDAR_SYNC.md)（メール・カレンダー同期関連の作業時）
- [docs/AI_FEATURES.md](docs/AI_FEATURES.md)（AI 機能関連の作業時）
- [docs/THREADING.md](docs/THREADING.md)（スレッド再構築・会話ビュー関連の作業時）
- [docs/SPAM.md](docs/SPAM.md) / [docs/GREEN_DOMAINS.md](docs/GREEN_DOMAINS.md)（迷惑メール判定関連の作業時）

### 修正後

- 開発中は、`npm run dev`（= `tauri dev`）で開発サーバーを起動しているので、ビルド確認は不要。`tauri dev` 実行中に `cargo check` / `cargo build` を回さない（`target` のロックを奪い合い、dev サーバーが停止・ハングする原因になる）。
- ブランチのマージ作業は、ユーザーからの指示がある場合のみ行う。
- `dev` ブランチにマージする前に、必ず、不要となったコードを調査し、整理を行う（モバイル版着手後に `mobile-dev` 相当のブランチを設けた場合も同様）。
- ブランチのマージ時は、ブランチを削除せずに、残しながらマージしてください。

### Git Worktree 運用ルール

Tauri のビルド成果物（`target/`、`node_modules/`、Rust 依存解決）の再構築には時間がかかるため、複数ブランチでの並行作業には **git worktree** を用いる。**worktree 自体は削除せず、ブランチを切り替えながら使い回す**ことで、ビルドキャッシュを維持する。

**worktree 構成:**

| パス | 役割 | 許可するブランチ |
|------|------|------------------|
| `C:\Users\shingo\dev\rondine` | メイン worktree（dev 専用・マージ作業用） | `dev` のみ |
| `C:\Users\shingo\dev\rondine-wt-1` | 作業用 worktree #1 | 作業ブランチ、または待機時は `wt-1` |
| `C:\Users\shingo\dev\rondine-wt-2` | 作業用 worktree #2 | 作業ブランチ、または待機時は `wt-2` |

**運用ルール:**

1. **メイン worktree は dev 専用** — `rondine/` では **`dev` ブランチ以外をチェックアウトしない**。`dev` へのマージ作業は必ずこのメイン worktree で実施する。
2. **作業ブランチは wt-1 / wt-2 worktree で展開** — `feature/xxx` 等の作業ブランチは `rondine-wt-1/` または `rondine-wt-2/` でチェックアウトして作業する。メイン worktree では作業ブランチをチェックアウトしない。
3. **作業終了後は待機ブランチに戻す** — 作業ブランチがマージ済み等で不要になったら、各作業 worktree は対応する待機ブランチ（`wt-1` / `wt-2`）にチェックアウトを戻す。**worktree 自体は `git worktree remove` で削除しない**。
4. **待機ブランチ（wt-1 / wt-2）は常に最新 dev と同期** — 次の作業着手をスムーズにするため、待機状態の `wt-1` / `wt-2` ブランチは最新の `dev` を取り込んで同期した状態を維持する（Tauri 依存解決の再利用率を高めるため）。
5. **dev へのマージ手順:**
   1. 作業 worktree（`rondine-wt-1/` または `rondine-wt-2/`）で作業ブランチを最新化・コミット
   2. メイン worktree（`rondine/`）に移動し、`dev` ブランチであることを確認
   3. メイン worktree 上で `git merge <作業ブランチ>` を実行（ブランチは削除せず残す — 「修正後」のルールに従う）

**禁止事項:**

- メイン worktree（`rondine/`）で `dev` 以外のブランチをチェックアウトすること
- 作業 worktree（`rondine-wt-1/` / `rondine-wt-2/`）で `dev` ブランチをチェックアウトすること
- worktree 自体を削除すること（Tauri 再ビルドコスト回避のため、必ず維持する）

**現在の worktree 一覧の確認:**

```bash
git worktree list
```

### プロジェクト関連レポジトリ

- Rondine アプリ（メイン worktree）："C:\Users\shingo\dev\rondine"
- Rondine website："C:\Users\shingo\dev\www\sites\rondine-website"（現状プレースホルダ・未整備。整備後に frontend/backend 構成へ）
- ローカルのユーザー設定フォルダー：TSG One 配下に Rondine 用は未作成。作成時は "C:\Users\shingo\Nextcloud\shingo-Cloud\tsg-one\rondine" を想定

---

最終更新日: 2026年7月（Tauri 2 + Rust スタック）