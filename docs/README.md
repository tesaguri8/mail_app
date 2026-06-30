# ドキュメント一覧

Comfort Mail（仮称）の計画・設計ドキュメント。プロジェクトは **実装未着手の計画段階**（コード 0 行）であり、技術スタックは **Primadoc 同等**（デスクトップ = Tauri 2 + Rust／モバイル = Expo / React Native）を採用する。
**メール／住所録／カレンダー／SNS（LINE・Instagram・Messenger・WhatsApp）** を束ねる**メッセージハブ**を目指し、全面ビジュアルのホーム（時計・日付ウィジェット化して常駐可能）を起点とする。**デスクトップ＋モバイルのクロスプラットフォーム**で、メールは各端末が IMAP で独立同期する。

**実装の優先順位**: まず**コア機能（メール → 住所録 → カレンダー）を安定**させる。**SNS 統合は後続ステップ**としてコア安定後に着手する。

ドキュメントはすべて本 `docs/` 配下に集約する。

| ドキュメント | 内容 |
|------------|------|
| [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) | 採用スタック・ディレクトリ構成・フェーズ計画・リスク（起点） |
| [FEATURE_SPEC.md](FEATURE_SPEC.md) | 機能仕様（表示／検索／タグ／住所録／カレンダー）・Tauri コマンド・セキュリティ・テスト・将来拡張 |
| [THREADING.md](THREADING.md) | スレッド再構築エンジン（引用解析・論理スレッド・自動分割／再件名・ヘッダ活用） |
| [FILTERING.md](FILTERING.md) | フィルタリング（ブックマーク・要再確認・知り合い・取引実績・グループ・カテゴリ・保存フィルタ） |
| [SYNC.md](SYNC.md) | 同期範囲・保持期間（取得する期間をユーザーが選択。本文/添付の遅延取得） |
| [PROTECTED_REGIONS.md](PROTECTED_REGIONS.md) | 保護領域（プライバシー伏字・暗証PDF・AIには伏字・オープン提案型） |
| [AI_FEATURES.md](AI_FEATURES.md) | AI 活用（件名/本文生成・要約・返信提案・分類・「新規メール」連携・プライバシー方針・JSON の扱い） |
| [DATABASE_SCHEMA.md](DATABASE_SCHEMA.md) | SQLite スキーマ（テーブル・インデックス・FTS5） |
| [UI_UX_DESIGN.md](UI_UX_DESIGN.md) | UI/UX 設計（フレームレス全面ビジュアル・ダッシュボード／ウィジェットモード・サイドバー・チャット表示・カラー・a11y・ダークモード） |
| [DATA_STORAGE.md](DATA_STORAGE.md) | データ保存場所設計（OS 別パス・Rust 実装例・バックアップ） |
| [APP_IDENTITY.md](APP_IDENTITY.md) | アプリ識別情報の単一ソース化（製品名・identifier のハードコード排除・改名フロー） |
| [I18N.md](I18N.md) | 多言語対応（i18next / react-i18next） |
| [CROSS_PLATFORM.md](CROSS_PLATFORM.md) | クロスプラットフォーム / モバイル版（Expo・独立 IMAP 同期・共有 mail-core） |
| [SNS_INTEGRATION.md](SNS_INTEGRATION.md) | SNS 統合（メッセージハブ）・プラットフォーム別可否・クラウド中継アーキテクチャ |

> プロジェクト全体の概要・技術スタック・開発コマンドは、リポジトリ直下の [`CLAUDE.md`](../CLAUDE.md) を参照。

---

最終更新日: 2026-06-30
