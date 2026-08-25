# Google 連絡先同期（People API）

Rondine の住所録と Google 連絡先を同期する機能の設計・使い方。

**ステータス: 取り込み（pull）のみ実装済み。** 取り込んだ連絡先は**台帳に溜まるだけで、住所録には
まだ現れない**。照合（既存の連絡先と同一人物かの判定）と送信（push）は後続。

認証は Google カレンダーと**共通**（同じアカウント・同じ refresh_token）。連携まわりの詳細は
[CALENDAR_SYNC.md](CALENDAR_SYNC.md) §3-0／§3-1 を参照。

---

## 1. なぜ取り込みをそのまま住所録に入れないのか

初回同期では、Google 側の連絡先と Rondine の住所録に**同じ人が別 ID で二重に存在する**。そのまま
`contacts` へ入れると住所録が丸ごと二重になる。カレンダーの予定は独立した単位なのでこの問題が
起きなかったが、連絡先では避けられない。

そこで取り込みは `contact_identities`（台帳）で止める。

```
Google (People API)
      │ pull
      ▼
contact_identities   ← 取り込んだ内容をそのまま保持（contact_id は NULL＝未照合）
      │ 照合フェーズ（未実装）
      ▼
contacts             ← 既存の連絡先に紐付ける／新規として起こす
```

照合には既存の `services::dedupe`（氏名の Jaro-Winkler・電話の数字化と携帯/固定振り分け・
組織名正規化）を転用する予定。

---

## 2. 使い方

1. **設定 →「Google カレンダー」**で OAuth クライアント資格情報を保存（[CALENDAR_SYNC.md](CALENDAR_SYNC.md) §1）。
2. **「連絡先も同期する（People API）」にチェック**を入れてから **「Google アカウントを連携」**。
   - 既にカレンダーだけで連携済みの場合も、チェックを入れて連携し直せば連絡先の権限が足される
     （`include_granted_scopes=true` なのでカレンダーの許可は失われない）。
3. 連携中アカウントの **「連絡先を取り込む」** を押す。
4. 結果に「取り込み N 件 / 削除 N 件 / 対象外 N 件（うち未照合 N 件）」が出る。
   **未照合の件数が、次の照合フェーズの対象数**。

> 同意画面で連絡先だけ許可を外すこともできる。その場合はトークン応答の実スコープを見て
> `sync_contacts` を立てないので、「連絡先を取り込む」ボタン自体が出ない。

---

## 3. 設計（実装メモ）

### 3-1. モジュール構成

```
services/google/contacts.rs              API ベース URL・personFields
services/google/contacts/api.rs          People API v1 の薄いラッパー
services/google/contacts/convert.rs      Person → ImportedContact
services/google/contacts/sync.rs         取り込み（pull）エンジン
services/store/contact_sync.rs           台帳の操作（RemoteContact / ContactIdentity）
```

中間表現は **vCard / Google CSV の取り込みと同じ `vcard::ImportedContact`** を使う。同じ型に
落としておけば、照合も保存も取り込み元を問わず同じ道を通る。

### 3-2. データモデル（`migrations/0054_contact_identities.sql`）

`contact_identities` は 0017 が「提供元 ID の対応表は API 同期の実装時に追加する」と予告していた
もの。`contacts.external_id` は 1 プロバイダ分しか持てないので、同期の突き合わせはこちらを正とする。

| 列 | 意味 |
|---|---|
| `provider` / `account_id` / `external_id` | どのアカウントの、どの `resourceName` か（UNIQUE） |
| `contact_id` | 紐付いたローカル連絡先。**NULL＝未照合** |
| `etag` | People API の更新に**必須**（読んだ版の etag を送らないと弾かれる） |
| `snapshot` | 取り込んだ内容（`ImportedContact` の JSON）。照合と差分判定の材料 |
| `remote_deleted` | Google 側で削除された印 |

`contact_id` は `ON DELETE SET NULL`。ローカル連絡先を消しても台帳は未照合として残る。

増分同期トークンは **アカウント単位**（`google_accounts.contacts_sync_token`）。カレンダーが
カレンダー単位で持つのとは異なる。

### 3-3. 取り込みアルゴリズム（`contacts/sync.rs`）

1. 連絡先グループ（ラベル）を引いて ID → 名前の対応を作る。システムグループ（`myContacts` 等）は除く。
   **失敗しても取り込みは続ける**（タグが付かないだけ）。
2. `people/me/connections` をページングしながら取得。`syncToken` があれば増分、無ければフル。
   フル・増分どちらでも `requestSyncToken=true` を付け、最終ページで次回用トークンを保存する。
3. `metadata.deleted` の連絡先は台帳に**削除印を付けるだけ**（行は消さない）。ローカル連絡先を
   どうするかは送信フェーズの判断で、取り込みでは決めない。
4. 名前もメールも電話も無い Person は連絡先として成立しないので飛ばす（`skipped`）。
5. **トークン失効**（410 または本文の `EXPIRED_SYNC_TOKEN`）はトークンを捨ててフル同期へ。
   台帳は upsert なので再適用は安全。

### 3-4. フィールド対応

| Google (People API) | Rondine |
|---|---|
| `names[primary]` の displayName / familyName / givenName | `display_name` / `family_name` / `given_name` |
| `phoneticFamilyName` / `phoneticGivenName` | `phonetic_family` / `phonetic_given`（結合して `name_kana`） |
| `emailAddresses` / `phoneNumbers` | `contact_emails` / `contact_phones`（ラベル付き複数） |
| `addresses` | `contact_addresses`（構造化） |
| `organizations[primary]` | `organization` / `org_title` / `org_department` |
| `birthdays` | `birthday`（年が無ければ vCard 4.0 と同じ `--MM-DD`） |
| `biographies[0]` | `note` |
| `memberships` → contactGroup | タグ（`labels`） |

ラベルは vCard 取り込みと同じ語彙に揃える（`home`→自宅 / `work`→職場 / `mobile`→携帯 /
`*Fax`→FAX / `main`→代表）。Google の既定値 `other` は無ラベル、ユーザーのカスタム名はそのまま使う。

**取らないもの**: 写真・カスタム項目・関係・チャットなど。`personFields` に挙げていないので
取得せず、送信時も `updatePersonFields` に挙げなければ Google 側で保持される。

**送らないもの（Rondine 固有）**: `is_business` / `allow_remote_images` / 組織レコードへのリンク
（代表電話・FAX・代表メール）。Google 側に対応概念が無く、往復で落ちるため同期対象外とする。

---

## 4. 残っている段

| 段 | 内容 |
|---|---|
| 照合 | 台帳の未照合分を `dedupe` で既存連絡先と突き合わせ、高信頼は自動で紐付け、低信頼は重複整理 UI へ |
| 送信（push） | `contacts` の変更を Google へ。`dirty` 列＋ push→pull 順＋ etag 競合の検出 |
| ラベル同期 | contactGroups ⇄ Rondine のタグの双方向 |

> **`otherContacts`（Gmail から自動収集された連絡先）は同期しない。** 件数が膨大でノイズになる。

---

## 5. 製品化時の注意

`https://www.googleapis.com/auth/contacts` は **sensitive scope**（Gmail のような restricted scope
ではない）ため、第三者セキュリティ審査（CASA）は不要で OAuth 同意画面の verification のみで済む
見込み。ただし**プライバシーポリシー URL・ドメイン所有確認・デモ動画**が要る。

> スコープ分類と審査要件は Google のポリシー変更が頻繁な領域なので、製品化に踏み切る時点で
> 最新の要件を再確認すること。
