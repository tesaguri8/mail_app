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

1. **設定 →「Google カレンダー」**。
2. 「OAuth クライアント認証情報」に **クライアント ID** と **クライアント シークレット**を入力 → **保存**。
3. **「Google アカウントを連携」** を押す → 既定ブラウザで Google の同意画面が開く。
   - テストユーザーに追加したアカウントでログイン →「続行」（未審査アプリの警告が出ても、テスト
     ユーザーなら「続行」で進めます）→ カレンダーの権限を許可。
   - 「認証が完了しました」ページが出たらブラウザを閉じて Rondine に戻ります。
4. 連携中アカウントの **「今すぐ同期」** で双方向同期を実行。以後もこのボタンで同期します。
5. 解除は **「解除」**。取り込んだ Google カレンダー／予定は Rondine から削除されます
   （Google 側の予定は消えません）。ローカル専用のカレンダー・予定には影響しません。

### 開発時: `.env` で資格情報を渡す（任意）

毎回 UI に貼らずに済ませたい場合は、プロジェクト直下の `.env`（`.gitignore` 済み）に書けます。
起動時に `src-tauri` が読み込みます（`dotenvy`）。**UI 入力（keyring 保存）があればそちらが優先**、
無ければこの環境変数を使います。

```dotenv
GCAL_CLIENT_ID=xxxx-xxxx.apps.googleusercontent.com
GCAL_CLIENT_SECRET=GOCSPX-xxxxxxxx
```

雛形は `.env.example`（`cp .env.example .env` で複製して実値を記入）。実値は**絶対にコミットしない**でください。

---

## 3. 設計（実装メモ）

### 3-1. 認証（`services/gcal/oauth.rs`）

- ループバック待受（`127.0.0.1:0` の任意ポート）+ **PKCE(S256)** + `state` 検証。
- `access_type=offline` / `prompt=consent` で **refresh token** を取得。
- スコープ: `https://www.googleapis.com/auth/calendar openid email`
  （読み書き＋連携アカウントのメール特定）。
- keyring 保存キー: Client Secret = `gcal:client_secret` / refresh token = `gcal:refresh:<email>`。
  Client ID は `app_settings.gcal_client_id`（非機密）。
- 資格情報の解決順: **保存済み（app_settings/keyring）→ 環境変数（`GCAL_CLIENT_ID` /
  `GCAL_CLIENT_SECRET`。dev の `.env` 用）**。`commands.rs::gcal_resolve_credentials`。
- 同期のたびに refresh token → access token を取り直す（アクセストークンは保存しない）。

### 3-2. データモデル（`migrations/0041_calendar_sync.sql`）

- `calendar_accounts`: 連携した Google アカウント（複数対応。メタのみ、資格情報は keyring）。
- `calendars` に追加: `account_id` / `sync_token`（増分同期）/ `access_role` / `sync_enabled`。
  - `source='google'` / `external_id`（Google カレンダー ID）は既存（0039）。
- `events` に追加: `etag` / `dirty`（ローカル変更が未送信＝1）。
  - `source='google'` / `external_id`（Google 予定 ID）は既存（0038）。
- ユーザー操作（`event_upsert` / `event_delete`）は `dirty=1` を立て、Google カレンダー所属の
  予定だけが次回同期で送信される。同期エンジンの取り込み（`apply_remote_event`）は `dirty=0`。

### 3-3. 同期アルゴリズム（`services/gcal/sync.rs`）

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

競合解決は v1 では概ね **後勝ち**（push→pull の順なので、最後に同期した側の状態へ収束）。

### 3-4. 変換の要点（`services/gcal/convert.rs`）

- 日時: ローカルは端末ローカルの素の文字列（終日=`YYYY-MM-DD` / 時間指定=`YYYY-MM-DDTHH:MM`）。
  取り込み時は Google の RFC3339（オフセット付き）→ 端末ローカルへ、送信時は端末オフセットを付けて RFC3339 に。
- 終日の終了日: Google は**排他日（翌日）**。取り込みで −1 日、送信で +1 日。
- 繰り返し: Google `recurrence[]` の先頭 `RRULE:` を `events.recurrence` に保存（送信は `["RRULE:…"]`）。
- 予定あり/なし: `transparency`（opaque/transparent）⇄ `availability`（busy/free）。
- 公開設定: `visibility`（default/public/private）。

### 3-5. v1 の制限（既知）

- **繰り返しの個別インスタンス上書き**（1 回だけ時間変更/削除など）は取り込まない（マスターのみ扱う）。
- **参加者（ゲスト）の送信は未対応**（取り込み・ローカル編集は従来どおり）。
- **添付・会議リンク（Meet）・色 ID** はマッピング対象外。
- 競合は後勝ち（フィールド単位のマージや競合 UI はなし）。
- 読み取り専用カレンダーに Rondine 側で作った予定は送信されない（ローカルに残る）。

---

最終更新日: 2026年7月
