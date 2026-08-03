# 作成・送信の実務機能

**ステータス:** 実装中（アルファ・0.1.0-alpha.1）。送信（`mail_send`）・下書き（`mail_save_draft` 等）・署名（`signature_*`）は実装済み。送信取消／予約／テンプレート／スヌーズ・添付サニタイズ配線は未実装（§1・§4 参照）。
**目的:** 安心して気持ちよく書いて送れる「現代のメール」の作法を揃える。

関連: [FEATURE_SPEC.md](FEATURE_SPEC.md)（作成モード）/ [PROTECTED_REGIONS.md](PROTECTED_REGIONS.md)（保護領域）/ [FILTERING.md](FILTERING.md)/ [FLY_SEND.md](FLY_SEND.md)（Fly 送信演出）

---

## 1. 機能

- **下書き自動保存**（**実装済み**）: 入力中つねに保存。下書きから再開。コマンドは §4 参照。
  - **ローカル保存は入力の 1 秒デバウンス、サーバー Drafts への APPEND は「明示保存」と「閉じる時」だけ**。入力のたびに APPEND すると、書いている間ずっとサーバー上で下書きの入れ替え（削除→APPEND）が走り、送信時の後片付けと競合してサーバーにゴミが残りやすい。
  - **破棄・送信後の下書きは「墓標（`deleted_keys`）」で復活を止める**。ローカル削除の直前に `canonical_key` を墓標として残し、(1) 取り込み側（`insert_email`）が墓標のあるキーを取り込まない、(2) サーバー側コピーの削除が済んでいない墓標（`remote_pending=1`）は同期のたびに再試行する（`imap_sync::retry_pending_draft_deletes`。同期の Pass 0）。
    - 背景: サーバー削除は best-effort で失敗しうるうえ、実行中の同期は「削除より前に取得した一覧」を持っているため、墓標が無いと **送信済みメールの下書きが下書きフォルダに溜まり続ける**（実測で 43 件中 30 件が送信済みの残骸）。
    - 墓標はサーバー削除が済んでから 30 日で掃除する（`services/store/tombstones.rs`）。
- **署名**（**実装済み・一部**）: アカウント別の署名（`signatures` テーブル＋`signature_*` コマンド。アカウントへは `accounts.signature_id` で紐付け）。※**返信時の挿入位置の設定は未実装**。
- **送信取消（Undo Send）**（**未実装**）: 送信後すぐの待機時間内なら取消（ローカル保留→実送信）。`mail_undo_send` は未実装。
- **送信予約（スケジュール送信）**（**未実装**）: 指定時刻に送信。`mail_schedule_send` は未実装。
- **テンプレート（定型文）**（**未実装**）: 再利用スニペット。`templates` テーブル・`template_*` コマンドとも未実装。**宿泊施設の問い合わせ定型返信**に有効なので後続で。
- **スヌーズ**（**未実装**）: メール/スレッドを指定時刻まで隠し再浮上。`mail_snooze`・`emails.snooze_until` とも未実装。
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

- `signatures` テーブル（**実装済み**）、下書きは `emails`（folder=draft）で管理（[DATABASE_SCHEMA.md](DATABASE_SCHEMA.md)）。破棄済み下書きの墓標は `deleted_keys`（0050。§1）。※`templates` テーブル・`emails.snooze_until` は**未実装**。
- 添付メタデータ削除ポリシーは設定ストア（`strip_attachment_metadata`: `all` | `gps` | `off`）で保持（配線は §3 のとおり保留）。

| コマンド | 用途 | 状態 |
|---|---|---|
| `mail_save_draft` / `mail_get_draft` | 下書きの保存・取得 | **実装済み** |
| `mail_draft_sync_remote` / `mail_draft_discard` | 下書きのリモート同期・破棄 | **実装済み** |
| `mail_send` | 送信 | **実装済み** |
| `signature_list` / `signature_create` / `signature_update` / `signature_delete` | 署名管理 | **実装済み** |
| `mail_undo_send` | 送信取消（遅延送出） | 未実装 |
| `mail_schedule_send` | 予約送信 | 未実装 |
| `attachment_sanitize` | 添付/インライン画像の EXIF/メタデータ削除（送信前） | 未実装（§3・Compose 添付対応後） |
| `attachment_inspect_metadata` | 添付のメタデータ（GPS 有無等）を検査して作成画面に提示 | 未実装（§3） |
| `template_*` | 定型文管理 | 未実装 |
| `mail_snooze` | スヌーズ設定 | 未実装 |
