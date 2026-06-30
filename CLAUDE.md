# Comfort Mail（仮称）開発プロジェクト

## プロジェクト概要

**Comfort Mail（仮称）** は、既存のメールクライアントにない、モダンで直感的なユーザー体験を提供するデスクトップメールアプリケーションです（「気持ちよく使えるメール」がコンセプト）。チャット形式の会話ビュー、高速検索、スマートな振り分け機能を特徴とします。さらに**住所録・カレンダー**、および**SNS（LINE・Instagram・Messenger・WhatsApp）の DM・コメント**を統合し、すべてを 1 つのチャット形式インボックスに束ねる**メッセージハブ**を目指します（特に宿泊施設などでの問い合わせ取りこぼし防止）。

ホームはウィンドウ全面を美しい画像が覆うフレームレスのパネルで、普段は小さくして**時計・日付ウィジェット**のようにデスクトップへ常駐できます。UI 方針の詳細は [docs/UI_UX_DESIGN.md](docs/UI_UX_DESIGN.md) を参照。

> 看板メッセージ：**「Windows で唯一の、軽くて美しくて、AIに勝手に中身を渡さない、所有したくなるメール」**。フリーウェアで TSG One アプリ群の一員（AIトークン利用可）。プロダクトの北極星は [docs/POSITIONING.md](docs/POSITIONING.md)。

## 技術スタック

> **Primadoc 同等スタック（Tauri 2 + Rust）を採用。** 構成・段階計画は [docs/DEVELOPMENT_PLAN.md](docs/DEVELOPMENT_PLAN.md) を参照。本プロジェクトは実装未着手の計画段階。

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
- **async-imap + tokio** - IMAP 受信・同期
- **lettre** - SMTP 送信
- **mail-parser** - MIME 解析
- **keyring** - 資格情報を OS 金庫に保存（Electron safeStorage の代替）

### データベース
- **SQLite**（Rust `rusqlite`）- メタデータ管理・索引
- **SQLCipher** - データベース暗号化（`bundled-sqlcipher`）
- **FTS5** - 全文検索（メール大量件数向けにインデックス検索）
- マイグレーションは Rust 側で自前 SQL 管理（Alembic 不採用）

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
│   ├── bindings/       # ts-rs 生成の Rust→TS 型（手書き禁止）
│   ├── renderer/       # components/ hooks/ stores/ services/ locales/ config/ ...
│   └── shared/         # フロント/バック共有ロジック
├── src-tauri/          # Rust バックエンド
│   ├── src/
│   │   ├── commands/   # #[tauri::command]（account, mail, search, tag, sync ...）
│   │   ├── services/   # imap/ smtp/ parser/ store/ search/ crypto.rs ...
│   │   ├── lib.rs / main.rs / error.rs
│   ├── capabilities/   # 権限定義（宣言的）
│   └── tauri.conf.json
├── mobile/             # モバイル版（Expo / React Native）
├── packages/           # 共有 TS（mail-core: 引用解析/スレッド再構築, types, i18n, utils）
├── config/             # 定数の単一ソース（app-identity.json 等。ハードコード排除）
├── scripts/            # 開発ツール（sync-app-identity.mjs 等）
├── spec/               # 公開仕様（保護領域の相互運用。コードは非公開、仕様のみ公開）
└── docs/               # ドキュメント（非公開・内部設計）
```

> **公開方針**: アプリ本体コードは非公開（フリーウェア）。セキュリティ提案（保護領域）の**相互運用仕様だけを `spec/` で公開**（ベンダー中立）。テレメトリは最小・透明・オプトイン（コンテンツは送らない）。詳細: [docs/POSITIONING.md](docs/POSITIONING.md)。

## データ保存場所

アプリ識別子（identifier）規則: **`tesaguri.<app_name>.app`**（Tesaguri アプリ共通）。
**暫定値: `tesaguri.comfortmail.dev`**（仮称 **Comfort Mail**、`.dev` は暫定。正式確定時に `tesaguri.<確定名>.app` へ）。
データディレクトリはこの identifier をフォルダ名として各 OS 標準場所に配置（詳細: [docs/DATA_STORAGE.md](docs/DATA_STORAGE.md)）。

> **ハードコード排除**: 製品名・identifier は `config/app-identity.json`（単一ソース）に集約し、`tauri.conf.json` / TS / Expo へ生成・実行時参照で配る。直書きしない（詳細: [docs/APP_IDENTITY.md](docs/APP_IDENTITY.md)）。

### Windows
```
C:\Users\{username}\AppData\Roaming\tesaguri.comfortmail.dev\
```

### macOS
```
~/Library/Application Support/tesaguri.comfortmail.dev/
```

### Linux
```
~/.local/share/tesaguri.comfortmail.dev/
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

SQLite（`rusqlite` + SQLCipher + FTS5）を Rust バックエンドで管理。マイグレーションは
Alembic ではなく、`src-tauri/src/services/store/` 内で自前のバージョン管理 SQL として実装する。

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
- データベース: SQLCipher 暗号化
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

3. **SQLCipher のビルドで詰まる場合**
   - `rusqlite` の `bundled-sqlcipher` フィーチャと、Windows ビルドツール（MSVC）の導入を確認

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

---

最終更新日: 2026年6月（Tauri 2 + Rust スタックで計画策定）