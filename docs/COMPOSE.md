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

## 3. データモデル（要約） / コマンド

- `signatures`・`templates` テーブル、`emails.snooze_until`、下書きは `emails`（folder=draft）で管理（[DATABASE_SCHEMA.md](DATABASE_SCHEMA.md)）。

| コマンド | 用途 |
|---|---|
| `draft_save` / `draft_list` | 下書き保存・一覧 |
| `mail_send`（遅延付き）/ `mail_undo_send` | 送信・取消 |
| `mail_schedule_send` | 予約送信 |
| `signature_*` / `template_*` | 署名・定型文管理 |
| `mail_snooze` | スヌーズ設定 |
