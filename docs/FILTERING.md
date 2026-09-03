# フィルタリング / 絞り込み

**ステータス:** 計画（実装未着手）／コア機能
**目的:** 受信箱・統合インボックスを多観点（ファセット）で素早く絞り込み、見落としを防ぎ整理を助ける。

関連: [FEATURE_SPEC.md](FEATURE_SPEC.md)（タグ・スマートフォルダ）/ [THREADING.md](THREADING.md)（論理スレッド・ヘッダ活用）/ [DATABASE_SCHEMA.md](DATABASE_SCHEMA.md) / [AI_FEATURES.md](AI_FEATURES.md)（自動分類）

---

## 1. フィルタの観点（ファセット）と出所

| ファセット | 例 | 出所 / 判定 |
|---|---|---|
| **状態（ユーザー操作）** | ブックマーク、要再確認（フォローアップ）、フラグ、未読/既読、添付あり | メール/スレッドに付与する**フラグ**（`emails.is_bookmarked` / `needs_review` 等） |
| **相手：知り合い** | 住所録に存在する差出人 | 差出人アドレスが `contacts` に**一致**（導出） |
| **相手：返信歴** | やり取りしたことがある差出人 | 差出人アドレスが**送信履歴の索引**（`sent_addresses`）に**一致**（導出。実装済み） |
| **相手：取引実績** | 取引のある相手 | ①手動の「取引先」フラグ（`contacts.is_business`）②**双方向のやり取り履歴**（送信実績あり）から導出 |
| **相手：お気に入り/VIP** | 重要な相手 | `contacts.is_favorite` |
| **アドレスグループ** | 「社内」「常連ゲスト」等 | `contact_groups` 所属（住所録のグループ） |
| **カテゴリ** | 予約/請求/問い合わせ 等 | 分類ラベル（`tags.kind='category'`、手動 or AI 自動分類） |
| **タグ** | 任意ラベル | `tags`（複数付与可） |
| **種別** | メルマガ/一括配信、SNS チャネル、宛先エイリアス | ヘッダ（`List-Id`/`Precedence`/`Delivered-To`、[THREADING.md](THREADING.md) §7.5）、SNS は将来 |
| **期間・サイズ・全文** | 日付範囲、サイズ、キーワード | `date`/`size`、FTS5（`clean_body`） |

> 「知り合い」「取引実績」は**自動導出**を基本にしつつ、手動フラグ（取引先・VIP）で上書き・補強できる。

---

## 2. 「返信歴」「取引実績」の定義（自動導出 + 手動）

宿泊業などで「やり取りのある相手か」を素早く判別できるようにする。

### 2.1 返信歴あり（実装済み・フィルタ `replied`）

**定義: その差出人アドレスへ、自分から送ったことがある。** 受信箱の一覧に出ている時点で受信は
あるので、これがそのまま「双方向のやり取りがある相手」＝取引実績の自動導出になる。返信
（In-Reply-To 付き）に限定せず、**こちらから新規に出した相手も含める**（挨拶や見積もりを
こちらから出した相手を取りこぼさないため）。

- **索引**: `sent_addresses`（`address` 小文字 PK / `sent_count` / `last_sent_ts`）。
  送信済み（`folder='sent'`。ゴミ箱にある送信控え `prev_folder='sent'` も含む）メールの
  **To/Cc** に現れたアドレスを 1 行ずつ持つ。実装は `services/store/sent_addresses.rs`。
- **更新経路**: ①取り込み時（`insert_email` で `folder='sent'` のメール）②送信直後
  （`mail_send` が `record_sent_recipients`。送信控えの同期を待たずに反映）③初回起動時の
  一括構築（`ensure_built`。`app_settings` の `sent_index.built_version` で 1 度だけ）。
- **Bcc は記録しない**。送信控え（Sent への APPEND）に Bcc は残らず、索引を作り直すと消えて
  しまうため（結果が再構築のたびに変わるのを避ける）。
- **一覧での判定**: `EXISTS (SELECT 1 FROM sent_addresses WHERE address = lower(from_address))`。
  アドレス完全一致 1 回で済み、件数に依存しない（`emails.to_addresses` を LIKE で舐めない）。
- **制約**: 判定できるのは **Sent フォルダが手元に同期されている範囲**（アカウントの同期
  ウィンドウ。既定 6 ヶ月）。それ以前の送信歴は拾えない。より長く遡るには同期ウィンドウを
  広げるか、Sent だけ全期間スキャンする経路を別途足す。

### 2.2 取引実績（計画）

- **手動**: 連絡先に「取引先」フラグ（`contacts.is_business`）を付与すれば確実に扱える。
- 「知り合い（住所録に存在）」とは区別する（住所録にいても取引実績が無い場合がある）。
- やり取り回数の指標化には `sent_addresses.sent_count` / `last_sent_ts` を使える。

---

## 3. 状態フラグ（ユーザー操作）

| フラグ | 用途 | 保存 |
|---|---|---|
| ブックマーク | 後で見返す重要メール | `emails.is_bookmarked` |
| 要再確認（フォローアップ） | 対応・返信が必要。期限も任意 | `emails.needs_review` / `follow_up_at` |
| フラグ | 既存の重要マーク | `emails.is_flagged` |

- メール単位で付与し、**スレッド（論理スレッド）にも集約表示**（1通でもブックマークがあればスレッドに印）。
- ホーム/ウィジェットに「要再確認 N件」を表示し、取りこぼしを防ぐ。

---

## 4. 保存フィルタ（スマートフォルダ）

ファセットの組み合わせを**名前付きで保存**し、サイドバーに固定できる（既存「スマートフォルダ」と統合）。

- 例: 「**要対応**」= 要再確認 AND 未読 / 「**常連ゲスト**」= グループ=常連 AND 取引実績あり / 「**メルマガ**」= List-Id あり。
- 条件は AND/OR の組み合わせ。動的に評価（保存するのは条件であり、メールのコピーではない）。
- 可変構造のため条件は JSON で保持（`saved_filters.definition_json`。[AI_FEATURES.md](AI_FEATURES.md) §4 の JSON 方針に沿う）。

---

## 5. 自動付与（ルールエンジン連携）

既存の振り分けルール（[FEATURE_SPEC.md](FEATURE_SPEC.md) §2.3）でフラグ・カテゴリ・タグを自動付与:

- 差出人/件名/本文キーワード/`List-Id`/添付有無/時間帯 → カテゴリ付与・要再確認・ブックマーク等。
- **AI 自動分類**（[AI_FEATURES.md](AI_FEATURES.md)）でカテゴリ提案も可能（提案→人が確定）。

---

## 6. UI（ファセット絞り込み）

- 一覧上部/サイドにファセットパネル。各ファセットの件数（ファセットカウント）を表示。
- 複数ファセットの掛け合わせで即時絞り込み（FTS5＋インデックスで高速）。
- よく使う条件は保存フィルタ化してワンクリック。

---

## 7. データモデル（要約）

詳細は [DATABASE_SCHEMA.md](DATABASE_SCHEMA.md)。

- `emails` に追加: `is_bookmarked` / `needs_review` / `follow_up_at`
- `contacts` に追加: `is_business`（取引先。`is_favorite` は既存）
- `tags` に `kind`（`'tag' | 'category'`）を追加し、カテゴリをタグ機構で表現
- `saved_filters`（保存フィルタ。条件は `definition_json`）
- 「知り合い」はクエリで導出、「返信歴（＝自分から送った相手）」は `sent_addresses` 索引で導出

---

## 8. Tauri コマンド（抜粋）

実装済み（`src-tauri/src/commands.rs`）。状態フラグは一括操作（`Vec<i64>`）で受ける。

| コマンド | 用途 |
|---|---|
| `mail_set_bookmarked(ids, value)` | ブックマークの付与・解除 |
| `mail_set_starred(ids, value)` | スター（フラグ相当）の付与・解除 |
| `mail_set_read(ids, read)` | 既読/未読の切替 |
| `tag_list` / `tag_create` / `tag_update` / `tag_delete` / `tag_set_parent` | タグ・カテゴリ（`tags.kind` で区別）の管理・階層化 |
| `mail_add_tag(ids, tag_id)` / `mail_remove_tag(ids, tag_id)` | メールへのタグ付与・解除 |

> **未実装（計画）**: ファセット横断取得（`inbox_filter` 相当）、保存フィルタ（スマートフォルダ）の
> 読み書きコマンド（`saved_filters` テーブルは 0001 で存在するが**コマンド未配線**）、連絡先の
> 「取引先」専用トグルコマンドは未提供。現状の一覧取得は `mail_list` / `thread_list` /
> `mail_search`、連絡先の更新は `contact_upsert`（`is_business` を含む）で行う。
