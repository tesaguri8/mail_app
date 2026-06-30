# SNGDesign メールアプリ開発プロジェクト

## プロジェクト概要

このプロジェクトは、既存のメールクライアントにない、モダンで直感的なユーザー体験を提供するデスクトップメールアプリケーションです。チャット形式の会話ビュー、高速検索、スマートな振り分け機能を特徴とします。さらに**住所録・カレンダー**を統合し、「メール／住所録／カレンダー」を束ねるパーソナルなホームを目指します。

ホームはウィンドウ全面を美しい画像が覆うフレームレスのパネルで、普段は小さくして**時計・日付ウィジェット**のようにデスクトップへ常駐できます。UI 方針の詳細は [docs/UI_UX_DESIGN.md](docs/UI_UX_DESIGN.md) を参照。

## 技術スタック

> **Primadoc 同等スタック（Tauri 2 + Rust）を採用。** 構成・段階計画は [docs/DEVELOPMENT_PLAN.md](docs/DEVELOPMENT_PLAN.md) を参照。本プロジェクトは実装未着手の計画段階。

### アプリ基盤
- **Tauri 2** - デスクトップアプリケーション基盤（Rust + WebView）

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

1. **美しいホーム画面** - 全面ビジュアル背景＋概要ダッシュボード。小さくすると時計・日付ウィジェット化し常駐
2. **チャット形式メール表示** - 手紙風の温かい会話体験
3. **高速検索** - SQLiteベースの軽快な全文検索
4. **スマートタグ** - 自動振り分けと手動タグ付け
5. **住所録（アドレス帳）** - 連絡先・グループ・誕生日。メール／カレンダーと連携
6. **カレンダー** - 予定管理（月／週／日）。メール招待・連絡先と連携
7. **多言語対応** - 日本語・英語（将来的に拡張）
8. **マルチアカウント** - 複数のメールアカウント統合管理

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
├── scripts/            # 開発ツール
└── docs/               # ドキュメント
```

## データ保存場所

### Windows
```
C:\Users\{username}\AppData\Roaming\SNGDesign\MailApp\
```

### macOS
```
~/Library/Application Support/SNGDesign/MailApp/
```

### Linux
```
~/.local/share/sngdesign/mailapp/
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
- 認証方式: まずアプリパスワード/基本認証に対応し、OAuth2（Gmail/Outlook）は後続フェーズ
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