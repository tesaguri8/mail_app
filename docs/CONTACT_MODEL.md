# 連絡先モデル（作り直し）

**ステータス:** 第 1 段（裏側: マイグレーション 0059・store 層・同期・取り込み）と第 2 段（画面: §2 のアイコン・
絞り込み、項目の追加、会社の複数化、§4-1 の整理）を実装済み（2026-10-08）。実装で設計から変えた点は §8。

## 0. 前提と方針

- **リリース前なので、表は綺麗に作り直す。**継ぎ足しの跡（本体の主値列と子テーブルの二重持ち、
  使われていない `contact_groups`）を残さない（利用者の判断 2026-10-08）。
- **個人の連絡先は一度すべて消し、同期で入れ直す。**Google は People API の同期で、iCloud は
  後日の CardDAV で入れ直す。お気に入り・タグ・メモなど Rondine で手入力したものは失われるが、
  利用者が了承済み（同日）。
- **組織カード（`organizations`）は残す。**代表電話・所在地などの手入力がある（vaio で 16 件中 2 件）。
  入れ直した連絡先は、会社名で組織カードへつなぎ直す（§4）。
- **項目は Google と iCloud の和集合を持つ。**各サービスには、そのサービスが扱える項目だけを送る。
- **1 人の連絡先が、複数のサービスと同時につながってよい。**「つながり表」に 1 行ずつ持つ（§3）。

## 1. 表

### 1-1. 本体 `contacts`（1 人 1 行。複数値は持たない）

| 列 | 意味 | Google | iCloud (vCard) |
|---|---|---|---|
| `display_name` | 表示名 | `names.displayName`（読み取り専用。姓名から組み立てる） | `FN` |
| `name_prefix` / `name_suffix` | 敬称 / 接尾辞 | `honorificPrefix` / `honorificSuffix` | `N` の 4・5 番目 |
| `family_name` / `middle_name` / `given_name` | 姓 / ミドル / 名 | `familyName` / `middleName` / `givenName` | `N` の 1〜3 番目 |
| `phonetic_family` / `phonetic_middle` / `phonetic_given` | よみ | `phonetic*Name` | `X-PHONETIC-*-NAME` |
| `nickname` | ニックネーム | `nicknames[0]` | `NICKNAME` |
| `maiden_name` | 旧姓 | （無し） | `X-MAIDENNAME` |
| `sort_name` | 並び替え用（よみ優先。保存時に組み立てる） | — | — |
| `birthday` | 誕生日（`YYYY-MM-DD` / 年なし `--MM-DD`） | `birthdays[0]` | `BDAY` |
| `note` | メモ | `biographies[0]` | `NOTE` |
| `avatar_path` | アバター（後続。§6） | `photos` | `PHOTO` |
| `show_as_company` | 会社として表示 | （無し） | `X-ABShowAs:COMPANY` |
| `is_favorite` | お気に入り | `memberships` の `starred`（§2） | （無し） |
| `is_business` / `allow_remote_images` | 取引先 / 外部画像許可（Rondine 固有） | 送らない | 送らない |
| `dirty` | 未送信のローカル変更（つながりの `dirty` のどれかが 1、または未連携の新規） | — | — |
| `created_at` / `updated_at` / `deleted_at` | 作成 / 更新 / 論理削除 | — | — |

**廃止する列:** `email` / `emails` / `phone` / `address` / `organization` / `org_id` / `org_title` /
`org_department`（子テーブルへ）、`name_kana`（`sort_name` へ）、`source` / `external_id` / `uid`
（つながり表へ。出どころは「どこにつながっているか」で表す）。

### 1-2. 子テーブル（すべて `contact_id` ＋ `position`。`position = 0` が主）

| 表 | 列 | Google | iCloud (vCard) |
|---|---|---|---|
| `contact_organizations` | `org_id`（→ 組織カード）/ `name` / `phonetic_name` / `title` / `department` | `organizations[]`（複数） | `ORG` / `TITLE` / `X-PHONETIC-ORG`（1 つ） |
| `contact_emails` | `label` / `value` / `is_shared` | `emailAddresses[]` | `EMAIL` |
| `contact_phones` | `label` / `value` / `is_shared` | `phoneNumbers[]` | `TEL` |
| `contact_addresses` | `label` / `po_box` / `postal` / `region` / `city` / `street` / `extended` / `country` / `country_code` | `addresses[]` | `ADR` ＋ `X-ABADR` |
| `contact_urls` | `label` / `value` | `urls[]` | `URL` |
| `contact_dates` | `label` / `date`（記念日など。誕生日は本体） | `events[]` | `X-ABDATE` / `ANNIVERSARY` |
| `contact_relations` | `label` / `name` | `relations[]` | `X-ABRELATEDNAMES` |
| `contact_handles` | `kind`（`im` / `social`）/ `service` / `value` / `label` | `imClients[]` | `IMPP` / `X-SOCIALPROFILE` |
| `contact_custom_fields` | `key` / `value` | `userDefined[]` | （無し） |
| `contact_tags` | `tag_id`（既存のまま） | ラベル（`memberships`） | グループ（後続） |

- **主値は `position = 0`**。`is_primary` 列は持たない（並び順と二重にしない）。
- **ラベルの語彙は今の vCard 取り込みに揃える**（自宅 / 職場 / 携帯 / FAX / 代表。Google の `other`
  は無ラベル、カスタム名はそのまま）。
- **メール・電話の `is_shared`**（会社の共有アドレス・代表番号の印）は Rondine 固有。送らない。

### 1-3. つながり表 `contact_identities`（既存を正式化）

| 列 | 意味 |
|---|---|
| `provider` | `google` / `icloud` |
| `account_id` | 連携アカウント（`google_accounts.id`。iCloud は後続で同等の表） |
| `external_id` | 向こうの ID（Google は `people/c…`、iCloud は vCard の URL/UID） |
| `contact_id` | つながる Rondine の連絡先（NULL＝未照合） |
| `etag` | 向こうの版（更新の衝突検出） |
| `snapshot` | 取り込んだ内容（照合と差分判定の材料。`ContactFields` の JSON） |
| `dirty` | このつながりへ未送信のローカル変更がある（つながりごとに送るため。§8） |
| `fetched_at` | 取り込んだ時刻 |

1 人の連絡先に、`provider` の違う行が並んでよい（Google と iCloud の両方につながる）。

### 1-4. 片付けるもの

- `contact_groups` / `contact_group_members` と `contact_group_list` コマンド（画面から使われて
  いない。タグに置き換わっている）。
- 同期への置き換え用に作った「写しの片付け」（`store/google_copies.rs` /
  `GoogleContactCopies.tsx`）。全件を入れ直すので不要。

## 2. 表示: どこと同期しているか

- **一覧と詳細に、つながっているサービスのアイコンを並べる**（Google / iCloud）。どこにも
  つながっていなければ Rondine のアイコン。
- 詳細ではアイコンにアカウント名を添える（同じサービスに複数アカウントがありうるため）。
- 住所録の一覧の上に「すべて / Google / iCloud / Rondine のみ」の絞り込みを置く。
- お気に入りは Google の「スター付き」（システムグループ `starred`）と同期する。

## 3. 同期の規則

| 場面 | 動き |
|---|---|
| Rondine で編集 | つながっている**すべての**サービスへ送る（各サービスが扱える項目だけ） |
| Rondine で削除 | つながっているすべてのサービスから削除する（ゴミ箱を経由） |
| 向こうで編集 | そのサービスから取り込む。送っていない変更（`dirty`）がある連絡先は上書きしない |
| 向こうで削除 | **そのつながりだけを外す。**Rondine の連絡先と、他のサービスの連絡先は残す（削除を連鎖させない） |
| 両方で同時に編集 | 後勝ち（今と同じ） |
| 同期で入ってきた人が既存の誰かと同じ | 高確信ならつながりを足すだけ。迷ったら新規で作り、「重複整理」に出す |
| 重複整理でまとめる | 残す側に、すべてのつながりを寄せる |
| 連携を一時的に解除 | つながりも未送信の変更も残す（印は「解除中」）。同じアカウントで再接続すると続きから同期する |
| 連携を完全に解除 | そのアカウントのつながりだけを外す。連絡先は Rondine のみとして残す（docs/CALENDAR_SYNC.md §2） |

### 送るときに向こうの項目を消さない

Google の更新は項目ごと（`names` / `organizations` …）の丸ごと置き換え。Rondine が持たない
部分（将来足される項目、Google の内部的な付帯情報）を消さないため、**送る直前にその連絡先を
Google から読み直し、それを土台に Rondine が扱う部分だけを上書きして送る。**送るのは編集した
連絡先だけなので、読み直しの負担は小さい。

## 4. 組織カードとのつながり

- `contact_organizations.org_id` で組織カードを指す。会社名は `name` に文字で持ち、組織カードに
  つながっていなければ `org_id = NULL`。
- **同期の取り込みでは、組織カードを自動では作らない。**既にある組織カードと名前が一致した
  ときだけつなぐ（表記ゆれは組織の重複整理と同じ正規化で吸収）。Google の 6,141 件の会社名を
  すべてカードにすると、取引の無い会社や表記ゆれまでカードになり、組織カードが何千件にも膨らむ。
  どれをカードにするかは、下の「整理」で人が選ぶ（利用者の提案 2026-10-08）。
- 連絡先の編集画面で会社名を入れたときは今の規則のまま（候補から選べばつなぐ、無い名前は
  保存時にカードを作る）。人が明示的に入れた会社だけがカードになる。
- 組織カードの代表電話・FAX・代表メール・URL・所在地は**どのサービスにも送らない**（向こうに
  「組織」という単位が無い）。個人の連絡先に同じ値がある場合の「統合しますか？」は今のまま。

### 4-1. 整理（組織タブ）

組織タブに「整理」を置き、今の「組織の重複整理」と並べる。どちらも**候補を出すだけで、
つなぐ・作るは人が選ぶ。**

1. **組織カードになっていない会社名**: `org_id` が無い `contact_organizations.name` を、正規化した
   名前ごとにまとめて人数の多い順に出す。選んで「組織カードにする」と、カードを作って同じ名前の
   人を全員つなぐ。
2. **組織カードがあるのにつながっていない人**: 組織カードごとに、つながっていない人の候補を
   理由つきで出す。
   - 会社名が一致（正規化後）
   - メールのドメインが一致（組織カードの代表メール、または既につながっているメンバーの
     メールと同じドメイン）。**フリーメール（gmail.com 等）は除く**（グリーンドメインと同じ一覧）
   選んで「つなぐ」。

## 5. マイグレーション

- **0059**: 個人の連絡先まわりを作り直す。
  - 消して作り直す: `contacts` と子テーブル、`contact_identities`、`contact_group_identities`
  - 消す: `contact_groups` / `contact_group_members`
  - 残す: `organizations` / `tags`（`contact_tags` は作り直すので連絡先との紐付けは消える）
  - Google の連絡先同期の印を戻す（`google_accounts.contacts_sync_token = NULL`、
    `last_contacts_sync_at = NULL`）。次の取り込みがフル同期になる
- 連絡先を参照している他の機能（差出人名の解決・知り合い/グリーン判定・迷惑判定・宛先候補・
  スレッド）は、本体の主値列ではなく子テーブルを読むよう直す。入れ直すまでは一時的に空になる。

## 6. 後続

- **アバター（写真）**: 表には `avatar_path` を持つが、同期は後続（容量と取得の手間がかかる）。
- **iCloud（CardDAV）**: つながり表に `provider = 'icloud'` を足す。vCard 取り込みも §1 の項目を
  読むよう広げる。

## 7. 進め方

1. この文書の承認
2. マイグレーション 0059 と store 層（連絡先の読み書き・照合・同期の変換）
3. 連絡先を参照している他の機能の追従
4. 編集画面（項目の追加・会社の複数化）と一覧（アイコン・絞り込み）、組織タブの「整理」（§4-1）
5. raytrek で試験用 DB に Google から入れ直して確認 → vaio

## 8. 実装メモ（第 1 段で設計から変えた点・決めた点）

- **つながり表の `remote_deleted` は持たず、代わりに `dirty` を持つ。**向こうで削除されたら
  つながりの行そのものを消す（§3「そのつながりだけを外す」）。Rondine で削除して向こうへ削除を
  送り終えたときも行を消す。未送信の印は**つながりごと**に持つ — 本体の `dirty` 1 つだけでは、
  2 つのアカウント（や Google と iCloud）につながった人を片方へ送った時点で印が落ち、もう片方へ
  送られない。本体の `dirty` は「つながりの `dirty` のどれかが 1」に保つ（取り込みの上書きを
  止める条件は今までどおり本体の `dirty`）。
- **`uid`（rondine-id）は廃止。**§1-1 の「つながり表へ」に従った。iCloud（CardDAV）で新規に
  作るときの UID は、その段で採番する。
- **共有の印（`is_shared`）の付いたメール・電話も、値そのものは送る（印は送らない）。**
  旧実装は値ごと送らなかったが、送らないと Google 側から消え、次の取り込みで Rondine からも
  消える（取り込みは Google が扱う項目を Google の正本で置き換えるため）。
- **取り込み（同期）は Google が扱う項目を Google の正本で置き換える**（旧実装の COALESCE を
  やめた。Google 側で消した項目は Rondine でも消える）。Google に無い項目（旧姓・会社として
  表示・SNS のハンドル・取引先・外部画像許可）と、共有の印・組織カードとのつながりは残す
  （`services::contact_fields::overlay_google`）。
- **照合で既存の連絡先へ寄せるとき（高確信）は、通常の取り込みと同じ規則で Google の内容を
  取り込むだけにし、送り直さない。**Google が扱う項目は Google の値、Rondine にしか無い項目は残す
  （`overlay_google`）。手元に未送信の変更がある人はつなぐだけで上書きしない。同期のたびに自動で
  照合するようになったため、確認なしに Google を書き換えない（2026-10-09。以前は和集合にまとめて
  送り直していた）。Rondine にしか無い項目は、その人を利用者が編集したときに初めて送られる。
- **ファイル取り込み（vCard / Google CSV）も組織カードを作らない**（既存のカードと正規化名が
  一致したときだけつなぐ）。§4 の理由（表記ゆれまでカードになる）がファイルでも同じなため。
- **編集画面でも、正規化名が一致するカードがあればそこへつなぐ**（「(株)テスト」と入れても
  「株式会社テスト」のカードがあれば作らない）。カードを作るのは、正規化名が一致するカードが
  無く、かつ**その連絡先に保存済みでない（今回入れた）会社名**のときだけ — 同期で入った会社名が
  編集画面で保存しただけでカードになるのを防ぐため。つながった会社の名前はカードの名前に
  そろえる（カードの名前を変えると、名前が変わった人だけ送り直しの印が立つ）。
- **`sort_name` はよみ（姓・ミドル・名を空白でつなぐ）、無ければ表示名。**
- **ラベルの語彙を広げた**（`services::contact_labels`）: 自宅 / 職場 / 携帯 / FAX / 代表 に加え、
  ホームページ / ブログ / プロフィール / 記念日 / 関係（配偶者・子・母・父・親・兄弟・姉妹・友人・
  親戚・同居人・パートナー・上司・アシスタント・紹介者）。iCloud の `_$!<…>!$_` も同じ表で読む。
- **送信の本文**（`services::google::contacts::outgoing`）: 送る直前に `people.get` で読み直した
  Person を土台に、同じ値（メールは小文字・電話は数字・会社は正規化名・住所と会社は位置）の要素を
  引き継いで Rondine が扱うキーだけを上書きする。`metadata` / `formattedType` /
  `formattedValue` / 名前の `displayName`・`unstructuredName` などの読み取り専用・派生のキーは
  落とす。ニックネーム・メモ・誕生日は先頭の 1 つだけを置き換え、2 つ目以降（Rondine は持たない）
  は残す。etag は読み直した版のものを使う（結果として後勝ち）。
- **スター（お気に入り）とラベルの所属は、読み直した Person の所属と比べて差分だけを
  `contactGroups/*/members:modify` で送る**（旧実装は前回取り込み時点の snapshot と比べていた）。
- **一覧の境界型は子テーブルを空で返し、主値の写し（`primary_email` / `primary_phone` /
  `primary_organization`）と `links`（アイコン用）を持つ。**詳細（`contact_get`）と重複整理の
  候補は子テーブルまで充填する。
- **組織の整理（§4-1）のコマンド**: `organization_unlinked_names`（1）、
  `organization_create_from_name`（1 の「組織カードにする」）、`organization_link_suggestions`（2）、
  `organization_link_contacts`（2 の「つなぐ」）。2 の「会社名が一致」は、カードにつながっていない
  会社名だけを見る。「つなぐ」は、その人の会社のうち正規化名が同じもの（無ければ名前の無いもの）を
  カードへつなぎ、どちらも無ければ会社を 1 つ足す。
