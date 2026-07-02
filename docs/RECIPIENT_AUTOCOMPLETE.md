# 宛先オートコンプリート（Compose × 住所録）設計

メール作成画面（Compose）の宛先入力（To / Cc / Bcc）で、入力に応じて候補を表示し、
**名前・メールアドレスのどちらからでも**相手を選べるようにする。候補ソースは
**住所録（連絡先）＋ 過去のやり取り相手（送受信履歴）**の2つ。

> ステータス: **設計確定・実装待ち**。実装は下記「前提（マージ依存）」の解消後に着手する。

## 背景・前提（マージ依存）

宛先オートコンプリートは 2 つの機能に同時に依存するが、現時点でこの両方を含む
ブランチが存在しない。

| ブランチ | Compose（送信UI） | 住所録（contacts） |
|---|---|---|
| `dev` | ✅ あり | ❌ なし |
| `feature/contacts` | ❌ なし | ✅ あり |
| `feature/mail-search` | ✅ あり | ❌ なし |

→ **実装には Compose と contacts の両方が同一ワークツリーに揃っている必要がある。**
ユーザーが両者をマージ（例: `feature/contacts` を `dev` に統合、または統合ブランチを用意）
した後に、そのブランチ上で本設計に沿って実装する。本ドキュメントはマージ完了までの
「実装の設計図」。

## データソースと既存インターフェイス

### ソース1: 住所録（contacts）
- テーブル: `contacts`（`display_name`, `name_kana`, `email`(代表), `is_favorite` ほか）、
  ラベル付き複数アドレスは `contact_emails(contact_id, label, value, is_primary)`。
- 既存コマンド `contact_list(query, group)` は
  `display_name / name_kana / email / organization` を部分一致検索し、
  `is_favorite DESC, name_kana, display_name` 順で `ContactSummary[]` を返す（`email` は代表1件）。
  - 参考: `src/renderer/services/contacts.ts`、`src-tauri/src/services/store/contacts.rs`

> 本機能では**代表アドレス1件**を採用（ユーザー選択）。会社用/個人用など複数アドレスの
> 出し分けは将来拡張（`contact_emails` を JOIN、または選択時 `contact_get` で展開）。

### ソース2: 過去のやり取り相手（送受信履歴）
- テーブル: `emails.from_address`（受信の差出人）、`emails.to_addresses`（送信の宛先。
  複数はカンマ区切りのヘッダ文字列 `"Name <a@b>, c@d"`）。索引 `idx_emails_from`。
- 住所録に無い相手でも、過去に一度でもやり取りしていれば候補に出す。

## バックエンド設計（Rust）

### 新モデル `RecipientSuggestion`（`src-tauri/src/models.rs`）
```rust
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RecipientSuggestion {
    pub email: String,            // 正規化前の表示用アドレス（小文字化はキー用に内部で）
    pub name: Option<String>,     // 表示名（連絡先名 or ヘッダから抽出）
    pub source: String,           // "contact" | "history"
    pub is_favorite: bool,        // 住所録のお気に入り（並び優先）
    pub contact_id: Option<i32>,  // 住所録由来なら連絡先ID（詳細展開用）
}
```

### 新ストアメソッド（新ファイル `src-tauri/src/services/store/recipients.rs`）
```rust
pub fn suggest_recipients(&self, query: &str, limit: i64)
    -> rusqlite::Result<Vec<RecipientSuggestion>>
```
ロジック:
1. `query` を空白トリム。空なら空配列（住所録全件を出すかは要検討＝初期は空でよい）。
2. **住所録**: `contacts` を `display_name / name_kana / email / organization LIKE %q%`
   （`COLLATE NOCASE`, `ESCAPE '\'` で `% _ \` をエスケープ）で検索し、`email IS NOT NULL`
   の行から `RecipientSuggestion{ source:"contact", is_favorite, contact_id }` を生成。
3. **履歴**: `emails` から `from_address LIKE %q% OR to_addresses LIKE %q%` の候補行を取得し、
   Rust 側で各ヘッダ文字列をパース（`Name <email>` / 素の `email` / カンマ区切りを分解）。
   マッチしたアドレスのみ採用。`source:"history"`。
4. **重複排除**: `email` の小文字化をキーに **住所録を優先**（同じアドレスが両方に出たら
   contact 側を残す）。履歴側は登場頻度（COUNT）で降順に。
5. **並び**: `is_favorite DESC` → source(contact>history) → 頻度 → 名前。`LIMIT limit`。

パース補助（同ファイル内 private fn）:
```rust
// "Some Name <a@b.com>" -> (Some("Some Name"), "a@b.com"); "a@b.com" -> (None, "a@b.com")
fn parse_addr(raw: &str) -> Option<(Option<String>, String)>
// "A <a@b>, c@d" -> Vec<(Option<String>, String)>
fn split_header_addrs(raw: &str) -> Vec<(Option<String>, String)>
```

> パフォーマンス: 履歴の LIKE は前方一致索引が効かない（`%q%`）ため、`limit` を小さめ
> （例 8〜10）＋候補行取得にも上限を設ける。将来的に「連絡先へ昇格」or 専用の
> recipients 集計テーブルを検討（過剰最適化はしない）。

### 新コマンド（`src-tauri/src/commands.rs` ＋ `lib.rs` 登録）
```rust
#[tauri::command]
pub fn recipient_suggest(store: State<Store>, query: String, limit: i64)
    -> Result<Vec<RecipientSuggestion>, String>
```
`lib.rs` の `generate_handler![...]` に `commands::recipient_suggest` を追加。

### テスト（`recipients.rs` 内 `#[cfg(test)]`）
- `parse_addr` / `split_header_addrs` の単体（名前付き・素アドレス・カンマ区切り・空）。
- `suggest_recipients`: 住所録一致・履歴一致・**重複時に住所録優先**・お気に入り上位・
  空クエリで空。`storage.rs` テストと同じ `test_store()`（in-memory + migrations）流儀。

## フロントエンド設計（React / TS）

### サービス `src/renderer/services/recipients.ts`
```ts
import { invoke } from '@tauri-apps/api/core';
import type { RecipientSuggestion } from '@bindings/RecipientSuggestion';

export const recipientSuggest = (query: string, limit = 8) =>
  invoke<RecipientSuggestion[]>('recipient_suggest', { query, limit });
```

### コンポーネント `src/renderer/components/RecipientInput.tsx`
To / Cc / Bcc の 3 か所で共有する。既存の Compose は各フィールドを
**カンマ区切り文字列 + `splitAddresses()`** で扱うので、**同じ `value: string` / `onChange`
契約を維持**して差し替え負荷を最小化する（チップ化は将来拡張）。

Props:
```ts
{ value: string; onChange: (v: string) => void; placeholder?: string; autoFocus?: boolean }
```
挙動:
- 入力の**最後のトークン**（最後のカンマ以降）を `query` として 200〜250ms デバウンスで
  `recipientSuggest` を呼ぶ（メール検索と同じデバウンス方式）。
- 候補ドロップダウンを表示。1 行 =「表示名 <メール>」＋お気に入り★／source バッジ。
- 選択で、最後のトークンを `name <email>`（名前があれば）または `email` に確定し、
  末尾に `, ` を付けて次の入力に備える。`onChange` で親へ反映。
- キーボード操作: `↑/↓` で候補移動、`Enter`/`Tab` で確定、`Esc` で閉じる。
  候補が閉じている時の `Enter` は通常送信フローを妨げない。
- クリック外し（blur）でドロップダウンを閉じる（`onMouseDown` で選択を先取り）。
- アクセシビリティ: `role="combobox"` + `aria-expanded` + `aria-activedescendant`。

### Compose への差し込み（`src/renderer/components/Compose.tsx`）
現状の 3 つの `<input value={to|cc|bcc} onChange=...>`（`docs` 参照の宛先ブロック）を
`<RecipientInput value=... onChange=... placeholder=... />` に置換。
`splitAddresses` / 送信時の `to/cc/bcc` 組み立ては**変更不要**（文字列契約を維持するため）。

## i18n（`src/renderer/locales/{ja,en}/common.json`）
`compose` 名前空間に追記（既存に `toPlaceholder` 等あり）:
```jsonc
"compose": {
  "suggestFavorite": "お気に入り",   // ★バッジ / aria
  "suggestFromContacts": "住所録",   // source=contact バッジ
  "suggestFromHistory": "履歴",      // source=history バッジ
  "suggestEmpty": "候補がありません"  // 任意（空表示を出すなら）
}
```
（英: "Favorite" / "Contacts" / "History" / "No matches"）

## 実装順（マージ後）
1. Backend: `RecipientSuggestion` モデル → `recipients.rs`（`suggest_recipients` + パーサ + テスト）。
2. Backend: `recipient_suggest` コマンド追加 → `lib.rs` 登録。
3. `npm run gen:bindings` で `RecipientSuggestion.ts` 生成。
4. Front: `recipients.ts` → `RecipientInput.tsx`。
5. Compose の To/Cc/Bcc を差し替え。
6. i18n 追記。
7. 検証: `npm run typecheck && npm run lint`、`cd src-tauri && cargo test --lib && cargo clippy`。

## 将来拡張（今回のスコープ外）
- 連絡先の**複数アドレス**を候補に展開（`contact_emails` JOIN もしくは選択時 `contact_get`）。
- 宛先の**チップ（トークン）UI**（削除・重複検知・不正アドレス警告）。
- 履歴候補の**頻度/最近性スコア**の高度化・専用集計テーブル。
- 候補からの**その場で住所録へ追加**（`contact_upsert`）。
