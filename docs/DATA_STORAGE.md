# データ保存場所設計

**ステータス:** 計画（実装未着手）
**出典:** 旧 `README_plan.md` §13。パス取得の実装例は Electron から **Tauri（Rust）** に置き換え。

---

## 0. データの置き場所：アプリ専用 vs TSG One 共有

| データ | 置き場所 | 理由 |
|---|---|---|
| メールDB(SQLCipher)・添付・索引・`media`（背景）・アプリ設定 | **アプリ専用** `…\tesaguri.comfortmail.dev\` | 大容量・機密・アプリ固有。隔離＆暗号化、移行/アンインストール容易 |
| AI 注釈（要約・分類など、メール内容由来） | **アプリ専用**（メールDB側 `ai_annotations`） | メールに紐づく機密。共有領域に出さない |
| TSG One アカウント／サインイン・**AIトークン残量・使用量**・共有AI設定 | **TSG One 共有** `…\tesaguri.tsg-one\` | 全TSGアプリで同一アカウント・同一トークン残量を共有 |
| 機密（TSG One トークン・資格情報） | **OS keyring**（サービス名 `tesaguri.tsg-one`） | keyring は OS 全体共有 → 全TSGアプリが同じ資格情報を参照。平文フォルダ不要 |

> 方針: **メール本体はアプリ専用**（他アプリと共有しない）／**AI のアカウント・トークンは TSG One 共有**。秘密は共有フォルダでなく **keyring の共通サービス名**で集約する。
> ※ 共有ストアの正確な名称は、既存 TSG One デスクトップのアカウント保管と揃える（単一ソース）。

---

## 1. 基本方針
- **アプリ識別子（identifier）規則: `tesaguri.<app_name>.app`**（Tesaguri アプリ共通。Primadoc = `tesaguri.primadoc.app`）。
  - **暫定値: `tesaguri.comfortmail.dev`**（製品名は仮称 **Comfort Mail**、`.dev` は開発/暫定の意）。正式確定時に `tesaguri.<確定名>.app` へ変更する。
  - slug は両プラットフォーム対応のため**連結 `comfortmail`**（Apple bundle id は下線不可、Android package はハイフン不可のため、区切りなしが安全）。
- データディレクトリは **この identifier をそのままフォルダ名**として、各 OS の標準場所に配置（Tauri / Primadoc と同方式）。
- プラットフォーム固有の標準的な場所を使用。
- セキュリティとプライバシーを考慮した保存戦略。

---

## 1.5 容量と分離（再配置できるデータルート）

メールは添付・大量メールで**容量が大きくなる**ため、保存を二層に分け、**大容量側をユーザーが別の場所（別ドライブ等）へ再配置できる**ようにする。

| 層 | 内容 | 場所 |
|---|---|---|
| **設定層（小・固定）** | `settings.json` / `ui-state.json` / **データルートの場所ポインタ** | 標準: `…\tesaguri.comfortmail.dev\config\` |
| **データルート（大・再配置可）** | `mail.db`・`emails`・`attachments`・`search`・`media`・`cache` | 既定はアプリ配下。**設定で任意のフォルダ/ドライブへ変更可**（例 `D:\ComfortMailData\`） |

- **移動操作**: 設定で新しいデータルートを選択 → 既存データを移行（コピー → 検証 → 旧削除）→ ポインタ更新。
- **保持ポリシーと併用**: 物理的な再配置（どこに置くか）と、[SYNC.md](SYNC.md) の保持ポリシー（どれだけ残すか）は別軸。両方で容量を管理。
- keyring（秘密）は場所に依存しない（OS 共有）。
- （任意・将来）アカウント別に異なるデータルート（大きいアカウントを別ドライブ）も拡張可能。

---

## 2. プラットフォーム別データ保存場所

> 以下のパス中 `tesaguri.comfortmail.dev` は暫定 identifier。名称確定時に置換する。

### Windows
```
C:\Users\{username}\AppData\Roaming\tesaguri.comfortmail.dev\
├── data\
│   ├── mail.db                # SQLite（SQLCipher 暗号化）
│   ├── emails\                # メール本文ファイル（年月別: 2024\01\ ...）
│   ├── attachments\           # 添付（{email_id}\ 別）
│   └── search\                # 検索インデックス
├── media\                     # ユーザー資産
│   └── backgrounds\           # 取り込んだ背景画像（コピー保存）
├── config\                    # settings.json / ui-state.json（アカウント機密は keyring）
├── cache\                     # thumbnails（背景サムネ含む）/ temp
└── logs\                      # app.log / sync.log / error.log
```

> **背景画像の取り込み方針**: ユーザー選択画像は元ファイルを参照せず `media\backgrounds\` へ**コピー**して保持（移動/削除に強く、バックアップ・移行も完結）。サムネは `cache\thumbnails\`。アプリ同梱画像は `static/`（バンドル resources）。大容量メディアで Roaming プロファイルの肥大を避けたい場合は `media\` のみ `%LOCALAPPDATA%`（Local）に置く選択肢もある。

### macOS
```
~/Library/Application Support/tesaguri.comfortmail.dev/
├── data/ (mail.db, emails/, attachments/, search/)
├── media/backgrounds/
├── config/
├── cache/
└── logs/

機密情報: macOS Keychain（keyring クレート経由）
```

### Linux
```
~/.local/share/tesaguri.comfortmail.dev/     # data/, media/, cache/, logs/
~/.config/tesaguri.comfortmail.dev/          # settings.json 等
機密情報: Secret Service（keyring クレート経由）
```

> モバイル（Android/iOS）は将来のモバイル版で別途設計。基本はアプリサンドボックス内に `data/` を置き、エクスポート/バックアップのみ共有領域を使う。

---

## 3. 実装例（Tauri / Rust）

識別子は**ハードコードしない**。`tauri.conf.json` の `identifier` を単一ソースとし、`app_data_dir()`（identifier ベース）でパスを解決する。identifier 自体は `config/app-identity.json` から生成する（[APP_IDENTITY.md](APP_IDENTITY.md)）。
資格情報は平文ファイルに置かず、必ず `keyring` を使う。

```rust
// src-tauri/src/services/storage_paths.rs
use std::path::PathBuf;

pub struct StoragePaths {
    pub app: PathBuf,
    pub data: PathBuf,
    pub database: PathBuf,
    pub emails: PathBuf,
    pub attachments: PathBuf,
    pub search_index: PathBuf,
    pub backgrounds: PathBuf,   // media/backgrounds（取り込み背景画像）
    pub config: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
}

impl StoragePaths {
    /// 二層構成: 設定層は identifier 配下に固定、データルート(大容量)は再配置可。
    /// identifier はハードコードせず Tauri（tauri.conf.json）を単一ソースに解決（app_data_dir）。
    /// data_root_override = ユーザーが設定した別ドライブ等のパス（無ければ既定＝アプリ配下）。
    pub fn resolve(app: &tauri::AppHandle, data_root_override: Option<PathBuf>) -> Self {
        use tauri::Manager;
        let config_base = app.path().app_data_dir().expect("app_data_dir"); // 設定層(固定) = …/<identifier>
        let data_root = data_root_override.unwrap_or_else(|| config_base.clone()); // 大容量(再配置可)
        let data = data_root.join("data");
        Self {
            // 大容量 → データルート配下（別ドライブに移動可能）
            database: data.join("mail.db"),
            emails: data.join("emails"),
            attachments: data.join("attachments"),
            search_index: data.join("search"),
            backgrounds: data_root.join("media").join("backgrounds"),
            cache: data_root.join("cache"),
            // 小・固定 → 設定層
            config: config_base.join("config"),
            logs: config_base.join("logs"),
            data,
            app: config_base,
        }
    }

    /// 必要なディレクトリを作成
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [&self.data, &self.emails, &self.attachments,
                    &self.search_index, &self.backgrounds,
                    &self.config, &self.cache, &self.logs] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}
```

> `app_data_dir()` は `tauri.conf.json` の `identifier` をフォルダ名に用いるため、**Rust 側に識別子のハードコードが不要**になる（identifier は `config/app-identity.json` → 生成 → tauri.conf という単一ソース）。現状の暫定 identifier は `tesaguri.comfortmail.dev`。
> カスタム保存先（ユーザー指定ドライブ等）は設定で受け取り、バリデーション後に上書きする。

---

## 4. セキュリティ考慮事項

- **暗号化**: アカウント情報 = `keyring`（OS 金庫）/ DB = SQLCipher / 添付 = AES-256（`aes-gcm`）
- **アクセス制御**: ファイルパーミッションは所有者のみ（600/700 相当）。データアクセスは Rust バックエンドに限定。
- **一時ファイル**: 自動削除とセキュアクリア。

---

## 5. バックアップ・同期戦略

```typescript
// 設定の型イメージ（フロント側）
interface BackupConfig {
  enabled: boolean;
  interval: 'daily' | 'weekly' | 'monthly';
  location: 'local' | 'cloud' | 'external';
  retention: number; // 保持世代数
}

interface StorageConfig {
  primary: string;          // メイン保存先
  backup?: string;          // バックアップ先
  sync?: {
    enabled: boolean;
    service: 'onedrive' | 'googledrive' | 'dropbox';
    path: string;
  };
}
```

---

## 6. メリット
1. **一貫性**: 全プラットフォームで統一されたディレクトリ構造
2. **拡張性**: identifier 規則（`tesaguri.<app>.app`）統一で、他 Tesaguri アプリ（Primadoc 等）と一貫
3. **管理性**: 複数アプリ利用時の一元管理
4. **標準準拠**: 各プラットフォームの標準的な場所を使用
5. **セキュリティ**: 適切なアクセス制御と暗号化
6. **バックアップ**: 容易なバックアップと復元
