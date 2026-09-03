# Google 連絡先同期（People API）

Rondine の住所録と Google 連絡先を同期する機能の設計・使い方。

**ステータス: 全 5 段とも実装済み（取り込み・照合・送信・ラベル同期）。** 取り込みは台帳止まりで、
そこから住所録へ入れるのは**照合**の役目（利用者が「住所録へ反映」を押したとき）。以後は同期の
たびに **push → pull** が走る。

> **`[要確認]` 実 API での往復はまだ確かめていない。** 開発機（raytrek）に有効な Google 資格情報が
> 無いため、送信は単体テスト（本文の組み立て・台帳の更新）までしか通していない。実アカウントを
> 繋いだ確認が要る（とくに `names` の書き込み可否、カスタム種別の受け付け、
> `contactGroups/*/members:modify` の挙動）。

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
   ▲       │ pull
   │       ▼
   │  contact_identities   ← 取り込んだ内容をそのまま保持（contact_id は NULL＝未照合）
   │       │ 照合（§3-5）
   │       ▼
   └───── contacts         ← 既存の連絡先に紐付ける／新規として起こす
     push（§3-6。dirty=1 のもの）
```

照合の物差しは重複検出（`services::dedupe`。氏名の Jaro-Winkler・電話の数字化と携帯/固定
振り分け・組織名正規化）**そのもの**を使う。同じ規則で見ているので、照合で決めきれずに新規と
して起こした連絡先は、既存の「重複整理」がそのまま候補に出す。

---

## 2. 使い方

1. **設定 →「Google カレンダー」**で OAuth クライアント資格情報を保存（[CALENDAR_SYNC.md](CALENDAR_SYNC.md) §1）。
2. **「連絡先も同期する（People API）」にチェック**を入れてから **「Google アカウントを連携」**。
   - 既にカレンダーだけで連携済みの場合も、チェックを入れて連携し直せば連絡先の権限が足される
     （`include_granted_scopes=true` なのでカレンダーの許可は失われない）。
3. 連携中アカウントの **「連絡先を取り込む」** を押す。
4. 結果に「取り込み N 件 / 削除 N 件 / 対象外 N 件（うち未照合 N 件）」が出る。
   **未照合の件数が、次の「住所録へ反映」の対象数**。
5. **「住所録へ反映」** を押す。件数の下見（何件を既存に紐付け、何件を新しく登録するか）が
   確認ダイアログに出るので、よければ実行する。
   - 決めきれなかった分は**新規として登録したうえで**「重複整理」に候補として出る。
     住所録 → 重複整理で、残す 1 件を選んで統合する。
6. 以後は「連絡先を取り込む」を押すたびに **送信 → 取り込み** が走る。住所録側で編集・削除した
   連絡先は Google へ反映される。
   - **Rondine で新しく作った連絡先は、既定では Google へ送らない。**送りたい場合は
     アカウント行の **「Rondine で作った連絡先も Google に作る」** にチェックを入れる。
     住所録を Google へ上げるかどうかは利用者が決めることなので、既定はオフにしてある。

> 同意画面で連絡先だけ許可を外すこともできる。その場合はトークン応答の実スコープを見て
> `sync_contacts` を立てないので、「連絡先を取り込む」ボタン自体が出ない。

---

## 3. 設計（実装メモ）

### 3-1. モジュール構成

```
services/google/contacts.rs              API ベース URL・personFields
services/google/contacts/api.rs          People API v1 の薄いラッパー
services/google/contacts/convert.rs      Person ⇄ Rondine の連絡先（取り込み・送信の両方向）
services/google/contacts/sync.rs         同期エンジン（push → pull ＋ ラベル）
services/store/contact_sync.rs           台帳の操作（取り込み・照合の保存・送信の下ごしらえ）
services/contact_match.rs                照合の判定（DB も API も見ない）
```

中間表現は **vCard / Google CSV の取り込みと同じ `vcard::ImportedContact`** を使う。同じ型に
落としておけば、照合も保存も取り込み元を問わず同じ道を通る。

### 3-2. データモデル（マイグレーション 0054〜0056）

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

0055 は送信のための列 —— `contacts.dirty`（未送信のローカル変更）と
`google_accounts.push_new_contacts`（ローカル生まれの連絡先を Google にも作るか。既定オフ）。
0056 は `contact_group_identities`（Google のラベル ID ⇄ 名前の対応表。§3-7）。

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

### 3-5. 照合（`services/contact_match.rs` ＋ `store/contact_sync.rs`）

判定は `services::contact_match::plan()` に閉じており、**DB にも People API にも触らない**
（保存は `store::contact_sync`）。同期エンジンに混ぜていないのは、照合の規則だけを後から
差し替えられるようにするため。

台帳の未照合 1 件ごとに、住所録の全員と突き合わせて次を決める。

| 状況 | 扱い |
|---|---|
| 高確信（携帯 or メールの一致＋氏名一致）の候補が**ただ 1 件** | その連絡先に紐付ける（住所録は増えない） |
| 高確信の候補が**複数** | 紐付けず**新規として起こす**（どちらに寄せるかは機械には決められない） |
| 確信が弱い（同名だけ・固定電話だけ 等） | 同上。**要確認**として数える |
| 候補なし | 新規として起こす |

安全側に倒す方針: **迷ったら紐付けない**。紐付けの誤りは別人の連絡先を混ぜてしまい取り返しが
つかないが、余分に起こした行は重複整理で畳めるため。

さらに 2 つの取り決めがある。

- **1 人のローカル連絡先を 2 つの外部 ID が掴まない。**Google 側に同じ人の重複があっても、
  紐付くのは先の 1 件だけ（送信フェーズで宛先が定まらなくなるのを防ぐ）。
- **統合しても紐付けは残る。**`merge_contacts` は消える側の `contact_identities` を残す側へ
  付け替える。付け替えないと `ON DELETE SET NULL` で紐付けが外れ、次の同期で同じ人がもう一度
  新規として起こされてしまう。

コマンドは 2 本。`gcontacts_match_preview`（読み取りだけ・件数を返す）と
`gcontacts_match_apply`（1 トランザクションで紐付け＋新規作成）。同じ計画関数を通るので、
下見と実行で結果がずれない。

### 3-6. 送信（`contacts/sync.rs` の `push_contacts`）

カレンダーと同じ **push → pull** の順。ローカルの変更を先に送ってから取り込むことで、双方の
状態が収束する。

**未送信の印は `contacts.dirty`**（マイグレーション 0055）。利用者の操作（`upsert_contact` /
`delete_contact` / `restore_contact` / `merge_contacts` / vCard・CSV 取り込み）で 1 が立ち、
送信に成功した時点で 0 に戻る。取り込み（`apply_remote_contact`）は `dirty` を立てない。

送るものは 2 種類。

| 対象 | 動作 |
|---|---|
| このアカウントの台帳を持つ連絡先 | 更新（`people/*:updateContact`）／論理削除なら削除（`:deleteContact`） |
| どの台帳にも無いローカル生まれの連絡先 | 作成（`people:createContact`）。**`push_new_contacts` が有効なときだけ** |

`push_new_contacts`（`google_accounts` の列・既定 0）は「Rondine で作った連絡先も Google に作る」
設定。住所録を Google へ上げるかは利用者が決めることなので、明示的に有効にしたときだけ送る。

**取り込みは未送信の変更を潰さない。** `apply_remote_contact` は、紐付いたローカル連絡先が
`dirty = 0` のときだけ Google の正本で上書きする。push に失敗して残った変更を取り込みが
消してしまわないための条件。Google 側で削除された連絡先も、`dirty = 0` のときだけローカルを
ゴミ箱へ落とす（完全削除はしない）。

**etag。** People API の更新は読んだ版の etag を要求する（`contact_identities.etag`）。古いと
`ApiError::EtagConflict` が返る。このとき**送らずに未送信のまま残す**: 続く取り込みで新しい
etag を受け取り、次回の送信で通る（結果として後勝ち＝ローカルの変更が Google 側の変更を
上書きする）。件数は同期結果の `conflicts` に出る。

**`updatePersonFields` の意味に注意。** 挙げた項目は**本文に無ければ Google 側で消える**。
そこで送信本文は空でもキーを必ず入れ（ローカルで消した項目が Google 側でも消えるように）、
Rondine が扱わない項目（写真・カスタム項目・関係）は**そもそも挙げない**ので保持される。
`memberships`（ラベル）も挙げない — ラベル同期は後続の段で、いま送ると Google 側のラベル分けを
消してしまう。

**二重送信の防止。** 同じアカウントの同期が重なると同一の未送信連絡先を二重に作成しうるので、
送信はプロセス全体で直列化する（`push_lock`。カレンダーの同名ロックと同じ理由）。

### 3-7. ラベル同期（contactGroups ⇄ タグ）

Google の「ラベル」（contactGroups）と Rondine のタグ（`tags` / `contact_tags`）を双方向に
合わせる。Rondine のタグはメールと共通（`tags.name` が一意）なので、**連絡先に付いたタグだけ**が
対象になる。

**対応表を持つ**（マイグレーション 0056 の `contact_group_identities`）。People API はグループを
`contactGroups/{id}` で指し、所属の変更も ID で行うため、名前だけでは足りない。取り込みのたびに
Google の一覧で**丸ごと洗い替える**（消えたラベルの行を残すと、そのタグが「Google の持ち物」と
誤判定されて次の取り込みで外れてしまう）。

**取り込み（Google → Rondine）。** 紐付いた連絡先のタグを、Google の所属に合わせる。ただし
**外す対象は対応表に載っている名前だけ**。載っていない名前（利用者がアプリ内だけで付けたタグ）は
Google 側の状態に関わらず触らない。これが無いと、Google を知らないタグが同期のたびに消える。

**送信（Rondine → Google）。** 連絡先本体の送信が通った後に、差分だけを送る。

| 差分 | 動作 |
|---|---|
| Rondine にあって Google に無い | ラベルが無ければ `contactGroups.create` で作り、`members:modify` で所属させる |
| Google にあって Rondine に無い | `members:modify` で外す |

差分の基準（「Google に付いている所属」）は**台帳の snapshot**（前回の取り込み時点）を使う。
送信は取り込みの前に走るので、これが Google の現在の状態にあたる。

> **所属は `people:updateContact` では変えられない。**`updatePersonFields` に `memberships` を
> 挙げても通らないので、`contactGroups/*/members:modify` を使う。`WRITE_PERSON_FIELDS` に
> `memberships` を入れていないのはこのため（入れると Google 側のラベル分けを消す）。

**システムグループ（`myContacts` 等）は対象外。**タグにしても意味が無いので一覧から除いている。


---

## 4. 既知の制限（v1）

- **実 API での往復が未検証**（上記 `[要確認]`）。とくに `names` は People API の `displayName`
  が読み取り専用なので、姓名に分けて送っている。表示名しか持たない連絡先は**表示名を姓に**
  入れており、Google 側の見え方は実アカウントで確かめる必要がある。
- **競合は後勝ち。**フィールド単位のマージも競合 UI も無い（カレンダーと同じ）。
- **Google 側で空にした項目が、ローカルでは消えない。**取り込みの更新は `COALESCE` で既存値を
  温存する（vCard 取り込みと同じ規則）。安全側だが、消去は伝わらない。
- **電話の正規化は日本前提の簡易実装**（`dedupe::mobile_number` は 070/080/090 の 11 桁と
  `+81` だけを扱う）。国際的な連絡先を強く突き合わせるなら E.164 正規化が要る。現状は
  「迷ったら紐付けない」側に倒れるだけなので実害は小さい。
- **Rondine 固有の項目だけを変えても送信対象になる。**取引先フラグや外部画像許可を切り替えると
  `dirty` が立ち、次の同期で（内容は変わらないのに）1 回更新が飛ぶ。害は無いが無駄ではある。
- **`[要注意]` 子テーブルに値を持たない連絡先を編集画面で保存すると、主値が消える。**
  住所録の編集画面はラベル付きの子テーブル（`contact_emails` 等）を編集するので、そこが空の
  連絡先を開くとメール欄が空に見え、保存すると `contacts.email` も消える。**push が入った今、
  この消去は Google 側にも伝わる。**Google 由来の連絡先は取り込み時に子テーブルが埋まるので
  通常は起きないが、フラット値だけを書く取り込み経路を足すときは注意する。
- **重複整理で統合しても、Google 側は 1 件にまとまらない。**統合は消える側の台帳を残す側へ
  付け替えるので、1 つのローカル連絡先に同じアカウントの外部 ID が複数ぶら下がりうる。この
  連絡先を編集すると、**Google 側の両方の連絡先が同じ内容に更新される**（消しはしない）。
  Google 側も 1 件に寄せたい場合は Google 連絡先側で統合する。
- **ラベルの送信に失敗すると、そのタグ変更は次の取り込みで巻き戻る。**本体の送信が通った時点で
  `dirty` が落ちるため、ラベルだけ失敗しても再送されない。直後の取り込みが Google の所属を
  正としてローカルのタグを戻す。稀だが、起きたときは付け直しが要る。
- **写真・カスタム項目・関係・チャットは同期しない**（取得も送信もしない＝ Google 側で保持）。

> **`otherContacts`（Gmail から自動収集された連絡先）は同期しない。** 件数が膨大でノイズになる。

---

## 5. 製品化時の注意

`https://www.googleapis.com/auth/contacts` は **sensitive scope**（Gmail のような restricted scope
ではない）ため、第三者セキュリティ審査（CASA）は不要で OAuth 同意画面の verification のみで済む
見込み。ただし**プライバシーポリシー URL・ドメイン所有確認・デモ動画**が要る。

> スコープ分類と審査要件は Google のポリシー変更が頻繁な領域なので、製品化に踏み切る時点で
> 最新の要件を再確認すること。
