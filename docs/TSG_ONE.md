# TSG One のアカウントでのログイン

`[確定]` 2026-10-31（利用者の判断）: Rondine は TSG One の**年額の利用権**で売る。試用は 30 日（tsg-core が数える）
なので、**試用にもログインが要る**。試用が終わって契約が無ければ、読むだけになる（同期と送信が止まる）。

**この段で入れたのはログインだけ。**利用権の判定（有効か・手元の保存・オフラインの署名の確かめ）と匿名の利用状況は、
自前で作らず tsg-kit の共通部品（`tsg-auth` の `EntitlementCache` / `tsg-telemetry`）で入れる。読むだけにする制限も、その後。

**メールの Google の接続（設定の「アカウント」）とは別のもの。**混ぜない。

## 仕様の出どころ

- tsg-core `docs/CLIENT_DEVICE_INTEGRATION.md`（端末の ID・登録・台数の枠・重い操作は会員サイト）
- tsg-core `docs/APP_SUBSCRIPTIONS.md`（アプリの利用権）
- tsg-kit `crates/auth/README.md`（`tsg-auth`：認可コード ＋ PKCE・端末の鍵 DPoP・`X-Device-ID`）
- 組み込み方は Primadoc（`src-tauri/src/commands/supporter/session.rs`）と同じ形

## 名乗り（単一ソースは `config/app-identity.json`）

| | 値 | 出どころ |
|---|---|---|
| client_id | `rondine-desktop` | `tsgClientId` |
| 戻り先 | 本番 `rondine://oauth/callback` / 開発版 `rondine-dev://` / 配る版 `rondine-preview://` | `scheme` ＋ `channel`（`app` なら付けない） |
| 接続先 | 本番 `https://accounts.tesaguri.tech` / preview `https://staging-accounts.tesaguri.tech` / dev `http://localhost:8109` | `channel` |
| 端末の鍵の名前 | `rondine-desktop` / `.preview` / `.dev` | `channel`（Windows の TPM は名前だけで鍵が決まるので、段ごとに分ける） |

- 戻り先のスキームは `scripts/sync-app-identity.mjs` が `tauri.conf.json` の `plugins.deep-link.desktop.schemes` に書く。
  Rust（`services/tsg_one/identity.rs`）も同じ規則で求め、**試験で突き合わせる**
- 開発版（debug の版）だけ、`RONDINE_TSG_CORE_URL` で接続先を差し替えられる（作り物の tsg-core で GUI を試すため）。本番の版は見ない

## 流れ（`src-tauri/src/services/tsg_one.rs`）

1. 設定の「TSG One」で提供元（Google / GitHub / Apple）を選ぶ → 端末の鍵を開き、鍵の拇印を名乗った認可の URL を
   システムのブラウザで開く。`state` と `code_verifier` は**メモリにだけ**置く（10 分）
2. ブラウザから deep link で戻る。Windows / Linux は OS がアプリをもう 1 つ起こすので、**単一インスタンス**が受け止めて
   今の窓へ引数を渡す。macOS は deep link のプラグインから届く（`lib.rs`）
3. `state` が合わない戻りは断るが、進めているログインは捨てない。引換券を鍵の proof と `X-Device-ID` を添えてトークンに替える
4. **更新用のトークンとログインした人は OS 金庫**（サービス名は識別子、項目名は `tsg-one`）。アクセストークンはメモリだけ。平文のファイルには置かない
5. `POST /api/devices` で端末を登録する。**台数の枠を超えた 403 は握り潰さず**、画面に「会員サイトでほかの端末を解除して確かめ直す」を出す
6. 結果は `tsg-one:changed` イベントで画面に知らせる。失敗の理由は種類（`TsgOneProblem`）で渡し、文言は画面が ja / en で出す

- 端末の鍵と、機械 ID が読めないときの退避の ID は **`app_data_dir/tsg-one/`**（データの置き場は移動できるので使わない。鍵を別の機械へ写さない）
- 端末の名前は OS のコンピューター名（hostname）
- ログアウトは `POST /api/auth/logout`（更新用のトークン ＋ 鍵の proof）を試み、**失敗しても手元は必ず消す**

## 開発版の単一インスタンス

プラグインの既定の名前は識別子（`tesaguri.rondine.dev`）。raytrek ではメインと作業木の開発版が同じセッションのバスに乗るので、
**Linux の開発版は名前を実行ファイルごとに分ける**（`services/deep_link.rs`）。分けないと並べて動かせない。

## 確かめていないこと

- **Windows の deep link の登録（`register_all`）と単一インスタンス**は raytrek では確かめられない（zv で確かめる）
- 本物の tsg-core（dev）を通したログイン（試験は作り物の tsg-core で通した）
- raytrek には `update-desktop-database`（desktop-file-utils）が無く、Linux の `register_all` は
  `.desktop` を書いたあとで失敗する（実際のブラウザからの戻りは、この機械では OS が渡さない）
- 端末の一覧・解除の画面はアプリに持たせていない（枠を超えたら会員サイトへ案内する。開く先は `<会員サイト>/account`。端末の画面の場所は会員サイト側で要確認）
