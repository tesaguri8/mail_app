# データ保存場所設計

**ステータス:** 計画（実装未着手）
**出典:** 旧 `README_plan.md` §13。パス取得の実装例は Electron から **Tauri（Rust）** に置き換え。

---

## 1. 基本方針
- ベンダー専用ディレクトリ（SNGDesign）配下にアプリ別ディレクトリ（MailApp）を配置
- プラットフォーム固有の標準的な場所を使用
- セキュリティとプライバシーを考慮した保存戦略
- 将来的な他の SNGDesign アプリとの一貫性を保持

---

## 2. プラットフォーム別データ保存場所

### Windows
```
C:\Users\{username}\AppData\Roaming\SNGDesign\MailApp\
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
~/Library/Application Support/SNGDesign/MailApp/
├── data/ (mail.db, emails/, attachments/, search/)
├── config/
├── cache/
└── logs/

機密情報: macOS Keychain（keyring クレート経由）
```

### Linux
```
~/.local/share/sngdesign/mailapp/     # data/, cache/, logs/
~/.config/sngdesign/mailapp/          # settings.json 等
機密情報: Secret Service（keyring クレート経由）
```

> モバイル（Android/iOS）は将来のモバイル版で別途設計。基本はアプリサンドボックス内に `data/` を置き、エクスポート/バックアップのみ共有領域を使う。

---

## 3. 実装例（Tauri / Rust）

パスは Tauri の `path` API または `dirs` クレートで解決し、`SNGDesign/MailApp` を付与する。
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
    /// OS ごとの標準ベースディレクトリ配下に SNGDesign/MailApp を構築
    pub fn resolve() -> Self {
        // Win:  %APPDATA%\SNGDesign\MailApp
        // mac:  ~/Library/Application Support/SNGDesign/MailApp
        // Linux: ~/.local/share/sngdesign/mailapp
        let base = dirs::data_dir().expect("data_dir");
        let app = if cfg!(target_os = "linux") {
            base.join("sngdesign").join("mailapp")
        } else {
            base.join("SNGDesign").join("MailApp")
        };
        let data = app.join("data");
        Self {
            database: data.join("mail.db"),
            emails: data.join("emails"),
            attachments: data.join("attachments"),
            search_index: data.join("search"),
            backgrounds: app.join("media").join("backgrounds"),
            config: app.join("config"),
            cache: app.join("cache"),
            logs: app.join("logs"),
            data,
            app,
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

> Tauri 側で `app_handle.path().app_data_dir()` を使う方法もある。ベンダー名（SNGDesign）を確実に付与するため、上記のように明示的に組み立てる方針とする。
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
2. **拡張性**: 将来の他 SNGDesign アプリとの統合が容易
3. **管理性**: 複数アプリ利用時の一元管理
4. **標準準拠**: 各プラットフォームの標準的な場所を使用
5. **セキュリティ**: 適切なアクセス制御と暗号化
6. **バックアップ**: 容易なバックアップと復元
