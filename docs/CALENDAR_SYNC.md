# Google カレンダー同期（双方向）

Rondine のローカルカレンダーと Google カレンダーを**双方向**で同期する機能の設計・使い方。

- Rondine で作成・編集・削除した予定 → Google へ送信（push）
- Google 側の作成・編集・削除 → Rondine へ取り込み（pull）

認証はデスクトップ向けの **OAuth 2.0（ループバック + PKCE）**。資格情報は OS 金庫（keyring）に保存し、
本文などのコンテンツは TSG One を含む外部へは一切送りません（Google API とだけ直接通信）。

---

## 1. 事前準備: Google Cloud Console で OAuth クライアントを作る

「テストユーザー運用」で構いません（Google の審査（verification）は不要）。所要 5〜10 分。

### 1-1. プロジェクトを用意

1. <https://console.cloud.google.com/> にログイン。
2. 画面上部のプロジェクト選択 →「新しいプロジェクト」→ 任意の名前（例: `rondine-dev`）で作成。

### 1-2. Google Calendar API を有効化

1. 左メニュー **「API とサービス」→「ライブラリ」**。
2. `Google Calendar API` を検索 → **有効にする**。

### 1-3. OAuth 同意画面（テスト公開）

1. **「API とサービス」→「OAuth 同意画面」**。
2. User Type = **外部（External）** を選択 →「作成」。
3. アプリ名（例: `Rondine`）、ユーザーサポートメール、デベロッパー連絡先を入力して保存。
4. **スコープ**: ここでは追加不要（アプリ側からリクエストします）。そのまま次へ。
5. **テストユーザー**: 「+ ADD USERS」で**自分の Google アカウント（連携したいアカウント）**を追加。
   - ここに入れたアカウントだけが連携できます（審査なしで使えるのはこのため）。
6. 公開ステータスは **「テスト中（Testing）」のまま**にします。

> ⚠️ テスト中の OAuth アプリは、発行される **refresh token が約 7 日で失効**します。
> 失効すると「今すぐ同期」がエラーになるので、その時は Rondine で**もう一度「連携」**してください。
> 恒久運用したくなったら、同意画面を「本番（In production）」に切り替え、Google の審査を通します。

### 1-4. OAuth クライアント ID（デスクトップアプリ）を作成

1. **「API とサービス」→「認証情報」→「+ 認証情報を作成」→「OAuth クライアント ID」**。
2. アプリケーションの種類 = **デスクトップアプリ** を選択（重要）。
   - デスクトップ種別は、Rondine が使う **ループバック（`http://127.0.0.1:<ポート>`）リダイレクト**を
     追加設定なしで許可します。リダイレクト URI を手動登録する必要はありません。
3. 名前を付けて「作成」。
4. 表示された **クライアント ID**（`…apps.googleusercontent.com`）と
   **クライアント シークレット**（`GOCSPX-…`）を控えます。

> デスクトップアプリのクライアントシークレットは秘匿性が高くありません（配布物に含まれる前提の種別）。
> それでも Rondine は Client Secret を**平文で持たず OS 金庫（keyring）に保存**します。

---

## 2. 使い方（Rondine 側）

1. **設定 →「同期」の「Google」欄**（2026-10-09 に「Google カレンダー」から改名。同期の画面は
   サービスごとの欄を並べ、iCloud の欄は CardDAV / CalDAV の実装まで「近日対応」の案内だけ）。
2. 「OAuth クライアント認証情報」に **クライアント ID** と **クライアント シークレット**を入力 → **保存**。
3. **「Google アカウントを連携」** を押す → 既定ブラウザで Google の同意画面が開く。
   - テストユーザーに追加したアカウントでログイン →「続行」（未審査アプリの警告が出ても、テスト
     ユーザーなら「続行」で進めます）→ カレンダーの権限を許可。
   - 「認証が完了しました」ページが出たらブラウザを閉じて Rondine に戻ります。
4. 連携中アカウントの **「今すぐ同期」** で双方向同期を実行。以後もこのボタンで同期します。
   - **1 回でカレンダーと連絡先の両方**を同期し、取り込んだ連絡先は住所録まで反映する
     （docs/CONTACTS_SYNC.md §2。`services::google::account::sync_account`、コマンド `google_sync`）。
     結果は 1 つの文にまとめて出す（0 の項目は省く）。片方が失敗しても、もう片方は進める
   - 自動同期（メールと同じ間隔）でもカレンダーは毎回同期する。連絡先は起動直後と、前回の
     連絡先の同期から 10 分以上たったときだけ（重いため。`utils/googleSyncSchedule.ts`）
5. 解除は **「解除」**。画面内の確認で 2 つから選ぶ（どちらも Google 側の予定・連絡先は消えない。
   ローカル専用のカレンダー・予定には影響しない）。
   - **一時的に解除（記録を残す・既定）**: refresh token を消し、アカウントを「解除中」にする
     （`google_accounts.disconnected_at`。マイグレーション 0061）。取り込んだカレンダー・予定、
     連絡先のつながり、未送信の変更（`dirty`）は残し、自動・手動・保存時の送信のどれも行わない。
     カレンダー一覧と設定のアカウント行に「解除中」と出る。**同じアカウントで「再接続」**すると
     同じ行を使い直して印が消え（メールアドレスで同一判定）、同期トークンもそのまま続きから
     同期し、未送信の変更はそのときに送る（トークンが失効していれば既存の 410 → フル同期）。
   - **完全に解除**: Google 側の許可を取り消し（`https://oauth2.googleapis.com/revoke`。失敗しても
     解除は続け、理由を表示）、連絡先のつながりを外し（連絡先は Rondine のみとして残す）、
     取り込んだカレンダーと予定の写し・アカウントの行を消す。**未送信の予定の変更は失われる。**
     許可を取り消すと、同じアカウント・同じ OAuth クライアントのほかの端末の連携も切れうる。
   - 以前は解除で行ごと消していたため、つながりが消えたアカウント ID を指したまま残り、再接続で
     id が変わると孤立していた（id は振り直されうる）。0061 でこの 2 段に分けた。

### 開発時: `.env` で資格情報を渡す（任意）

毎回 UI に貼らずに済ませたい場合は、プロジェクト直下の `.env`（`.gitignore` 済み）に書けます。
起動時に `src-tauri` が読み込みます（`dotenvy`）。**UI 入力（keyring 保存）があればそちらが優先**、
無ければこの環境変数を使います。

```dotenv
GOOGLE_CLIENT_ID=xxxx-xxxx.apps.googleusercontent.com
GOOGLE_CLIENT_SECRET=GOCSPX-xxxxxxxx
```

> 旧名 `GCAL_CLIENT_ID` / `GCAL_CLIENT_SECRET` も引き続き読み込みます（既存の `.env` を
> 壊さないため）。新規は `GOOGLE_*` を使ってください。

雛形は `.env.example`（`cp .env.example .env` で複製して実値を記入）。実値は**絶対にコミットしない**でください。

---

## 3. 設計（実装メモ）

### 3-0. モジュール構成（Google 連携全体）

Google 連携は **カレンダーと連絡先で共通の土台**を持つ。連絡先同期（People API）を足すときに
同じアカウントを二重連携させないため、認証・アカウント管理はサービス非依存の層に置く。

```
services/google.rs              共通: エンドポイント・スコープ組み立て・HTTP クライアント
services/google/oauth.rs        共通: OAuth（ループバック + PKCE）
services/google/calendar.rs     カレンダー同期のまとめ（API ベース URL）
services/google/calendar/{api,convert,sync}.rs
```

- **アカウント 1 件 = refresh_token 1 本**。カレンダーと連絡先はこれを共有する。
- 要求スコープは固定文字列ではなく `google::scopes(&[...])` で組み立てる。
- Tauri コマンドは、共通のものが `google_*`（`google_connect` / `google_disconnect` /
  `google_accounts` / `google_set_credentials` / `google_credentials_status`）、
  カレンダー固有の同期が `gcal_sync`。

### 3-1. 認証（`services/google/oauth.rs`）

- ループバック待受（`127.0.0.1:0` の任意ポート）+ **PKCE(S256)** + `state` 検証。
- `access_type=offline` / `prompt=consent` で **refresh token** を取得。
- `include_granted_scopes=true` を付けるので、**後から別サービスのスコープを追加同意しても
  既存の許可は失われない**（カレンダー連携済みに連絡先を足す場合など）。
- スコープ: `openid email` ＋ サービスごと（カレンダー =
  `https://www.googleapis.com/auth/calendar`、連絡先 = `.../auth/contacts`）。
- 実際に許可されたスコープはトークン応答から `google_accounts.granted_scopes` に記録する
  （ユーザーが一部だけ許可することがあるため、要求ではなく実測値を持つ）。
- keyring 保存キー: Client Secret = `google:client_secret` /
  refresh token = `google:refresh:<email>`。Client ID は `app_settings.google_client_id`（非機密）。
  旧キー（`gcal:` 接頭辞）は読み出し時に新キーへ自動で移す（再連携は不要）。
- 資格情報の解決順: **保存済み（app_settings/keyring）→ 環境変数（`GOOGLE_CLIENT_ID` /
  `GOOGLE_CLIENT_SECRET`。dev の `.env` 用）→ アプリ同梱の既定クライアント**。
  `commands.rs::google_resolve_credentials`。
- **同梱の既定クライアント**（`services/google.rs` の `BUILTIN_CLIENT_ID` /
  `BUILTIN_CLIENT_SECRET`）は製品化用の枠で、現在は空。実値を入れると、ユーザーが自分で
  Google Cloud Console にクライアントを作る手順（§1）が不要になる。デスクトップ種別の
  client_secret は秘匿性を前提としない種別なので同梱してよい。
- 同期のたびに refresh token → access token を取り直す（アクセストークンは保存しない）。

### 3-2. データモデル（`migrations/0041_calendar_sync.sql` / `0055_google_accounts.sql`）

- `google_accounts`: 連携した Google アカウント（複数対応。メタのみ、資格情報は keyring）。
  0041 では `calendar_accounts` だったものを 0055 でサービス共通へ改称し、
  `granted_scopes` / `sync_calendar` / `sync_contacts` /
  `last_calendar_sync_at` / `last_contacts_sync_at` を追加した。
- `calendars` に追加: `account_id` / `sync_token`（増分同期）/ `access_role` / `sync_enabled`。
  - `source='google'` / `external_id`（Google カレンダー ID）は既存（0039）。
- `events` に追加: `etag` / `dirty`（ローカル変更が未送信＝1）。
  - `source='google'` / `external_id`（Google 予定 ID）は既存（0038）。
- ユーザー操作（`event_upsert` / `event_delete`）は `dirty=1` を立て、Google カレンダー所属の
  予定だけが次回同期で送信される。同期エンジンの取り込み（`apply_remote_event`）は `dirty=0`。

### 3-3. 同期アルゴリズム（`services/google/calendar/sync.rs`）

カレンダーごとに **push → pull** の順で実行:

1. **カレンダー一覧**を取り込み `calendars` に upsert（`primary` は「マイ」、他は「他」）。
2. **push**（書き込み可能＝`owner`/`writer` のみ）: `dirty=1` の予定を
   - 未連携（`external_id` なし）→ 作成（insert）
   - 連携済み → 更新（patch）
   - 論理削除（`deleted_at`）→ Google 側も削除（delete）
   成功で `dirty=0`。
3. **pull**: `syncToken` があれば増分、なければ過去 1 年からのフル。
   - `status=cancelled` → ローカルを論理削除。
   - それ以外 → `external_id` で突き合わせて upsert。
   - 最終ページの `nextSyncToken` を保存。`410 Gone`（トークン失効）は sync_token を捨てて
     フル同期にフォールバック。
   - **手元と同じ版（etag が同じ・同じカレンダー・削除されていない）は何もしない**（取り込みに
     数えず、書き直さず、手元の未送信の変更も上書きしない）。`[実測]` 2026-10-09: Google の
     祝日カレンダー（`ja.japanese#holiday@group.v.calendar.google.com`）は自分で発行した
     `nextSyncToken` を次の呼び出しで毎回 410 にするので、同期のたびにフル取得になる。以前は
     その 169 件が毎回「取り込み」に数えられ、書き直されていた。410 の記録は debug ログに出す

競合解決は v1 では概ね **後勝ち**（push→pull の順なので、最後に同期した側の状態へ収束）。

### 3-4. 変換の要点（`services/google/calendar/convert.rs`）

- 日時: ローカルは端末ローカルの素の文字列（終日=`YYYY-MM-DD` / 時間指定=`YYYY-MM-DDTHH:MM`）。
  取り込み時は Google の RFC3339（オフセット付き）→ 端末ローカルへ、送信時は端末オフセットを付けて RFC3339 に。
- 終日の終了日: Google は**排他日（翌日）**。取り込みで −1 日、送信で +1 日。
- 繰り返し: Google `recurrence[]` の先頭 `RRULE:` を `events.recurrence` に保存（送信は `["RRULE:…"]`）。
- 繰り返しの例外インスタンス（`recurringEventId` ＋ `originalStartTime` を持つ予定）→ §3-6。
- 予定あり/なし: `transparency`（opaque/transparent）⇄ `availability`（busy/free）。
- 公開設定: `visibility`（default/public/private）。

### 3-5. v1 の制限（既知）

- **繰り返しの「この回だけ」を Rondine で新しく作ることはできない**（本体の編集はシリーズ全体に
  適用）。Google 側で作られた例外の取り込み・表示は §3-6。
- **`recurrence[]` の `EXDATE` 行は取り込まない**（Google 上の 1 回だけの削除は、通常 EXDATE ではなく
  cancelled の例外インスタンスとして来るので §3-6 で扱える。iCal 由来の予定で EXDATE が残っている
  ものだけが、削除した回も表示される）。
- **参加者（ゲスト）の送信は未対応**（取り込み・ローカル編集は従来どおり）。
- **添付・会議リンク（Meet）・色 ID** はマッピング対象外。
- 競合は後勝ち（フィールド単位のマージや競合 UI はなし）。
- 読み取り専用カレンダーに Rondine 側で作った予定は送信されない（ローカルに残る）。

### 3-6. 繰り返しの例外インスタンス（1 回だけの変更・削除）

`events.list` は `singleEvents=false` で取るので、繰り返しは**本体（RRULE 付き）1 件＋例外**で届く。
展開はフロント（`src/renderer/utils/recurrence.ts`）が本体の RRULE から行う。

| Google から来るもの | ローカルでの持ち方（`migrations/0060`） |
|---|---|
| **1 回だけ変更された回**（`recurringEventId`＋`originalStartTime`、status≠cancelled） | 通常の予定行（`recurrence` なし）として保存し、`events.recurring_external_id`（本体の Google ID）と `events.original_start_at`（元の回の開始・ローカル表現）を持たせる |
| **1 回だけ削除された回**（同上、status=cancelled） | 予定行は作らず `event_cancelled_instances` に（カレンダー, 例外 ID, 本体 ID, 元の開始）を記録 |

- **表示**: `list_events` は本体の行に `exdates`（変更・削除された回の元の開始）を付けて返す。
  展開はその**日付**の回を出さない（対応する FREQ は 1 日に高々 1 回なので日付で突き合わせる）。
  変更された回は通常の予定として範囲抽出で出る。変更された回を消した（ゴミ箱）場合も、本体の分は出さない。
- **本体の削除**（Google 側・Rondine 側とも）: その本体の変更された回も一緒に論理削除する（孤立させない）。
  Rondine 側の削除で送るのは本体だけ（Google は本体の削除で例外も消す）。本体を戻すと、一緒に
  ゴミ箱に入った回も戻る。
- **変更された回の編集**: その回の Google ID へ PATCH される（その回だけに効く）。編集画面では繰り返しの
  設定を出さない（`original_start_at` が非 null の予定）。
- **エクスポート（.ics）**: 本体に `EXDATE` を付け、変更された回は別の VEVENT として出す。
- **既存 DB**: 0060 で Google カレンダーの `sync_token` を捨て、次回の同期をフル同期にして過去の例外を取り直す。

---

最終更新日: 2026年10月
