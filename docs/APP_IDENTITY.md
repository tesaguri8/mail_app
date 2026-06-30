# アプリ識別情報の単一ソース化（ハードコード排除）

**ステータス:** 計画（実装未着手）
**目的:** 製品名・identifier などの**アプリ識別情報を 1 箇所に集約**し、いつでも差し替え可能にする。ハードコードを排除する（[CODING_GUIDELINES 準拠](DEVELOPMENT_PLAN.md)）。

---

## 1. 課題

製品名・identifier は複数の場所で必要になる:

- `src-tauri/tauri.conf.json`（`productName` / `identifier`）
- Rust（データパス・ログ・ストア）
- フロント（React/TS の表示名・スキーム）
- モバイル（Expo `app.json` の `name` / `ios.bundleIdentifier` / `android.package` / `scheme`）

これらに**直接値を書く（ハードコード）と、名称変更時に修正漏れ**が起きる。`tauri.conf.json` / `Cargo.toml` / Expo 設定は静的ファイルのため「JS のグローバル変数を参照」も直接はできない。

---

## 2. 方針：1つの JSON を真実の源にする

リポジトリ直下に **単一ソース** を置き、各ターゲットへ**生成・実行時参照**で配る。

```jsonc
// config/app-identity.json  ← ここだけ編集すれば全体に反映
{
  "productName": "Comfort Mail",          // 表示名（仮称）
  "identifier":  "tesaguri.comfortmail.dev", // 規則: tesaguri.<slug>.<channel|app>
  "vendor":      "tesaguri",
  "slug":        "comfortmail",            // 両OS安全な連結（_・- を使わない）
  "scheme":      "comfortmail",            // ディープリンク用 URL スキーム
  "channel":     "dev"                     // dev/staging/stable（正式時は app）
}
```

> slug は **連結（`comfortmail`）**。Apple bundle id は下線不可、Android package はハイフン不可のため、区切りなしが安全。

---

## 3. 各ターゲットへの反映

| ターゲット | 反映方法 |
|---|---|
| `tauri.conf.json` | 生成スクリプトが `productName` / `identifier` を書き込む（静的ファイルのため生成） |
| **Rust** | **実行時に Tauri から取得**（`app.config().identifier` / `app.package_info()`）。データパスは `app_data_dir()`（identifier ベース）。**定数を持たない** |
| フロント（React/TS） | 生成した `src/config/appIdentity.ts` を参照、または `@tauri-apps/api/app` の `getName()` / `getIdentifier()` / `getVersion()` を実行時に使用 |
| モバイル（Expo） | **`app.config.ts` が `config/app-identity.json` を import** し `name` / `ios.bundleIdentifier` / `android.package` / `scheme` を導出（Expo 動的コンフィグ＝真の単一ソース） |
| 共有 | `packages/app-identity`（JSON を re-export）で web/mobile の TS から共通利用 |

### 生成スクリプト（例）

```
scripts/sync-app-identity.mjs
  1. config/app-identity.json を読む
  2. src-tauri/tauri.conf.json（および *.dev/*.staging）へ productName / identifier を書き込む
  3. src/config/appIdentity.ts を生成（"// AUTO-GENERATED — do not edit" バナー付き）
  4. （任意）Cargo メタデータ／Expo は app.config.ts が直接 import するため不要
```

`package.json` の `predev` / `prebuild` / `prepare` で自動実行し、常に同期させる。

---

## 4. フロントでの使い方（イメージ）

```ts
// src/config/appIdentity.ts は生成物。利用側はここを import する（ハードコードしない）
import { APP } from '@/config/appIdentity';
// APP.productName / APP.identifier / APP.scheme ...

// あるいは Tauri から実行時取得
import { getName, getVersion } from '@tauri-apps/api/app';
```

---

## 5. 改名フロー（差し替え手順）

1. `config/app-identity.json` の `productName` / `identifier` / `slug` / `scheme` を編集（**ここだけ**）。
2. `npm run sync:identity`（生成スクリプト）で各設定へ反映。
3. **identifier を変えた場合**はデータ保存フォルダ名が変わるため、**旧→新フォルダの移行**（起動時に旧 identifier ディレクトリがあればリネーム/コピー）を 1 度行う。詳細: [DATA_STORAGE.md](DATA_STORAGE.md)。
4. ストア登録済みの場合、`identifier`（bundle id / package）変更は別アプリ扱いになるため、**公開後は identifier を固定**する（正式リリース前に確定させる）。

---

## 6. 原則（ハードコード排除の徹底）

- **アプリ識別情報** → `config/app-identity.json`（本書）
- **その他の定数** → `config/`（マジックナンバー禁止）
- **ユーザー向け文字列** → i18n リソース（[I18N.md](I18N.md)）

> 「表示名・identifier を直書きしない」をレビュー観点に含める。生成物（`appIdentity.ts` 等）は編集禁止バナーを付ける。
