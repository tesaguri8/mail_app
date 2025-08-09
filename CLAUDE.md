# SNGDesign メールアプリ開発プロジェクト

## プロジェクト概要

このプロジェクトは、既存のメールクライアントにない、モダンで直感的なユーザー体験を提供するデスクトップメールアプリケーションです。チャット形式の会話ビュー、高速検索、スマートな振り分け機能を特徴とします。

## 技術スタック

### フロントエンド
- **Electron 28.x** - デスクトップアプリケーション基盤
- **React 18.x** - UIライブラリ
- **TypeScript 5.x** - 型安全な開発
- **Vite** - 高速ビルドツール
- **TailwindCSS** - ユーティリティファーストCSS
- **Zustand** - 軽量状態管理
- **React Query** - データフェッチング

### バックエンド
- **Python 3.11+** - サーバーサイド言語
- **FastAPI 0.104+** - 高性能Web API フレームワーク
- **SQLAlchemy 2.0** - ORM
- **Alembic** - データベースマイグレーション
- **Pydantic 2.x** - データバリデーション

### データベース
- **SQLite** - メタデータ管理
- **SQLCipher** - データベース暗号化
- **FTS5** - 全文検索

## 主な機能

1. **美しいホーム画面** - 背景写真と重要情報の概要表示
2. **チャット形式メール表示** - 手紙風の温かい会話体験
3. **高速検索** - SQLiteベースの軽快な全文検索
4. **スマートタグ** - 自動振り分けと手動タグ付け
5. **多言語対応** - 日本語・英語（将来的に拡張）
6. **マルチアカウント** - 複数のメールアカウント統合管理

## ディレクトリ構造

```
mail_app/
├── gui/                # Electronアプリケーション
├── api/                # FastAPIバックエンド
├── data/               # 翻訳・設定データ
├── shared/             # 共有型定義
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
# フロントエンド依存関係
cd gui && npm install

# バックエンド依存関係
cd api && pip install -r requirements.txt

# 開発環境初期化
npm run setup:dev
```

### 開発サーバー起動
```bash
# フロントエンド開発サーバー
cd gui && npm run dev

# バックエンドサーバー
cd api && uvicorn app.main:app --reload --host 0.0.0.0 --port 8000
```

### ビルド
```bash
# プロダクションビルド
cd gui && npm run build

# Electronアプリパッケージング
cd gui && npm run dist
```

### テスト
```bash
# フロントエンドテスト
cd gui && npm test

# バックエンドテスト
cd api && pytest

# E2Eテスト
cd gui && npm run test:e2e
```

### リント・フォーマット
```bash
# TypeScript/React
cd gui && npm run lint
cd gui && npm run lint:fix

# Python
cd api && ruff check .
cd api && ruff format .
```

## 翻訳管理

### 翻訳ファイルの場所
```
data/translations/
├── common.json      # 共通翻訳
├── mail.json        # メール関連
├── settings.json    # 設定関連
├── search.json      # 検索関連
└── tags.json        # タグ関連
```

### 翻訳バリデーション
```bash
node scripts/validate-translations.js
```

## データベース管理

### マイグレーション
```bash
# マイグレーション作成
cd api && alembic revision --autogenerate -m "migration description"

# マイグレーション実行
cd api && alembic upgrade head

# マイグレーション履歴
cd api && alembic history
```

## 環境変数

`.env`ファイルを作成して以下の環境変数を設定：

```bash
# アプリケーション設定
APP_NAME=SNGDesign MailApp
APP_VERSION=1.0.0
DEBUG=false

# データベース
DATABASE_URL=sqlite:///./mail.db
DATABASE_ENCRYPTION_KEY=your-encryption-key-here

# ログ設定
LOG_LEVEL=INFO
LOG_FILE=logs/app.log

# メール設定
MAIL_SYNC_INTERVAL=300  # 5分
MAX_ATTACHMENT_SIZE=25MB
```

## セキュリティ

- アカウント認証情報: Electron safeStorage使用
- データベース: SQLCipher暗号化
- 通信: TLS/SSL必須
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

1. **ビルドエラー**
   ```bash
   # node_modules削除して再インストール
   rm -rf node_modules && npm install
   ```

2. **データベース接続エラー**
   ```bash
   # データベースマイグレーション確認
   cd api && alembic current
   ```

3. **翻訳ファイルエラー**
   ```bash
   # 翻訳バリデーション実行
   node scripts/validate-translations.js
   ```

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

最終更新日: 2024年1月