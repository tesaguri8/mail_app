# インポート / エクスポート（移行・可搬性）

**ステータス:** 一部のみ実装（**連絡先のインポートとエクスポートは実装済み**。メールのインポート／エクスポートは未実装）。
**目的:** 既存メールから**乗り換えやすく**、いつでも**自分のデータを取り出せる**（データ所有権）。

> **実装状況（重要）**
> - ✅ **連絡先インポートのみ実装済み**: `contact_import(path)` が **vCard 3.0/4.0** と **Google CSV** を取り込む（`services/vcard.rs` / `services/gcsv.rs`、重複排除は `services/dedupe.rs`）。返り値は `ImportReport`。
> - 🚧 **メールのインポートは未実装（計画）**: `.eml` / `.mbox` / Thunderbird プロファイル / Outlook（.pst/.ost）取り込みコマンドは存在しない。
> - ✅ **連絡先エクスポート実装済み**: `contact_export(path, ids, version)` が **vCard 3.0（既定）/4.0** に書き出す（`services/vcard/write.rs`・`services/contact_export.rs`）。詳細は §2-1。
> - 🚧 **メールのエクスポートは未実装（計画）**: `.eml` / `.mbox` / JSONL いずれのメール書き出しコマンドも無い（`attachment_export` は「1 添付をディスク保存」する別機能で、メールのエクスポートではない）。
>
> 以下の §1〜§2 のメール取り込み・書き出し表は**設計案（未実装）**として読むこと。

関連: [ONBOARDING.md](ONBOARDING.md) / [THREADING.md](THREADING.md) / [SYNC.md](SYNC.md)

---

## 1. インポート（移行）

> **状態**: 連絡先（vCard/Google CSV）以外は**未実装（計画）**。下表のメール取り込みは設計案。

| ソース | 方式 |
|---|---|
| **.eml**（単一メール） | RFC822 をそのまま取り込み |
| **.mbox**（一括） | Thunderbird 等の標準。複数メールを順次取り込み |
| **Thunderbird プロファイル** | プロファイル内の mbox/Maildir を検出して取り込み |
| **Outlook** | `.pst`/`.ost` は構造が複雑 → ライブラリで解析、または「.eml/.mbox に書き出してから取り込み」を案内 |
| **IMAP→IMAP（任意）** | 旧アカウントに直接接続してコピー |

- 取り込み時に**既読/フラグ/フォルダ/日付を保持**し、ローカル schema に展開 → **スレッド再構築**（[THREADING.md](THREADING.md)）と FTS5 索引を実行。
- 大量取り込みはバックグラウンドで進捗表示。

---

## 2. エクスポート（可搬性・バックアップ）

> **状態**: 連絡先の vCard 書き出し（§2-1）以外は**未実装（計画）**。下表のメール書き出しは設計案。

### 2-1. 連絡先（vCard）— 実装済み

連絡先画面の「書き出し」（取り込みの隣）から、範囲と版を選び、保存先をダイアログで選ぶ。
既定のファイル名は `rondine-contacts-YYYYMMDD.vcf`（製品名は `config/app-identity.json` の slug）。

- **範囲**: 全員（ゴミ箱を除く）／いまの一覧（検索・タグ・同期先で絞り込んでいるときだけ選べる）
- **版**: vCard 3.0（既定。Google 連絡先・iPhone・Outlook が読める）／4.0。文字は UTF-8、改行は CRLF、75 オクテットで折り返す
- **見出し**: 自宅/職場/携帯/FAX/ポケベルは `TYPE`、ほか（代表・記念日・配偶者・カスタム名）は iCloud と同じ `itemN.X-ABLabel`（既知の語は `_$!<Main>!$_` 形式）
- **書き出す項目**: 氏名（N の 5 要素）・よみ（`X-PHONETIC-*`）・ニックネーム・旧姓・会社（2 つ目以降は `itemN.ORG`/`itemN.TITLE`）・会社として表示（`X-ABShowAs`）・メール・電話・住所（国コードは `itemN.X-ABADR`）・URL・誕生日・記念日など（`X-ABDATE`）・関係（`X-ABRELATEDNAMES`）・チャット（`IMPP`）・SNS（`X-SOCIALPROFILE`）・カスタム項目（`itemN.X-RONDINE-CUSTOM`。Rondine 以外では読まれない）・メモ・タグ（`CATEGORIES`）
- **年なしの日付**: 3.0 は iCloud の `X-APPLE-OMIT-YEAR=1604`、4.0 の誕生日は `--MMDD`
- **書き出さないもの**: Rondine 固有の印（お気に入り・取引先・外部画像の許可・共有の代表値）、写真、どのサービスとつながっているか
- **往復**: Rondine の取り込み（`vcard::parse`）と対になっており、取り込み→書き出し→取り込みで上の項目が戻る（`services/vcard/write/tests.rs`）
- 数千件でも画面を止めないよう、組み立てと書き込みは `spawn_blocking` で行い、書き出した件数を返す（`ContactExportReport`）

### 2-2. メール — 未実装（計画）

| 形式 | 用途 |
|---|---|
| **.eml / .mbox** | 標準・可搬。他クライアントへ移行可能 |
| **JSONL** | アプリ独自メタ込み（タグ・論理スレッド・AI注釈等）。再取込・解析用（[DATABASE_SCHEMA.md](DATABASE_SCHEMA.md) の JSON 方針） |

- 範囲指定（アカウント/スレッド/期間/フィルタ結果）でエクスポート。
- **データ所有権の明言**: ユーザーはいつでも標準形式で全データを取り出せる。

---

## 3. Tauri コマンド

実装済み:

| コマンド | 用途 |
|---|---|
| `contact_import(path)` | 連絡先取り込み。**vCard 3.0/4.0**（`vcard.rs`）と **Google CSV**（`gcsv.rs`）を拡張子/内容で判別し、重複排除（`dedupe.rs`）して取り込む。返り値 `ImportReport`。 |
| `contact_export(path, ids, version)` | 連絡先書き出し（§2-1）。`ids` が null ならゴミ箱を除く全員、`version` は `"3.0"` / `"4.0"`。返り値 `ContactExportReport`。 |

計画（未実装。コマンド自体が存在しない）:

| コマンド（案） | 用途 |
|---|---|
| `import_eml` / `import_mbox` | メールのファイル取り込み |
| `import_thunderbird` / `import_outlook` | プロファイル/PST 取り込み |
| `export_mbox` / `export_eml` / `export_jsonl` | メールのエクスポート（範囲指定） |
