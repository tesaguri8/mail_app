# ドキュメント一覧

SNGDesign メールアプリの計画・設計ドキュメント。プロジェクトは **実装未着手の計画段階**（コード 0 行）であり、技術スタックは **Primadoc 同等（Tauri 2 + Rust）** を採用する。
**メール／住所録／カレンダー**を統合し、全面ビジュアルのホーム（時計・日付ウィジェット化して常駐可能）を起点とするアプリを目指す。

ドキュメントはすべて本 `docs/` 配下に集約する。

| ドキュメント | 内容 |
|------------|------|
| [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) | 採用スタック・ディレクトリ構成・フェーズ計画・リスク（起点） |
| [FEATURE_SPEC.md](FEATURE_SPEC.md) | 機能仕様（表示／検索／タグ／住所録／カレンダー）・Tauri コマンド・セキュリティ・テスト・将来拡張 |
| [DATABASE_SCHEMA.md](DATABASE_SCHEMA.md) | SQLite スキーマ（テーブル・インデックス・FTS5） |
| [UI_UX_DESIGN.md](UI_UX_DESIGN.md) | UI/UX 設計（フレームレス全面ビジュアル・ダッシュボード／ウィジェットモード・サイドバー・チャット表示・カラー・a11y・ダークモード） |
| [DATA_STORAGE.md](DATA_STORAGE.md) | データ保存場所設計（OS 別パス・Rust 実装例・バックアップ） |
| [I18N.md](I18N.md) | 多言語対応（i18next / react-i18next） |

> プロジェクト全体の概要・技術スタック・開発コマンドは、リポジトリ直下の [`CLAUDE.md`](../CLAUDE.md) を参照。

---

最終更新日: 2026-06-30
