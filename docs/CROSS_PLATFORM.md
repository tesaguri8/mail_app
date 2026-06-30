# クロスプラットフォーム / モバイル版 設計

**ステータス:** 計画（実装未着手）
**方針（確定）:**
- **デスクトップ = Tauri 2（Rust コア）**／**モバイル = Expo / React Native**（Primadoc 流）
- **メール同期は各端末が IMAP で独立同期**（IMAP サーバーを真実の源とするローカルファースト。共有バックエンドは持たない）

関連: [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) / [THREADING.md](THREADING.md) / [DATABASE_SCHEMA.md](DATABASE_SCHEMA.md) / [I18N.md](I18N.md)

---

## 1. 全体構成

```
┌──────────────────────────┐      ┌──────────────────────────┐
│  デスクトップ (Tauri 2)   │      │  モバイル (Expo / RN)     │
│  React レンダラー (TS)     │      │  React Native (TS)        │
│  Rust コア:               │      │  JS/Expo:                 │
│   IMAP/SMTP・MIME解析      │      │   IMAP/SMTP(JSライブラリ) │
│   暗号化・SQLite(FTS5)     │      │   expo-sqlite(FTS5)       │
│   スレッド再構築           │      │   expo-secure-store       │
└──────────┬───────────────┘      └──────────┬───────────────┘
           │  各自で直接                       │  各自で直接
           ▼                                  ▼
        ┌───────────────────────────────────────┐
        │   メールサーバー (IMAP/SMTP) = 真実の源 │
        └───────────────────────────────────────┘
   ※ 既読/フラグ等は IMAP 経由で自然に端末間共有される
```

- 共有バックエンドは設けない（SNS 統合の中継のみ後続で別途。[SNS_INTEGRATION.md](SNS_INTEGRATION.md)）。
- 既読・フラグ・フォルダ等は **IMAP サーバー側の状態**として端末間で揃う。

---

## 2. モバイルのスタック（Primadoc 同等）

| 項目 | 採用 |
|---|---|
| フレームワーク | **Expo（React Native）** |
| 言語 | TypeScript |
| ナビゲーション | React Navigation（bottom-tabs / native） |
| 状態管理 | Zustand（デスクトップと共通ライブラリ） |
| 資格情報 | `expo-secure-store`（OS セキュア領域） |
| ファイル/画像 | `expo-file-system` / `expo-image-picker`（背景画像取り込み） |
| ローカル DB | `expo-sqlite`（FTS5 対応） |
| 配信 | EAS（`eas.json`） |

> デスクトップ（Tauri）とモバイル（Expo）は**別アプリ**。UI 層は React と React Native で別だが、**ロジック・型・i18n は共有**する（次節）。

> **識別子（identifier）**: 規則 `tesaguri.<app_name>.app`（暫定 `tesaguri.comfortmail.dev`）。Expo の `ios.bundleIdentifier` / `android.package` も同値に揃える（[DATA_STORAGE.md](DATA_STORAGE.md)）。

---

## 3. コード共有（packages/ ワークスペース）

Primadoc 同様、npm workspaces の `packages/` で**プラットフォーム非依存の TS** を共有する。

| 共有パッケージ（案） | 内容 |
|---|---|
| `packages/mail-core` | **引用解析・スレッド再構築アルゴリズム**（[THREADING.md](THREADING.md)）、共通スキーマ、正規化ロジック |
| `packages/types` | 境界型（デスクトップは ts-rs 生成と整合） |
| `packages/i18n` | 翻訳リソース（[I18N.md](I18N.md)） |
| `packages/utils` | 日付・整形などの共通関数 |

### Rust コアの扱い（最重要の論点）

モバイルは Expo のため **Rust コアを直接実行できない**。スレッド再構築という“目玉”を二重実装しないために、**引用解析・スレッド判定アルゴリズムは TypeScript の `packages/mail-core` に置いて両プラットフォームで共有**する方針とする。

- **モバイル**: `mail-core`（TS）＋ JS の IMAP/MIME ライブラリ＋ `expo-sqlite` で完結。
- **デスクトップ**: I/O・暗号化・大量処理は引き続き **Rust**（IMAP/SMTP・SQLCipher・FTS5）。スレッド再構築アルゴリズムは、(a) `mail-core`(TS) をレンダラーで使う／(b) Rust に実装する、のどちらか。**重複と挙動差を避けるため (a) を推奨**（Rust 側は I/O と暗号・索引に専念）。
- ※ この (a)/(b) は実装時の確定事項。本書では「アルゴリズムは共有 TS が望ましい」を方針とする。

---

## 4. 端末間同期の考え方（独立 IMAP）と、その限界

- **メール本体・既読・フラグ**: IMAP サーバーが源。各端末が独立に同期すれば自然に揃う。サーバー不要。同期範囲（期間）は端末ごとにユーザーが選択（[SYNC.md](SYNC.md)）。
- **アプリ独自データは端末ローカル**（IMAP に載らない）:
  - 論理スレッドの割当・**手動の分割/結合/再件名**、タグ、AI 注釈、背景設定 など。
  - これらは**既定では端末間で共有されない**。
- **対処方針**:
  1. **決定的な再計算**: 同じ受信データから同じ `mail-core` で計算すれば、論理スレッドは各端末でほぼ一致する（自動部分は揃いやすい）。
  2. **手動上書き等のユーザー操作の同期は将来オプション**: Primadoc の同期基盤（WebDAV/Nextcloud 等）の考え方を流用し、**任意でクラウド同期**（後続）。必須にはしない。

> まずは「メールは IMAP で揃う／アプリ独自データは端末ローカル（自動部分は再計算で概ね一致）」で割り切る。ユーザー操作の端末間同期は後続の任意機能。

---

## 5. モバイル UI の方針

- ホームの思想（全面ビジュアル＋概要）は踏襲。**ウィジェット化**は OS のホーム画面ウィジェット（iOS/Android）として将来検討（デスクトップのリサイズ連動とは別実装）。
- ナビゲーションはボトムタブ（ホーム / メール / 住所録 / カレンダー）。
- 背景画像取り込みは `expo-image-picker` ＋ `expo-file-system`（アプリサンドボックス内に保存）。
- チャット形式表示・`clean_body` 表示は `mail-core` の解析結果を用いて RN で描画。

---

## 6. フェーズ上の位置づけ

- **デスクトップ（Tauri）のコアを先行**（[DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) Phase 1〜）。
- アルゴリズムを `packages/mail-core`(TS) に切り出す設計を、Phase 4（解析基盤）で意識する（モバイル再利用のため）。
- **モバイル版はコア安定後の別トラック**（Expo セットアップ → `mail-core` 結合 → IMAP 同期 → UI）。SNS 同様、基本機能の安定を優先。

---

## 7. リスク・留意点

| 項目 | 内容 | 対応 |
|---|---|---|
| Rust/TS 二重実装 | スレッド解析をデスクトップ Rust と切り離すと重複 | アルゴリズムを `mail-core`(TS) に集約し共有（§3） |
| モバイルの IMAP | RN での IMAP/MIME は実装負荷 | 実績ある JS ライブラリを採用、Phase で PoC |
| アプリ独自データの不一致 | 手動操作が端末間で揃わない | 決定的再計算＋将来の任意クラウド同期（§4） |
| UI 二重保守 | React と React Native で別 UI | ロジック/型/i18n を共有し、UI のみ各最適化 |
