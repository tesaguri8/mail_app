# Google 連絡先同期（People API）

Rondine の住所録と Google 連絡先を同期する機能の設計・使い方。

**ステータス: 全 5 段とも実装済み（取り込み・照合・送信・ラベル同期）。連絡先モデルは 0059 で
作り直した（[CONTACT_MODEL.md](CONTACT_MODEL.md)）。** 取り込みは台帳止まりで、
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

0. **Google Cloud Console で People API を有効にする**（「API とサービス」→「ライブラリ」→
   `Google People API` → 有効にする）。[CALENDAR_SYNC.md](CALENDAR_SYNC.md) §1-2 で有効にするのは
   Calendar API だけなので、**連絡先を使うならこちらも要る**（有効にしないと取り込みが
   403 `SERVICE_DISABLED` で落ちる）。
1. **設定 →「アカウント」**の一番下「Google の接続設定（開発用）」で OAuth クライアント資格情報を
   保存（[CALENDAR_SYNC.md](CALENDAR_SYNC.md) §1。2026-10-09 に「同期」メニューを廃止し、
   アカウントに一本化した。docs/ACCOUNTS.md）。
2. Google のカードの **「連絡先」のスイッチをオン**にする（カードが無ければ「＋ アカウントを追加」）。
   - 連絡先の権限が無ければ Google でログインして足す（`include_granted_scopes=true` なので
     カレンダーの許可は失われない）。権限があればそのままオンにして同期する。
   - オフは同期を止めるだけ。つながりと取り込んだ連絡先は残る（外すのは「Google の連携を解除」）。
3. カードの **「今すぐ同期」** を押す（スイッチをオンにしたときは自動で 1 回走る）。カレンダーと一緒に、連絡先の
   **送信 → 取り込み → 住所録への反映**が 1 回で走る（2026-10-09 に一本化。以前は「連絡先を
   取り込む」「住所録へ反映」を別々に押し、反映の前に件数の下見を確認していた）。
   - 反映の規則: 同じ人と確かなら既存の連絡先へつなぐ、それ以外は新規として登録する。決めきれ
     なかった分は**新規として登録したうえで**「重複整理」に候補として出る（住所録 → 重複整理で、
     残す 1 件を選んで統合する）
   - **反映は Google へ何も送らない。**新しく起こした連絡先は Google から来たままなので送信待ちに
     しない。既存へつないだ連絡先も、通常の取り込みと同じ規則（Google が扱う項目は Google の値、
     Rondine にしか無い項目は残す。手元に未送信の変更があれば触らない）で取り込むだけにし、送信待ち
     にしない。Rondine にしか無い項目は、その人を利用者が編集したときに初めて送られる。
     同期のたびに自動で照合するので、確認なしに Google を書き換える経路を作らない（利用者の判断
     2026-10-09。以前は和集合にまとめて送り直していた）。どちらも単体テストで確かめている
4. 結果は「予定: … ｜ 連絡先: 取り込み N 件 / 新規 N 人 / 既存につないだ N 人 / 重複整理の候補 N 人 …」
   の 1 文で出る（0 の項目は省く）。
5. 以後は「今すぐ同期」と自動同期のたびに同じ流れが走る。自動同期で連絡先を同期するのは、起動
   直後と前回から 10 分以上たったとき（docs/CALENDAR_SYNC.md §2 の 4）。住所録側で編集・削除した
   連絡先は Google へ反映される。
   - **同期先は 1 人ずつ選ぶ**（2026-10-09。マイグレーション 0062）。どこから追加しても（新規作成・
     メールからの追加）、まず Rondine の連絡先として登録し、連絡先の詳細の **「同期先」** で
     チェックを入れたアカウントにだけ保存する（次の同期で作る。それまで印は「作成待ち」）。
     以前のアカウント単位のスイッチが、どこにもつながっていない連絡先を全部 Google に作って
     いた（メールの相手まで電話帳に流れ込む）のをやめた
   - カードの「連絡先」の下の **「新しく作る連絡先は、既定で Google にも保存する」**（`push_new_contacts`）は、
     新規作成の画面で最初からチェックを入れるかの既定にすぎない（外せる）
   - 同期をやめるときは同期先のチェックを外し、画面内で「Google 側の連絡先は残す（既定。
     つながりをすぐ外す）」か「Google 側の連絡先も削除する（次の同期で削除を送り、送れたら
     つながりを外す。それまで印は「削除待ち」）」を選ぶ。どちらでも Rondine の連絡先は残る

> **連携の解除**（docs/CALENDAR_SYNC.md §2 の 5）: 一時的な解除ではつながり（`contact_identities`）も
> 未送信の印も残り、住所録の同期先の印は薄く「解除中」と出る。同じアカウントで再接続すると、
> つながりと増分同期トークンがそのまま生き、未送信の変更は再接続後の同期で送る。完全に解除すると
> そのアカウントのつながり（とラベルの対応 `contact_group_identities`）を外し、連絡先は Rondine の
> みとして残す。本体の `dirty` は残ったつながりの分だけにそろえる（外したアカウント宛ての変更が、
> ほかのアカウントへ新規作成として流れないように）。

> 同意画面で連絡先だけ許可を外すこともできる。その場合はトークン応答の実スコープを見て
> `sync_contacts` を立てないので、同期はカレンダーだけになる。

> **`[要注意]` 初めて実アカウントで試すときは、書き込みがあることを踏まえて始める。**
> この機能は Google の連絡先を**更新・削除・作成**し、ラベルも作る。バックアップ
> （contacts.google.com → エクスポート）を取るか、連絡先を数件だけ入れた**試験用の
> Google アカウント**で始めるのが安全。最初の 1 周は同期先にチェックを入れず、取り込み → 住所録への
> 反映だけを確かめるとよい。

---

## 3. 設計（実装メモ）

### 3-1. モジュール構成

```
services/google/contacts.rs              API ベース URL・personFields / updatePersonFields
services/google/contacts/api.rs          People API v1 の薄いラッパー（people.get を含む）
services/google/contacts/incoming.rs     Person → 連絡先の中身（ContactFields）
services/google/contacts/outgoing.rs     連絡先の中身 → 書き込み本文（読み直した Person を土台に上書き）
services/google/contacts/sync.rs         同期エンジン（push → pull ＋ ラベル・スター）
services/store/contact_sync.rs           つながり表の操作（取り込み・照合の保存・送信の下ごしらえ）
services/store/contact_groups.rs         ラベル（contactGroups）とタグ名の対応表
services/contact_match.rs                照合の判定（DB も API も見ない）
services/contact_fields.rs               中身どうしの合成（取り込みの上書き・統合の和集合）
services/contact_labels.rs               ラベルの語彙（vCard / iCloud / Google の種別 ⇄ 表記）
```

中間表現は **vCard / Google CSV の取り込みと同じ `models::ContactFields`**（境界型の連絡先の
中身そのもの）を使う。同じ型に落としておけば、照合も保存も取り込み元を問わず同じ道を通る。

### 3-2. データモデル（マイグレーション 0059）

連絡先の表は 0059 で作り直した（[CONTACT_MODEL.md](CONTACT_MODEL.md)）。つながり表
`contact_identities` は 1 人の連絡先に複数のサービス・アカウントの行が並んでよい。

| 列 | 意味 |
|---|---|
| `provider` / `account_id` / `external_id` | どのアカウントの、どの `resourceName` か（UNIQUE） |
| `contact_id` | 紐付いたローカル連絡先。**NULL＝未照合** |
| `etag` | 取り込んだ版の etag（送信は送る直前に読み直した版の etag を使う） |
| `snapshot` | 取り込んだ内容（`ContactFields` の JSON）。照合の材料 |
| `dirty` | **このつながりへ**未送信のローカル変更がある |

`contact_id` は `ON DELETE SET NULL`。ローカル連絡先を消しても台帳は未照合として残る。
`contacts.dirty` は「つながりの `dirty` のどれかが 1（または未連携の新規）」に保つ。
`google_accounts.push_new_contacts` はローカル生まれの連絡先を Google にも作るか（既定オフ）。
`contact_group_identities` は Google のラベル ID ⇄ 名前の対応表（§3-7）。

増分同期トークンは **アカウント単位**（`google_accounts.contacts_sync_token`）。カレンダーが
カレンダー単位で持つのとは異なる。0059 で全アカウントのトークンを消したので、次の同期は
フル同期になる。

### 3-3. 取り込みアルゴリズム（`contacts/sync.rs`）

1. 連絡先グループ（ラベル）を引いて ID → 名前の対応を作る。システムグループ（`myContacts` 等）は除く。
   **失敗しても取り込みは続ける**（タグが付かないだけ）。
2. `people/me/connections` をページングしながら取得。`syncToken` があれば増分、無ければフル。
   フル・増分どちらでも `requestSyncToken=true` を付け、最終ページで次回用トークンを保存する。
3. `metadata.deleted` の連絡先は**そのつながりだけを外す**（台帳の行を消す）。ローカルの連絡先と、
   他のサービス・アカウントとのつながりは残す（削除を連鎖させない）。
4. 名前もメールも電話も無い Person は連絡先として成立しないので飛ばす（`skipped`）。
5. **トークン失効**（410 または本文の `EXPIRED_SYNC_TOKEN`）はトークンを捨ててフル同期へ。
   台帳は upsert なので再適用は安全。

### 3-4. フィールド対応

| Google (People API) | Rondine |
|---|---|
| `names[primary]` の displayName / honorificPrefix / familyName / middleName / givenName / honorificSuffix | `display_name` / `name_prefix` / `family_name` / `middle_name` / `given_name` / `name_suffix` |
| `phoneticFamilyName` / `phoneticMiddleName` / `phoneticGivenName` | `phonetic_family` / `phonetic_middle` / `phonetic_given` |
| `nicknames[0]` | `nickname` |
| `emailAddresses` / `phoneNumbers` | `contact_emails` / `contact_phones`（ラベル付き複数） |
| `addresses` | `contact_addresses`（構造化・私書箱・国コード） |
| `organizations`（複数） | `contact_organizations`（名前・よみ・役職・部署） |
| `birthdays[0]` | `birthday`（年が無ければ vCard 4.0 と同じ `--MM-DD`） |
| `biographies[0]` | `note` |
| `urls` / `events` / `relations` | `contact_urls` / `contact_dates` / `contact_relations` |
| `imClients` | `contact_handles`（`kind = im`） |
| `userDefined` | `contact_custom_fields` |
| `memberships` → contactGroup | タグ（ユーザーのラベル）／お気に入り（システムグループ `starred`） |

ラベルの語彙は `services::contact_labels` の表に揃える（`home`→自宅 / `work`→職場 / `mobile`→携帯 /
`*Fax`→FAX / `main`→代表 / `homePage`→ホームページ / `anniversary`→記念日 / `spouse`→配偶者 …）。
Google の既定値 `other` は無ラベル、ユーザーのカスタム名はそのまま使う。

**取らないもの**: 写真（後続）。

**送らないもの（Rondine 固有）**: `is_business` / `allow_remote_images` / 共有の印（`is_shared`。
値そのものは送る）/ 組織カードへのつながり（代表電話・FAX・代表メール）/ 旧姓・会社として表示・
SNS のハンドル（Google に対応する項目が無い）。取り込みでも、これらは Rondine 側の値を残す。

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

- **1 人のローカル連絡先を、同じアカウントの 2 つの外部 ID が掴まない。**Google 側に同じ人の
  重複があっても、紐付くのは先の 1 件だけ（送信フェーズで宛先が定まらなくなるのを防ぐ）。
- **統合すると、Google 側も 1 件にまとめる**（2026-10-09。利用者の要望）。`merge_contacts` は
  消える側の `contact_identities` を残す側へ付け替える（付け替えないと `ON DELETE SET NULL` で
  紐付けが外れ、次の同期で同じ人がもう一度新規として起こされる）。そのうえで、同じアカウントの
  ID が複数になるときは**1 つだけ残し**（残す側がもともと持っていた ID を優先、無ければ最初の
  1 つ）、余りを削除待ち（`unlink_requested`）にする。次の同期の送信が、同期先を外すときと同じ
  経路で Google 側を削除し、送れたらつながりを片付ける（§3-6）。残した 1 件には統合後の内容を送る。
  - 解除中のアカウントの ID は削除待ちにしない（解除中は送らない作法に合わせる）。
  - 選び方は `store::contact_dedupe::merge_remote` の 1 つの関数にまとめ、統合の確認画面の件数
    （`contact_merge_preview`）・統合・下の片付けが同じ関数を通る。
  - 画面: 重複の整理の「１つに統合」は、Google 側から消すものがあるときだけ確認を挟む
    （「Google 連絡先からも N 件削除して 1 件にまとめます（アカウント名）」と、Google のゴミ箱から
    30 日は戻せる旨）。
- **この取り決めより前に統合した人の片付け。**以前の統合で寄せた人には、同じアカウントの ID が
  2 つ以上ぶら下がったまま残っている。重複の整理の左上に「Google 連絡先に重複が N 件残って
  います。まとめますか？」を出し（`contact_google_duplicates`）、了承したら統合と同じ規則で
  片付ける（`contact_google_duplicates_tidy`）。黙って消さない。

コマンドは 2 本。`gcontacts_match_preview`（読み取りだけ・件数を返す）と
`gcontacts_match_apply`（1 トランザクションで紐付け＋新規作成）。同じ計画関数を通るので、
下見と実行で結果がずれない。

### 3-6. 送信（`contacts/sync.rs` の `push_contacts`）

カレンダーと同じ **push → pull** の順。ローカルの変更を先に送ってから取り込むことで、双方の
状態が収束する。

**未送信の印はつながりごと**（`contact_identities.dirty`）。利用者の操作（`upsert_contact` /
`delete_contact` / `restore_contact` / `merge_contacts` / 組織カードの改名 / vCard・CSV 取り込み）で
その連絡先の**全つながり**に 1 が立ち、そのつながりへ送れた時点で 0 に戻る。2 つのアカウント
（や将来の iCloud）につながった人は、それぞれへ送る。取り込み（`apply_remote_contact`）は印を立てない。

送るものは 3 種類。

| 対象 | 動作 |
|---|---|
| このアカウントへのつながりが未送信の連絡先 | 更新（`people/*:updateContact`）／論理削除なら削除（`:deleteContact`）してつながりを外す |
| 同期をやめて向こうも消すつながり・統合で余ったつながり（`contact_identities.unlink_requested`） | 削除（`:deleteContact`）してつながりを外す。連絡先は Rondine に残る |
| このアカウントへの作成待ち（`contact_create_requests`） | 作成（`people:createContact`）。作れたらつながりを作り、作成待ちを消す。ゴミ箱の人は作らない |

**どこにもつながっていない連絡先を勝手に作ることはしない。**作成待ちは利用者が同期先を選んだ
ときだけ置かれる（`store::contact_targets`）。作成待ちを別の表に置くのは、`contact_identities`
の `external_id` が NOT NULL で、読み手の多く（印・照合・外す処理）が「つながり＝向こうの ID が
ある」前提で書かれているため。主キー（連絡先・サービス・アカウント）で同じ人・同じアカウントの
作成待ちが二重にならず、既につながっているアカウントには置かない（統合で寄せたときも落とす）。
`push_new_contacts`（`google_accounts` の列・既定 0）は新規作成の画面の既定のチェックにだけ効く。

**取り込みは未送信の変更を潰さない。** `apply_remote_contact` は、紐付いたローカル連絡先が
`dirty = 0` のときだけ、Google が扱う項目を Google の正本で置き換える。push に失敗して残った
変更を取り込みが消してしまわないための条件。

**送る直前に読み直す。** 更新は `people.get` で読み直した Person を土台に、Rondine が扱う部分だけを
上書きして送る（`contacts/outgoing.rs`。[CONTACT_MODEL.md](CONTACT_MODEL.md) §3・§8）。Rondine が
知らない項目・付帯情報（会社の `location` など）を消さないため。etag は読み直した版のものを使う
（結果として後勝ち＝ローカルの変更が Google 側の変更を上書きする）。読み直しから送信までの間に
Google 側が変わると `ApiError::EtagConflict` が返り、**送らずに未送信のまま残す**（件数は同期結果の
`conflicts`）。

**`updatePersonFields` の意味に注意。** 挙げた項目は**本文に無ければ Google 側で消える**。
そこで送信本文は空でもキーを必ず入れる（ローカルで消した項目が Google 側でも消えるように）。
写真は挙げないので保持される。`memberships`（ラベル・スター）は `updateContact` では変えられない
ので挙げず、§3-7 の `members:modify` で送る。

**二重送信の防止。** 同じアカウントの同期が重なると同一の未送信連絡先を二重に作成しうるので、
送信はプロセス全体で直列化する（`push_lock`。カレンダーの同名ロックと同じ理由）。

### 3-7. ラベル同期（contactGroups ⇄ タグ）

Google の「ラベル」（contactGroups）と Rondine のタグ（`tags` / `contact_tags`）を双方向に
合わせる。Rondine のタグはメールと共通（`tags.name` が一意）なので、**連絡先に付いたタグだけ**が
対象になる。

**対応表を持つ**（マイグレーション 0058 の `contact_group_identities`）。People API はグループを
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

差分の基準（「Google に付いている所属」）は**送る直前に読み直した Person** の所属を使う。

**お気に入り（スター）も同じ道で送る。**システムグループ `contactGroups/starred` の所属を
`is_favorite` に合わせる（取り込みでは `starred` の所属をお気に入りにする）。

> **所属は `people:updateContact` では変えられない。**`updatePersonFields` に `memberships` を
> 挙げても通らないので、`contactGroups/*/members:modify` を使う。`WRITE_PERSON_FIELDS` に
> `memberships` を入れていないのはこのため（入れると Google 側のラベル分けを消す）。

**システムグループ（`myContacts` 等）はタグにしない。**意味が無いので一覧から除いている
（`starred` だけはお気に入りとして扱う）。


---

## 4. 既知の制限（v1）

- **実 API での往復が未検証**（上記 `[要確認]`）。とくに `names` は People API の `displayName`
  が読み取り専用なので、姓名に分けて送っている。表示名しか持たない連絡先は**表示名を姓に**
  入れており、Google 側の見え方は実アカウントで確かめる必要がある。
- **競合は後勝ち。**フィールド単位のマージも競合 UI も無い（カレンダーと同じ）。
- **電話の正規化は日本前提の簡易実装**（`dedupe::mobile_number` は 070/080/090 の 11 桁と
  `+81` だけを扱う）。国際的な連絡先を強く突き合わせるなら E.164 正規化が要る。現状は
  「迷ったら紐付けない」側に倒れるだけなので実害は小さい。
- **Rondine 固有の項目だけを変えても送信対象になる。**取引先フラグや外部画像許可を切り替えると
  `dirty` が立ち、次の同期で（内容は変わらないのに）1 回更新が飛ぶ。害は無いが無駄ではある。
- **統合で Google 側の余りを消すのは、次の同期で送れたとき。**送れるまでは印が「削除待ち」の
  まま残る。解除中のアカウントの余りは消さないので、再接続後も同じアカウントの ID が複数残る
  （重複の整理の「まとめますか？」で片付けられる）。
- **ラベル・スターの送信に失敗すると、その変更は次の取り込みで巻き戻る。**本体の送信が通った
  時点で `dirty` が落ちるため、ラベルだけ失敗しても再送されない。直後の取り込みが Google の所属を
  正としてローカルのタグ・お気に入りを戻す。稀だが、起きたときは付け直しが要る。
- **写真は同期しない**（取得も送信もしない＝ Google 側で保持。後続）。
- **2 つ以上のサービスにつながった人は、最後に取り込んだサービスの値になる。**取り込みは
  そのサービスが扱う項目をそのサービスの正本で置き換えるため（後勝ち）。

> **`otherContacts`（Gmail から自動収集された連絡先）は同期しない。** 件数が膨大でノイズになる。

---

## 5. 製品化時の注意

`https://www.googleapis.com/auth/contacts` は **sensitive scope**（Gmail のような restricted scope
ではない）ため、第三者セキュリティ審査（CASA）は不要で OAuth 同意画面の verification のみで済む
見込み。ただし**プライバシーポリシー URL・ドメイン所有確認・デモ動画**が要る。

> スコープ分類と審査要件は Google のポリシー変更が頻繁な領域なので、製品化に踏み切る時点で
> 最新の要件を再確認すること。
