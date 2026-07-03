# 作成・送信の実務機能

**ステータス:** 計画（実装未着手）
**目的:** 安心して気持ちよく書いて送れる「現代のメール」の作法を揃える。

関連: [FEATURE_SPEC.md](FEATURE_SPEC.md)（作成モード）/ [PROTECTED_REGIONS.md](PROTECTED_REGIONS.md)（保護領域）/ [FILTERING.md](FILTERING.md)/ [FLY_SEND.md](FLY_SEND.md)（Fly 送信演出）

---

## 1. 機能

- **下書き自動保存**: 入力中つねに保存。下書き一覧から再開。クラッシュ耐性。
- **送信取消（Undo Send）**: 送信後すぐの**待機時間（既定 5–30 秒・設定可）**内なら取消。ローカルで保留してから実送信。
- **送信予約（スケジュール送信）**: 指定時刻に送信。ローカルファースト前提のため**アプリ起動時に送出**（未起動だと遅延する旨を明示）。
- **署名**: アカウント別の署名（プレーン/リッチ）。返信時の位置も設定。
- **テンプレート（定型文）**: 再利用スニペット。変数差し込み（宛名等）。**宿泊施設の問い合わせ定型返信**に有効。
- **スヌーズ**: メール/スレッドを指定時刻まで隠し、再浮上（受信箱の整理。[FILTERING.md](FILTERING.md) の要再確認と別軸）。
- **不在応答（バケーション）**: 任意・後続。

---

## 2. 作成フォーマット方針

- **Markdown で書いて送信時に HTML 化**（`multipart/alternative` で **plain + HTML 両方**を同梱）。受信側はどちらでも読める。
- Primadoc の Markdown パイプラインを `packages/`（[CROSS_PLATFORM.md](CROSS_PLATFORM.md)）で共有・流用。
- プレーン主体で書きたい人向けに**プレーンのみ送信**も選択可。

---

## 3. 送信時のプライバシー：添付画像の EXIF/メタデータ削除

**目的:** 送る側の情報漏れを防ぐ。写真の **EXIF には撮影位置（GPS）・日時・カメラ機種/シリアル**等が入り、受信者や中継サーバに意図せず渡る。既定で削って送る。関連: [MAIL_SECURITY.md](MAIL_SECURITY.md)（あちらは**受信**側の画像安全表示。本節は**送信**側）。**実装フェーズ:** 画像安全ロードマップの **Phase 2**（[MAIL_SECURITY.md](MAIL_SECURITY.md) §5）。受信側と独立で並行可・規模 M。`services/media.rs`（decode/re-encode 基盤・既存）に無劣化除去＋GPS検査を追加する。

- **既定オン**: 添付・インライン（cid:）画像を送信前にサニタイズ。設定でオフ可（原本のまま送りたいケース）。
- **方式は「メタデータ除去」優先、「再エンコード」はフォールバック**:
  - まず EXIF/XMP/IPTC セグメントだけを外す（**画素は無変換＝画質・サイズ劣化なし**）。Rust 側で `img-parts` 等を用い、JPEG/PNG/WebP のメタチャンクを除去。
  - 除去だけで安全化しにくい形式や不正データは、デコード→クリーン再エンコードで無害化（[MAIL_SECURITY.md](MAIL_SECURITY.md) §1.1 のサニタイズと同じ考え方の送信版）。
- **粒度**: GPS だけ消す / 全メタデータを消す、を選択可。既定は**全削除**。
- **可視化**: 添付に位置情報を検出したら作成画面に「📍 位置情報あり（送信時に削除）」を表示。1 通だけ原本送信も選べる。
- **処理場所はローカル（Rust）**。送信は lettre 経由のため**クラウド不要**でクライアント内完結。
- **範囲**: 画像を優先実装。PDF/Office 文書の作成者・GPS 等メタデータ除去は**後続**（形式ごとに難易度差）。

> **実装状況（2026-07）**: 中核ロジックは実装済み — `services/media.rs` に `strip_image_metadata`（JPEG は img-parts で EXIF を無劣化除去）と `image_has_gps`（kamadak-exif で GPS 検知）を追加、単体テスト付き。**ただし送信経路への配線は保留**：現状 Compose は**添付機能自体が未実装**（`OutgoingMessage`/`SendInput` に添付フィールドが無い）ため、剥がす対象の送信添付が存在しない。**前提として「Compose の添付対応」が必要**で、それが入った時点で `attachment_sanitize`/`attachment_inspect_metadata` コマンド・設定（`strip_attachment_metadata`）・Compose UI（📍表示）・`smtp.rs` の最終適用を配線する。

---

## 4. データモデル（要約） / コマンド

- `signatures`・`templates` テーブル、`emails.snooze_until`、下書きは `emails`（folder=draft）で管理（[DATABASE_SCHEMA.md](DATABASE_SCHEMA.md)）。
- 添付メタデータ削除ポリシーは設定ストア（`strip_attachment_metadata`: `all` | `gps` | `off`）で保持。

| コマンド | 用途 |
|---|---|
| `draft_save` / `draft_list` | 下書き保存・一覧 |
| `mail_send`（遅延付き）/ `mail_undo_send` | 送信・取消 |
| `mail_schedule_send` | 予約送信 |
| `attachment_sanitize` | 添付/インライン画像の EXIF/メタデータ削除（送信前） |
| `attachment_inspect_metadata` | 添付のメタデータ（GPS 有無等）を検査して作成画面に提示 |
| `signature_*` / `template_*` | 署名・定型文管理 |
| `mail_snooze` | スヌーズ設定 |
