# メールデスクトップアプリ開発計画（詳細版）

## 1. プロジェクト概要

### 1.1 ビジョン
- **目的**: 既存のメールクライアントにない、モダンで直感的なユーザー体験を提供
- **差別化要素**: チャット形式の会話ビュー、高速検索、スマートな振り分け
- **ターゲットユーザー**: 効率的なメール管理を求める個人ユーザー

### 1.2 主要機能
1. チャット形式のメール表示
2. 高速メタデータ検索
3. スマートタグシステム
4. マルチアカウント対応
5. アドレス帳連携

## 2. 技術アーキテクチャ

### 2.1 フロントエンド（GUI）
```
技術スタック:
- Electron 28.x
- React 18.x
- TypeScript 5.x
- Vite（ビルドツール）
- TailwindCSS（スタイリング）
- Zustand（状態管理）
- React Query（データフェッチング）
```

### 2.2 バックエンド（API）
```
技術スタック:
- Python 3.11+
- FastAPI 0.104+
- SQLAlchemy 2.0（ORM）
- Alembic（マイグレーション）
- IMAPlib/SMTP（メールプロトコル）
- Pydantic 2.x（データバリデーション）
```

### 2.3 データストレージ
```
- SQLite: メタデータ、インデックス
- ファイルシステム: メール本文、添付ファイル
- 暗号化: SQLCipher（SQLite暗号化）
```

## 3. 詳細機能仕様

### 3.1 メール表示機能

#### 3.1.1 チャット形式ビュー
```
要件:
- 同一スレッドのメールをチャット風に表示
- 送信者ごとにメッセージをグループ化
- タイムスタンプの自動グループ化（5分以内）
- インライン返信機能
- 既読/未読の視覚的表現

実装詳細:
- Virtual Scrolling（大量メール対応）
- メッセージのリアルタイム更新
- アニメーション付きメッセージ追加
```

#### 3.1.2 従来形式ビュー
```
要件:
- 標準的なメールスレッド表示
- 折りたたみ可能な返信履歴
- クイックプレビュー
- 一括操作対応
```

### 3.2 検索システム

#### 3.2.1 検索機能詳細
```
検索対象:
- 件名、本文、送信者、受信者
- 添付ファイル名
- タグ、フラグ
- 日付範囲
- サイズ

検索方式:
- 全文検索（FTS5）
- ファセット検索
- 自然言語クエリ（将来実装）
- 検索履歴とサジェスト
```

#### 3.2.2 インデックス戦略
```
- 非同期インデックス作成
- 差分更新
- 定期的な最適化
- メモリ効率的な実装
```

### 3.3 タグ・振り分けシステム

#### 3.3.1 タグ機能
```
機能:
- 手動タグ付け
- 自動タグ付けルール
- タグの階層構造
- カラーコーディング
- タグベースのフィルタリング

ルールエンジン:
- 送信者ベース
- 件名パターンマッチ
- 本文キーワード
- 添付ファイルの有無
- 時間帯
```

#### 3.3.2 スマートフォルダ
```
- 動的フォルダ（検索条件保存）
- プリセットフォルダ（未読、重要、など）
- カスタムフォルダ
```

### 3.4 アドレス帳機能

#### 3.4.1 連携対象
```
- Google Contacts（OAuth2）
- iCloud Contacts（macOS）
- ローカルアドレス帳
- LDAP（企業向け、将来実装）
```

#### 3.4.2 機能詳細
```
- 自動補完
- 連絡先のマージ
- グループ管理
- 最近の連絡先
- お気に入り
```

## 4. データベース設計（詳細）

### 4.1 主要テーブル

```sql
-- アカウント
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    email TEXT NOT NULL,
    display_name TEXT,
    imap_host TEXT NOT NULL,
    imap_port INTEGER DEFAULT 993,
    smtp_host TEXT NOT NULL,
    smtp_port INTEGER DEFAULT 587,
    auth_type TEXT DEFAULT 'password',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- メール
CREATE TABLE emails (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    message_id TEXT UNIQUE NOT NULL,
    thread_id TEXT,
    subject TEXT,
    from_address TEXT,
    to_addresses TEXT,
    cc_addresses TEXT,
    bcc_addresses TEXT,
    date TIMESTAMP,
    received_date TIMESTAMP,
    size INTEGER,
    has_attachments BOOLEAN DEFAULT FALSE,
    is_read BOOLEAN DEFAULT FALSE,
    is_flagged BOOLEAN DEFAULT FALSE,
    folder_id INTEGER,
    raw_headers TEXT,
    body_plain TEXT,
    body_html TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id)
);

-- スレッド
CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    subject TEXT,
    participants TEXT,
    last_activity TIMESTAMP,
    message_count INTEGER DEFAULT 0,
    unread_count INTEGER DEFAULT 0,
    has_attachments BOOLEAN DEFAULT FALSE
);

-- タグ
CREATE TABLE tags (
    id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL,
    color TEXT,
    parent_id INTEGER,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (parent_id) REFERENCES tags(id)
);

-- メール-タグ関連
CREATE TABLE email_tags (
    email_id INTEGER,
    tag_id INTEGER,
    assigned_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (email_id, tag_id),
    FOREIGN KEY (email_id) REFERENCES emails(id),
    FOREIGN KEY (tag_id) REFERENCES tags(id)
);

-- 添付ファイル
CREATE TABLE attachments (
    id INTEGER PRIMARY KEY,
    email_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT,
    size INTEGER,
    file_path TEXT,
    checksum TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (email_id) REFERENCES emails(id)
);

-- 検索インデックス（FTS5）
CREATE VIRTUAL TABLE email_fts USING fts5(
    subject,
    from_address,
    to_addresses,
    body_plain,
    content=emails,
    content_rowid=id
);
```

### 4.2 インデックス戦略
```sql
CREATE INDEX idx_emails_thread_id ON emails(thread_id);
CREATE INDEX idx_emails_date ON emails(date DESC);
CREATE INDEX idx_emails_from ON emails(from_address);
CREATE INDEX idx_emails_account_folder ON emails(account_id, folder_id);
CREATE INDEX idx_email_tags_tag_id ON email_tags(tag_id);
```

## 5. API設計

### 5.1 エンドポイント一覧

```yaml
# アカウント管理
POST   /api/accounts                 # アカウント追加
GET    /api/accounts                 # アカウント一覧
PUT    /api/accounts/{id}            # アカウント更新
DELETE /api/accounts/{id}            # アカウント削除

# メール操作
GET    /api/emails                   # メール一覧（ページネーション）
GET    /api/emails/{id}              # メール詳細
POST   /api/emails                   # メール送信
PUT    /api/emails/{id}              # メール更新（既読、フラグ等）
DELETE /api/emails/{id}              # メール削除

# スレッド
GET    /api/threads                  # スレッド一覧
GET    /api/threads/{id}/messages    # スレッド内メール

# 検索
POST   /api/search                   # 検索実行
GET    /api/search/suggestions       # 検索サジェスト

# タグ
GET    /api/tags                     # タグ一覧
POST   /api/tags                     # タグ作成
PUT    /api/tags/{id}                # タグ更新
DELETE /api/tags/{id}                # タグ削除

# 同期
POST   /api/sync/start               # 同期開始
GET    /api/sync/status              # 同期状態
POST   /api/sync/stop                # 同期停止

# WebSocket
WS     /ws/notifications             # リアルタイム通知
```

### 5.2 データモデル例

```python
# メールモデル
class EmailResponse(BaseModel):
    id: int
    message_id: str
    thread_id: str
    subject: str
    from_address: str
    to_addresses: List[str]
    date: datetime
    preview: str
    is_read: bool
    is_flagged: bool
    has_attachments: bool
    tags: List[TagResponse]
```

## 6. セキュリティ設計

### 6.1 認証情報の保護
```
- Electron safeStorage API使用
- メインプロセスでのみ復号化
- メモリ上での最小限保持
- 定期的なトークンローテーション
```

### 6.2 通信セキュリティ
```
- IPC通信の検証
- CSP（Content Security Policy）設定
- XSS対策
- SQLインジェクション対策
```

### 6.3 データ保護
```
- SQLite暗号化（SQLCipher）
- 添付ファイルの暗号化保存
- 安全な一時ファイル処理
- 定期的なデータクリーンアップ
```

## 7. パフォーマンス最適化

### 7.1 フロントエンド
```
- React.memo、useMemoの適切な使用
- 仮想スクロール実装
- 画像の遅延読み込み
- Web Worker活用（検索、暗号化）
```

### 7.2 バックエンド
```
- 非同期処理の徹底
- データベースクエリ最適化
- キャッシュ戦略
- バッチ処理
```

## 8. テスト戦略

### 8.1 テストの種類
```
- 単体テスト（Jest、pytest）
- 統合テスト
- E2Eテスト（Playwright）
- パフォーマンステスト
```

### 8.2 カバレッジ目標
```
- コアロジック: 90%以上
- API: 80%以上
- UI: 70%以上
```

## 9. 開発フェーズ詳細

### フェーズ1: 基盤構築（2週間）
```
Week 1:
- プロジェクト構造の作成
- 開発環境のセットアップ
- 基本的なElectronアプリ起動
- FastAPIサーバー起動

Week 2:
- データベーススキーマ実装
- 基本的なAPI実装
- IPC通信の確立
- 認証システムの基礎
```

### フェーズ2: コア機能（3週間）
```
Week 3-4:
- IMAP/SMTP接続実装
- メール同期機能
- 基本的なメール表示UI
- メール送信機能

Week 5:
- チャット形式ビュー実装
- スレッド管理
- リアルタイム更新
```

### フェーズ3: 高度な機能（3週間）
```
Week 6-7:
- 全文検索実装
- タグシステム
- 振り分けルール

Week 8:
- アドレス帳基本機能
- Google Contacts連携
```

### フェーズ4: 品質向上（2週間）
```
Week 9:
- パフォーマンス最適化
- UIポリッシュ
- エラーハンドリング

Week 10:
- テスト作成
- ドキュメント整備
- リリース準備
```

## 10. 将来の拡張計画

### 短期（3-6ヶ月）
- モバイルアプリ開発
- 追加のメールプロバイダ対応
- プラグインシステム

### 中期（6-12ヶ月）
- AI機能（スマート返信、分類）
- チーム共有機能
- 高度な自動化

### 長期（1年以上）
- 多言語対応
- エンタープライズ機能
- SaaS版の提供

## 11. 多言語対応（国際化/i18n）設計

### 11.1 基本方針
- 最初から多言語対応を組み込むことで、後々の実装を容易にする
- 日本語と英語の2言語から開始（将来的に2〜4言語程度を想定）
- 各JSONファイル内で言語をキーとして管理する方式を採用

### 11.2 翻訳データ構造

#### ファイル構成
```
data/
└── translations/
    ├── common.json      # 共通翻訳（ボタン、ラベル等）
    ├── mail.json        # メール関連
    ├── settings.json    # 設定関連
    ├── search.json      # 検索関連
    └── tags.json        # タグ関連
```

#### JSONファイル構造例
```json
// data/translations/mail.json
{
  "mailList": {
    "title": {
      "ja": "受信トレイ",
      "en": "Inbox"
    },
    "unread": {
      "ja": "未読",
      "en": "Unread"
    },
    "markAsRead": {
      "ja": "既読にする",
      "en": "Mark as read"
    },
    "noMails": {
      "ja": "メールがありません",
      "en": "No emails"
    },
    "mailCount": {
      "ja": "{{count}}件のメール",
      "en": "{{count}} emails"
    }
  },
  "compose": {
    "to": {
      "ja": "宛先",
      "en": "To"
    },
    "subject": {
      "ja": "件名",
      "en": "Subject"
    },
    "send": {
      "ja": "送信",
      "en": "Send"
    },
    "sendingError": {
      "ja": "送信に失敗しました",
      "en": "Failed to send"
    }
  }
}
```

### 11.3 実装方法

#### TypeScript型定義
```typescript
// shared/types/i18n.ts
export type SupportedLanguage = 'ja' | 'en';

export type TranslationValue = {
  [key in SupportedLanguage]: string;
};

export interface TranslationStructure {
  [key: string]: TranslationValue | TranslationStructure;
}
```

#### 翻訳フック実装
```typescript
// gui/src/hooks/useTranslation.ts
import { useState, useCallback } from 'react';
import type { SupportedLanguage } from '@shared/types/i18n';

export const useTranslation = (namespace: string) => {
  const [locale, setLocale] = useState<SupportedLanguage>('ja');
  
  const t = useCallback((key: string, params?: Record<string, any>): string => {
    // ネストしたキーへのアクセス (例: "mailList.title")
    const keys = key.split('.');
    let value: any = translations[namespace];
    
    for (const k of keys) {
      value = value?.[k];
    }
    
    if (!value || !value[locale]) {
      console.warn(`Translation missing: ${namespace}.${key}[${locale}]`);
      return value?.en || key; // フォールバックとして英語またはキー自体を返す
    }
    
    let result = value[locale];
    
    // パラメータ置換 {{param}} 形式
    if (params) {
      Object.entries(params).forEach(([k, v]) => {
        result = result.replace(`{{${k}}}`, String(v));
      });
    }
    
    return result;
  }, [locale, namespace]);
  
  return { t, locale, setLocale };
};
```

#### コンポーネントでの使用例
```tsx
// gui/src/components/MailList.tsx
import { useTranslation } from '@/hooks/useTranslation';

export const MailList: React.FC = () => {
  const { t } = useTranslation('mail');
  
  return (
    <div>
      <h1>{t('mailList.title')}</h1>
      <span>{t('mailList.unread')}: 5</span>
      <p>{t('mailList.mailCount', { count: 10 })}</p>
      {emails.length === 0 && (
        <p>{t('mailList.noMails')}</p>
      )}
    </div>
  );
};
```

### 11.4 翻訳管理ツール

#### バリデーションスクリプト
```javascript
// scripts/validate-translations.js
const fs = require('fs');
const path = require('path');

function validateTranslations() {
  const translationsDir = path.join(__dirname, '../data/translations');
  const files = fs.readdirSync(translationsDir).filter(f => f.endsWith('.json'));
  const supportedLanguages = ['ja', 'en'];
  
  const errors = [];
  
  files.forEach(file => {
    const content = JSON.parse(fs.readFileSync(path.join(translationsDir, file), 'utf8'));
    checkMissingLanguages(content, file, [], errors, supportedLanguages);
  });
  
  if (errors.length > 0) {
    console.error('Translation validation errors:');
    errors.forEach(e => console.error(e));
    process.exit(1);
  } else {
    console.log('✓ All translations are valid!');
  }
}

function checkMissingLanguages(obj, file, path, errors, languages) {
  Object.entries(obj).forEach(([key, value]) => {
    const currentPath = [...path, key];
    
    if (typeof value === 'object' && languages.some(lang => lang in value)) {
      // 翻訳値の場合
      languages.forEach(lang => {
        if (!value[lang]) {
          errors.push(`Missing ${lang} translation: ${file} -> ${currentPath.join('.')}`);
        }
      });
    } else if (typeof value === 'object') {
      // ネストしたオブジェクトの場合
      checkMissingLanguages(value, file, currentPath, errors, languages);
    }
  });
}
```

### 11.5 メリット

1. **翻訳漏れの即座の発見**: 一つのファイルで全言語を確認できる
2. **開発効率**: 新機能追加時に一つのファイルを編集するだけ
3. **型安全性**: TypeScriptによる補完とエラー検出
4. **保守性**: 言語数が少ない場合に最適な構造
5. **拡張性**: 新言語の追加が容易

## 12. ディレクトリ構成（詳細）

```
mail_app/
├── gui/                                # Electronアプリケーション
│   ├── src/
│   │   ├── main/                      # Electronメインプロセス
│   │   │   ├── main.ts                # エントリーポイント
│   │   │   ├── window-manager.ts      # ウィンドウ管理
│   │   │   ├── security.ts            # セキュリティ設定
│   │   │   ├── menu.ts                # アプリケーションメニュー
│   │   │   └── ipc-handlers.ts        # IPC通信ハンドラー
│   │   │
│   │   ├── renderer/                  # React アプリケーション
│   │   │   ├── App.tsx                # メインアプリコンポーネント
│   │   │   ├── main.tsx               # React エントリーポイント
│   │   │   ├── index.html
│   │   │   │
│   │   │   ├── components/            # 再利用可能コンポーネント
│   │   │   │   ├── ui/                # 基本UIコンポーネント
│   │   │   │   │   ├── Button.tsx
│   │   │   │   │   ├── Input.tsx
│   │   │   │   │   ├── Modal.tsx
│   │   │   │   │   ├── Loading.tsx
│   │   │   │   │   └── index.ts
│   │   │   │   │
│   │   │   │   ├── layout/            # レイアウトコンポーネント
│   │   │   │   │   ├── Header.tsx
│   │   │   │   │   ├── Sidebar.tsx
│   │   │   │   │   ├── Layout.tsx
│   │   │   │   │   └── StatusBar.tsx
│   │   │   │   │
│   │   │   │   ├── mail/              # メール関連コンポーネント
│   │   │   │   │   ├── MailList.tsx
│   │   │   │   │   ├── MailItem.tsx
│   │   │   │   │   ├── MailViewer.tsx
│   │   │   │   │   ├── ComposeModal.tsx
│   │   │   │   │   ├── ThreadView.tsx
│   │   │   │   │   ├── ChatView.tsx
│   │   │   │   │   └── AttachmentList.tsx
│   │   │   │   │
│   │   │   │   ├── search/            # 検索関連コンポーネント
│   │   │   │   │   ├── SearchBar.tsx
│   │   │   │   │   ├── SearchFilters.tsx
│   │   │   │   │   ├── SearchResults.tsx
│   │   │   │   │   └── SearchHistory.tsx
│   │   │   │   │
│   │   │   │   ├── tags/              # タグ関連コンポーネント
│   │   │   │   │   ├── TagList.tsx
│   │   │   │   │   ├── TagEditor.tsx
│   │   │   │   │   ├── TagSelector.tsx
│   │   │   │   │   └── RuleEditor.tsx
│   │   │   │   │
│   │   │   │   └── settings/          # 設定関連コンポーネント
│   │   │   │       ├── AccountSettings.tsx
│   │   │   │       ├── GeneralSettings.tsx
│   │   │   │       ├── AppearanceSettings.tsx
│   │   │   │       └── SecuritySettings.tsx
│   │   │   │
│   │   │   ├── pages/                 # ページコンポーネント
│   │   │   │   ├── Dashboard.tsx      # メインダッシュボード
│   │   │   │   ├── MailPage.tsx       # メール表示ページ
│   │   │   │   ├── SearchPage.tsx     # 検索ページ
│   │   │   │   ├── SettingsPage.tsx   # 設定ページ
│   │   │   │   └── OnboardingPage.tsx # 初回セットアップ
│   │   │   │
│   │   │   ├── hooks/                 # カスタムフック
│   │   │   │   ├── useTranslation.ts  # 多言語対応
│   │   │   │   ├── useMails.ts        # メール管理
│   │   │   │   ├── useSearch.ts       # 検索機能
│   │   │   │   ├── useTags.ts         # タグ管理
│   │   │   │   ├── useSettings.ts     # 設定管理
│   │   │   │   ├── useTheme.ts        # テーマ管理
│   │   │   │   └── useKeyboardShortcuts.ts
│   │   │   │
│   │   │   ├── services/              # API通信・ビジネスロジック
│   │   │   │   ├── api.ts             # API クライアント
│   │   │   │   ├── mail-service.ts    # メール操作
│   │   │   │   ├── search-service.ts  # 検索処理
│   │   │   │   ├── tag-service.ts     # タグ操作
│   │   │   │   ├── sync-service.ts    # 同期処理
│   │   │   │   └── storage-service.ts # ローカルストレージ
│   │   │   │
│   │   │   ├── contexts/              # React Context
│   │   │   │   ├── I18nContext.tsx    # 多言語対応
│   │   │   │   ├── ThemeContext.tsx   # テーマ管理
│   │   │   │   ├── AuthContext.tsx    # 認証状態
│   │   │   │   └── SettingsContext.tsx
│   │   │   │
│   │   │   ├── stores/                # 状態管理（Zustand）
│   │   │   │   ├── mail-store.ts      # メール状態
│   │   │   │   ├── ui-store.ts        # UI状態
│   │   │   │   ├── search-store.ts    # 検索状態
│   │   │   │   └── settings-store.ts  # 設定状態
│   │   │   │
│   │   │   ├── utils/                 # ユーティリティ関数
│   │   │   │   ├── date.ts            # 日付処理
│   │   │   │   ├── format.ts          # フォーマット処理
│   │   │   │   ├── validation.ts      # バリデーション
│   │   │   │   └── keyboard.ts        # キーボードショートカット
│   │   │   │
│   │   │   └── styles/                # スタイル関連
│   │   │       ├── globals.css
│   │   │       ├── components.css
│   │   │       ├── themes.css
│   │   │       └── tailwind.config.js
│   │   │
│   │   ├── shared/                    # 共有型定義など
│   │   │   ├── types/
│   │   │   │   ├── mail.ts            # メール関連型
│   │   │   │   ├── api.ts             # API型定義
│   │   │   │   ├── ui.ts              # UI型定義
│   │   │   │   └── settings.ts        # 設定型定義
│   │   │   └── constants.ts           # 定数定義
│   │   │
│   │   └── tests/                     # テストファイル
│   │       ├── components/
│   │       ├── hooks/
│   │       └── utils/
│   │
│   ├── public/                        # 静的ファイル
│   │   ├── icons/                     # アプリケーションアイコン
│   │   │   ├── icon.png
│   │   │   ├── icon.icns
│   │   │   └── icon.ico
│   │   └── locales/                   # 翻訳ファイル（build時にコピー）
│   │
│   ├── electron-builder.json          # パッケージング設定
│   ├── vite.config.ts                 # Vite設定
│   ├── tsconfig.json                  # TypeScript設定
│   ├── tailwind.config.js             # TailwindCSS設定
│   ├── eslintrc.json                  # ESLint設定
│   ├── prettier.config.js             # Prettier設定
│   └── package.json
│
├── api/                               # FastAPI バックエンド
│   ├── app/
│   │   ├── main.py                    # FastAPIエントリーポイント
│   │   ├── __init__.py
│   │   │
│   │   ├── api/                       # APIエンドポイント
│   │   │   ├── __init__.py
│   │   │   ├── v1/                    # APIバージョン管理
│   │   │   │   ├── __init__.py
│   │   │   │   ├── api.py             # APIルータ統合
│   │   │   │   ├── accounts.py        # アカウント管理API
│   │   │   │   ├── emails.py          # メール操作API
│   │   │   │   ├── threads.py         # スレッド管理API
│   │   │   │   ├── search.py          # 検索API
│   │   │   │   ├── tags.py            # タグ管理API
│   │   │   │   ├── sync.py            # 同期API
│   │   │   │   └── websocket.py       # WebSocket通信
│   │   │   └── deps.py                # 共通の依存関係
│   │   │
│   │   ├── core/                      # 設定、セキュリティ
│   │   │   ├── __init__.py
│   │   │   ├── config.py              # アプリケーション設定
│   │   │   ├── security.py            # セキュリティ関連
│   │   │   ├── logging.py             # ロギング設定
│   │   │   └── events.py              # イベントハンドラー
│   │   │
│   │   ├── models/                    # Pydanticモデル
│   │   │   ├── __init__.py
│   │   │   ├── account.py             # アカウントモデル
│   │   │   ├── email.py               # メールモデル
│   │   │   ├── thread.py              # スレッドモデル
│   │   │   ├── tag.py                 # タグモデル
│   │   │   ├── search.py              # 検索モデル
│   │   │   └── common.py              # 共通モデル
│   │   │
│   │   ├── schemas/                   # データベーススキーマ（SQLAlchemy）
│   │   │   ├── __init__.py
│   │   │   ├── account.py
│   │   │   ├── email.py
│   │   │   ├── thread.py
│   │   │   ├── tag.py
│   │   │   └── attachment.py
│   │   │
│   │   ├── services/                  # ビジネスロジック
│   │   │   ├── __init__.py
│   │   │   ├── mail/                  # メール処理
│   │   │   │   ├── __init__.py
│   │   │   │   ├── imap_client.py     # IMAP接続
│   │   │   │   ├── smtp_client.py     # SMTP送信
│   │   │   │   ├── parser.py          # メール解析
│   │   │   │   └── processor.py       # メール処理
│   │   │   ├── search/                # 検索エンジン
│   │   │   │   ├── __init__.py
│   │   │   │   ├── indexer.py         # インデックス作成
│   │   │   │   ├── searcher.py        # 検索実行
│   │   │   │   └── fts_engine.py      # 全文検索エンジン
│   │   │   ├── sync/                  # 同期処理
│   │   │   │   ├── __init__.py
│   │   │   │   ├── scheduler.py       # 同期スケジューラ
│   │   │   │   ├── syncer.py          # 同期処理
│   │   │   │   └── conflict_resolver.py
│   │   │   ├── auth.py                # 認証サービス
│   │   │   ├── encryption.py          # 暗号化サービス
│   │   │   └── contacts.py            # 連絡先サービス
│   │   │
│   │   ├── db/                        # データベース関連
│   │   │   ├── __init__.py
│   │   │   ├── database.py            # DB接続設定
│   │   │   ├── session.py             # セッション管理
│   │   │   └── migrations/            # マイグレーションファイル
│   │   │       └── alembic/
│   │   │
│   │   └── utils/                     # ユーティリティ
│   │       ├── __init__.py
│   │       ├── email_utils.py
│   │       ├── crypto_utils.py
│   │       └── file_utils.py
│   │
│   ├── tests/                         # テストファイル
│   │   ├── __init__.py
│   │   ├── conftest.py                # pytest設定
│   │   ├── test_api/
│   │   ├── test_services/
│   │   └── test_utils/
│   │
│   ├── alembic.ini                    # Alembic設定
│   ├── pytest.ini                     # pytest設定
│   ├── requirements.txt               # 本番依存関係
│   ├── requirements-dev.txt           # 開発依存関係
│   └── .env.example                   # 環境変数例
│
├── data/                              # データファイル
│   ├── translations/                  # 翻訳ファイル
│   │   ├── common.json               # 共通翻訳（ボタン、メニュー等）
│   │   ├── mail.json                 # メール関連翻訳
│   │   ├── settings.json             # 設定画面翻訳
│   │   ├── search.json               # 検索関連翻訳
│   │   ├── tags.json                 # タグ関連翻訳
│   │   └── errors.json               # エラーメッセージ翻訳
│   │
│   ├── templates/                     # メールテンプレート
│   │   ├── welcome.html
│   │   └── signature.html
│   │
│   └── samples/                       # サンプルデータ（開発用）
│       ├── sample-emails.json
│       └── sample-contacts.json
│
├── shared/                            # 共有リソース
│   ├── types/                         # TypeScript/Python共通型定義
│   │   ├── i18n.ts                   # 多言語対応型定義
│   │   ├── api.ts                    # API型定義
│   │   ├── mail.ts                   # メール型定義
│   │   └── database.ts               # データベース型定義
│   │
│   └── constants/                     # 定数定義
│       ├── email.ts                  # メール関連定数
│       ├── api.ts                    # API関連定数
│       └── ui.ts                     # UI関連定数
│
├── scripts/                           # ユーティリティスクリプト
│   ├── validate-translations.js      # 翻訳バリデーション
│   ├── generate-types.js             # 型定義自動生成
│   ├── setup-dev.js                  # 開発環境セットアップ
│   ├── build.js                      # ビルドスクリプト
│   └── migrate-db.py                 # DB マイグレーション
│
├── docs/                              # ドキュメント
│   ├── api/                          # API ドキュメント
│   │   ├── openapi.json
│   │   └── endpoints.md
│   ├── development/                   # 開発ドキュメント
│   │   ├── setup.md                  # 環境構築
│   │   ├── contributing.md           # コントリビューション
│   │   └── architecture.md           # アーキテクチャ
│   └── user/                         # ユーザードキュメント
│       └── user-guide.md
│
├── .gitignore
├── .env.example                       # 環境変数例
├── docker-compose.yml                 # 開発用Docker設定
├── README.md                          # プロジェクト概要
└── README_plan.md                     # 開発計画詳細（このファイル）
```

## 13. データ保存場所設計

### 13.1 基本方針
- ベンダー専用ディレクトリ（SNGDesign）配下にアプリ別ディレクトリを配置
- プラットフォーム固有の標準的な場所を使用
- セキュリティとプライバシーを考慮した保存戦略
- 将来的な他のSNGDesignアプリとの一貫性を保持

### 13.2 プラットフォーム別データ保存場所

#### Windows
```
メインデータディレクトリ:
C:\Users\{username}\AppData\Roaming\SNGDesign\MailApp\

詳細構造:
C:\Users\{username}\AppData\Roaming\SNGDesign\MailApp\
├── data\                              # メインデータ
│   ├── mail.db                        # SQLiteデータベース
│   ├── emails\                        # メール本文ファイル
│   │   ├── 2024\01\                   # 年月別フォルダ
│   │   └── ...
│   ├── attachments\                   # 添付ファイル
│   │   ├── {email_id}\                # メールID別フォルダ
│   │   └── ...
│   └── search\                        # 検索インデックス
│       └── fts_index.db
├── config\                            # 設定ファイル
│   ├── settings.json                  # アプリ設定
│   ├── accounts.json                  # アカウント設定（暗号化）
│   └── ui-state.json                  # UI状態
├── cache\                             # キャッシュファイル
│   ├── thumbnails\                    # 画像サムネイル
│   └── temp\                          # 一時ファイル
└── logs\                              # ログファイル
    ├── app.log                        # アプリケーションログ
    ├── sync.log                       # 同期ログ
    └── error.log                      # エラーログ

代替場所（ユーザー設定可能）:
- C:\Users\{username}\Documents\SNGDesign\MailApp\
- D:\MailData\SNGDesign\MailApp\（カスタムドライブ）
```

#### macOS
```
メインデータディレクトリ:
~/Library/Application Support/SNGDesign/MailApp/

詳細構造:
~/Library/Application Support/SNGDesign/MailApp/
├── data/
│   ├── mail.db
│   ├── emails/
│   ├── attachments/
│   └── search/
├── config/
│   ├── settings.json
│   └── accounts.json
├── cache/
└── logs/

設定ファイル（システム連携用）:
~/Library/Preferences/com.sngdesign.mailapp.plist

キーチェーン（パスワード保存）:
macOS Keychain（Electronの safeStorage使用）

代替場所:
- ~/Documents/SNGDesign/MailApp/
- /Volumes/ExternalDrive/SNGDesign/MailApp/
```

#### Linux
```
メインデータディレクトリ:
~/.local/share/sngdesign/mailapp/

詳細構造:
~/.local/share/sngdesign/mailapp/
├── data/
├── cache/
└── logs/

設定ファイル:
~/.config/sngdesign/mailapp/
├── settings.json
└── accounts.json

代替場所:
- ~/Documents/SNGDesign/MailApp/
- /mnt/storage/SNGDesign/MailApp/
```

#### Android
```
アプリ専用内部ストレージ:
/data/data/com.sngdesign.mailapp/files/
├── data/
├── config/
└── cache/

外部ストレージ（ユーザーアクセス可能）:
/storage/emulated/0/Android/data/com.sngdesign.mailapp/files/
├── exports/                           # エクスポートファイル
├── backups/                          # バックアップファイル
└── attachments/                      # 大きな添付ファイル

共有ストレージ（ユーザー設定）:
/storage/emulated/0/SNGDesign/MailApp/
```

#### iOS
```
アプリサンドボックス:
{App Container}/
├── Documents/                         # iCloudバックアップ対象
│   ├── data/
│   │   ├── mail.db
│   │   ├── emails/
│   │   └── attachments/
│   └── exports/                       # ユーザーエクスポートファイル
├── Library/
│   ├── Application Support/           # アプリサポートデータ
│   │   └── SNGDesign/MailApp/
│   ├── Caches/                       # キャッシュ（バックアップ対象外）
│   └── Preferences/                  # 設定
└── tmp/                              # 一時ファイル

iCloud同期（オプション）:
{iCloud Container}/Documents/SNGDesign/MailApp/
```

### 13.3 実装例

#### Electronでのパス管理
```typescript
// gui/src/main/storage-paths.ts
import { app } from 'electron';
import path from 'path';
import os from 'os';

export class StorageManager {
  private static instance: StorageManager;
  private paths: Record<string, string>;

  private constructor() {
    this.paths = this.initializePaths();
  }

  static getInstance(): StorageManager {
    if (!StorageManager.instance) {
      StorageManager.instance = new StorageManager();
    }
    return StorageManager.instance;
  }

  private initializePaths(): Record<string, string> {
    const platform = os.platform();
    const userDataPath = app.getPath('userData');
    
    // ベンダーディレクトリの確保
    let vendorPath: string;
    
    switch (platform) {
      case 'win32':
        vendorPath = path.join(app.getPath('appData'), 'SNGDesign', 'MailApp');
        break;
      case 'darwin':
        vendorPath = path.join(app.getPath('appData'), 'SNGDesign', 'MailApp');
        break;
      case 'linux':
        vendorPath = path.join(os.homedir(), '.local', 'share', 'sngdesign', 'mailapp');
        break;
      default:
        vendorPath = userDataPath;
    }

    return {
      // メインディレクトリ
      vendor: path.dirname(vendorPath),
      app: vendorPath,
      
      // データディレクトリ
      data: path.join(vendorPath, 'data'),
      database: path.join(vendorPath, 'data', 'mail.db'),
      emails: path.join(vendorPath, 'data', 'emails'),
      attachments: path.join(vendorPath, 'data', 'attachments'),
      searchIndex: path.join(vendorPath, 'data', 'search'),
      
      // 設定ディレクトリ
      config: path.join(vendorPath, 'config'),
      settings: path.join(vendorPath, 'config', 'settings.json'),
      accounts: path.join(vendorPath, 'config', 'accounts.json'),
      
      // キャッシュとログ
      cache: path.join(vendorPath, 'cache'),
      logs: path.join(vendorPath, 'logs'),
      temp: path.join(app.getPath('temp'), 'SNGDesign', 'MailApp')
    };
  }

  // パス取得メソッド
  getPath(key: string): string {
    return this.paths[key] || '';
  }

  getAllPaths(): Record<string, string> {
    return { ...this.paths };
  }

  // カスタムパス設定（ユーザー設定）
  setCustomPath(customPath: string): void {
    // バリデーションとパス更新
    // 設定ファイルに保存
  }

  // ディレクトリ初期化
  async initializeDirectories(): Promise<void> {
    const fs = await import('fs/promises');
    
    for (const [key, dirPath] of Object.entries(this.paths)) {
      if (key !== 'settings' && key !== 'accounts' && key !== 'database') {
        try {
          await fs.mkdir(dirPath, { recursive: true });
        } catch (error) {
          console.error(`Failed to create directory ${dirPath}:`, error);
        }
      }
    }
  }
}

// 使用例
export const getStoragePaths = () => StorageManager.getInstance().getAllPaths();
export const getStoragePath = (key: string) => StorageManager.getInstance().getPath(key);
```

### 13.4 セキュリティ考慮事項

#### データ暗号化
```typescript
// 機密データの暗号化
- アカウント情報: Electron safeStorage使用
- メールデータベース: SQLCipher使用
- 添付ファイル: AES-256暗号化
```

#### アクセス制御
```
- ファイルパーミッション: 所有者のみ読み書き可能（600/700）
- プロセス分離: メインプロセスでのみデータアクセス
- 一時ファイル: 自動削除とセキュアクリア
```

### 13.5 バックアップ・同期戦略

#### 自動バックアップ
```typescript
// 定期バックアップ機能
interface BackupConfig {
  enabled: boolean;
  interval: 'daily' | 'weekly' | 'monthly';
  location: 'local' | 'cloud' | 'external';
  retention: number; // 保持するバックアップ数
}
```

#### 設定可能な保存場所
```typescript
interface StorageConfig {
  primary: string;      // メインの保存場所
  backup?: string;      // バックアップ保存場所
  sync?: {              // 同期設定
    enabled: boolean;
    service: 'onedrive' | 'googledrive' | 'dropbox';
    path: string;
  };
}
```

### 13.6 メリット

1. **一貫性**: 全プラットフォームで統一されたディレクトリ構造
2. **拡張性**: 将来の他のSNGDesignアプリとの統合が容易
3. **管理性**: ユーザーが複数のSNGDesignアプリを使用する場合の一元管理
4. **標準準拠**: 各プラットフォームの標準的な場所を使用
5. **セキュリティ**: 適切なアクセス制御と暗号化
6. **バックアップ**: 容易なバックアップと復元

## 14. UI/UX設計（詳細）

### 14.1 基本デザインコンセプト

#### デザイン哲学
- **美しさと機能性の両立**: 美しい視覚体験と実用的な機能を組み合わせ
- **情報の階層化**: 重要な情報から段階的に詳細を表示
- **温かみのあるコミュニケーション**: 手紙のような温かい交流体験
- **シンプルで直感的**: 複雑な機能を簡潔なインターフェースで提供

#### ターゲット体験
```
起動 → 美しいホーム画面でウェルカム → 概要確認 → 詳細操作
     ↑                                              ↓
   リラックス                                    集中作業
```

### 14.2 ホーム画面設計

#### レイアウト構成
```
┌─────────────────────────────────────────────────────┐
│  [美しい背景写真 - 時間帯/季節対応グラデーションオーバーレイ]    │
│                                                     │
│  SNGDesign Mail                            🌅 朝    │
│                                                     │
│       📧 未読メール: 12件                           │
│       📩 今日の新着: 5件                           │
│       ⏰ 次の予定: 14:00 会議                       │
│                                                     │
│  最新メッセージ                                      │
│  ┌─────────────────────────────────────────────────┐  │
│  │ 👤 山田太郎                          15分前    │  │
│  │ 件名: 会議の件について確認したいことが...        │  │
│  │ プレビュー: 明日の会議資料についてですが...      │  │
│  └─────────────────────────────────────────────────┘  │
│                                                     │
│  ┌─────────────────────────────────────────────────┐  │
│  │ 👤 佐藤花子                          1時間前   │  │
│  │ 件名: プロジェクトの進捗報告                    │  │
│  │ プレビュー: お疲れ様です。先週から進めて...      │  │
│  └─────────────────────────────────────────────────┘  │
│                                                     │
│              [すべてのメールを見る]                  │
│                                                     │
└─────────────────────────────────────────────────────┘
```

#### 背景写真システム
```typescript
interface BackgroundConfig {
  timeOfDay: {
    morning: string[];   // 朝の背景画像URLs
    afternoon: string[]; // 昼の背景画像URLs
    evening: string[];   // 夕方の背景画像URLs
    night: string[];     // 夜の背景画像URLs
  };
  season: {
    spring: string[];
    summer: string[];
    autumn: string[];
    winter: string[];
  };
  weather?: string[];    // 将来: 天気連携
}
```

#### 情報表示の優先順位
1. **最重要**: 未読メール数、緊急フラグ付きメール
2. **重要**: 今日の新着、VIPからのメール
3. **参考**: 予定、リマインダー
4. **詳細**: 最新メッセージのプレビュー

### 14.3 サイドバー設計

#### サイドバー構成
```
┌─────┐
│  🏠  │ ← ホーム (ホットキー: H)
├─────┤
│  📥  │ ← 受信箱 (ホットキー: I) + 未読数バッジ
├─────┤
│  📝  │ ← 下書き (ホットキー: D) + 下書き数
├─────┤
│  📤  │ ← 送信済み (ホットキー: S)
├─────┤
│  🗑️  │ ← ゴミ箱 (ホットキー: T)
├─────┤
│  🏷️  │ ← タグ管理 (ホットキー: G)
├─────┤
│  👥  │ ← アドレス帳 (ホットキー: C)
├─────┤
│  🔍  │ ← 検索 (ホットキー: /)
├─────┤
│  ⚙️  │ ← 設定 (ホットキー: ,)
└─────┘

下部固定:
┌─────┐
│  🌙  │ ← ダークモード切替
├─────┤
│  🌐  │ ← 言語切替
├─────┤
│  ❓  │ ← ヘルプ
└─────┘
```

#### アイコン仕様
- **サイズ**: 24x24px (高DPI対応48x48px)
- **スタイル**: アウトライン + フィル状態
- **アニメーション**: ホバー時の微細なスケールアップ (1.05倍)
- **通知バッジ**: 赤色の小さな円、数字表示

#### インタラクション詳細
```css
.sidebar-item {
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  position: relative;
}

.sidebar-item:hover {
  transform: scale(1.05);
  background: rgba(99, 102, 241, 0.1);
}

.sidebar-item.active {
  background: rgba(99, 102, 241, 0.2);
  box-shadow: inset 3px 0 0 #6366f1;
}

.notification-badge {
  position: absolute;
  top: -4px;
  right: -4px;
  background: #ef4444;
  color: white;
  border-radius: 50%;
  min-width: 18px;
  height: 18px;
  font-size: 11px;
  font-weight: 600;
}
```

### 14.4 手紙風チャット表示設計

#### 基本レイアウト
```
スレッド表示例:
┌─────────────────────────────────────────────────┐
│  件名: Re: プロジェクトについて          🔒 暗号化  │
│  参加者: 山田太郎, 佐藤花子, 自分                  │
├─────────────────────────────────────────────────┤
│                                                 │
│  [受信メール - 左寄せ]                          │
│  ┌─────────────────────────────────────────┐     │
│  │ 📥 山田太郎                 2024/01/15  │     │
│  │                            14:30      │     │
│  │ ┌─────────────────────────────────────┐ │     │
│  │ │ こんにちは！                        │ │     │
│  │ │                                     │ │     │
│  │ │ プロジェクトの件でご相談があります。  │ │     │
│  │ │ 明日お時間はありますでしょうか？      │ │     │
│  │ └─────────────────────────────────────┘ │     │
│  └─────────────────────────────────────────┘     │
│                                                 │
│     [送信メール - 右寄せ]                        │
│     ┌─────────────────────────────────────────┐   │
│     │ 📤 自分                     2024/01/15 │   │
│     │                            15:45     │   │
│     │ ┌─────────────────────────────────────┐ │   │
│     │ │ お疲れ様です。                      │ │   │
│     │ │                                     │ │   │
│     │ │ 明日の午前中でしたら空いています。    │ │   │
│     │ │ 10時からいかがでしょうか？          │ │   │
│     │ │                                     │ │   │
│     │ │ 添付: 会議資料.pdf 📎              │ │   │
│     │ └─────────────────────────────────────┘ │   │
│     └─────────────────────────────────────────┘   │
│                                                 │
└─────────────────────────────────────────────────┘
```

#### 視覚的デザイン要素

**受信メール（左寄せ）**
```css
.received-message {
  background: linear-gradient(135deg, #fffbeb 0%, #fef3c7 100%);
  border: 1px solid #f59e0b;
  border-radius: 12px 12px 12px 4px;
  box-shadow: 
    0 2px 4px rgba(245, 158, 11, 0.1),
    inset 0 1px 0 rgba(255, 255, 255, 0.8);
  margin-left: 12px;
  margin-right: 64px;
  position: relative;
}

.received-message::before {
  content: "📮";
  position: absolute;
  top: -8px;
  left: 16px;
  font-size: 16px;
}
```

**送信メール（右寄せ）**
```css
.sent-message {
  background: linear-gradient(135deg, #eff6ff 0%, #dbeafe 100%);
  border: 1px solid #bfdbfe;
  border-radius: 12px 12px 4px 12px;
  box-shadow: 
    0 1px 3px rgba(59, 130, 246, 0.1),
    inset 0 1px 0 rgba(255, 255, 255, 0.6);
  margin-left: 64px;
  margin-right: 12px;
  position: relative;
  margin-left: auto;
}

.sent-message::before {
  content: "✓✓";
  position: absolute;
  bottom: 4px;
  right: 8px;
  color: #3b82f6;
  font-size: 10px;
}
```

**時間表示**
```css
.message-time {
  font-family: 'Caveat', cursive; /* 手書き風フォント */
  font-size: 12px;
  color: #6b7280;
  margin-bottom: 8px;
}

.day-separator {
  text-align: center;
  margin: 24px 0;
  position: relative;
}

.day-separator::before {
  content: "";
  position: absolute;
  top: 50%;
  left: 0;
  right: 0;
  height: 1px;
  background: linear-gradient(to right, transparent, #d1d5db, transparent);
}

.day-separator span {
  background: #f9fafb;
  padding: 4px 16px;
  color: #6b7280;
  font-size: 12px;
  font-weight: 500;
}
```

### 14.5 カラーパレット & デザインシステム

#### メインカラーパレット
```css
:root {
  /* プライマリカラー */
  --color-primary-50: #eff6ff;
  --color-primary-500: #3b82f6;
  --color-primary-600: #2563eb;
  --color-primary-700: #1d4ed8;

  /* セカンダリカラー */
  --color-secondary-50: #f9fafb;
  --color-secondary-100: #f3f4f6;
  --color-secondary-500: #6b7280;
  --color-secondary-700: #374151;

  /* アクセントカラー */
  --color-accent-emerald: #10b981;
  --color-accent-amber: #f59e0b;
  --color-accent-rose: #f43f5e;

  /* セマンティックカラー */
  --color-success: #10b981;
  --color-warning: #f59e0b;
  --color-error: #ef4444;
  --color-info: #3b82f6;

  /* メール状態カラー */
  --color-mail-sent: #eff6ff;
  --color-mail-received: #fffbeb;
  --color-mail-unread: #fef3c7;
  --color-mail-important: #fef2f2;
}
```

#### タイポグラフィ
```css
:root {
  /* フォントファミリー */
  --font-primary: 'Inter', 'Hiragino Kaku Gothic ProN', 'Meiryo', sans-serif;
  --font-handwriting: 'Caveat', 'Klee One', cursive;
  --font-mono: 'JetBrains Mono', 'Consolas', monospace;

  /* フォントサイズ */
  --text-xs: 0.75rem;    /* 12px */
  --text-sm: 0.875rem;   /* 14px */
  --text-base: 1rem;     /* 16px */
  --text-lg: 1.125rem;   /* 18px */
  --text-xl: 1.25rem;    /* 20px */
  --text-2xl: 1.5rem;    /* 24px */
  --text-3xl: 1.875rem;  /* 30px */

  /* 行間 */
  --leading-tight: 1.25;
  --leading-normal: 1.5;
  --leading-relaxed: 1.625;
}
```

### 14.6 レスポンシブ対応

#### ブレークポイント戦略
```css
/* デスクトップファースト */
.layout-grid {
  display: grid;
  grid-template-columns: 64px 1fr;
  height: 100vh;
}

/* タブレット (768px以下) */
@media (max-width: 768px) {
  .layout-grid {
    grid-template-columns: 1fr;
  }
  
  .sidebar {
    position: fixed;
    left: 0;
    top: 0;
    height: 100vh;
    width: 64px;
    z-index: 1000;
    transform: translateX(-100%);
    transition: transform 0.3s cubic-bezier(0.4, 0, 0.2, 1);
  }
  
  .sidebar.open {
    transform: translateX(0);
  }
}

/* スマートフォン (480px以下) */
@media (max-width: 480px) {
  .home-preview-card {
    margin: 8px;
    padding: 12px;
  }
  
  .chat-message {
    max-width: 85%;
    margin: 8px 12px;
  }
}
```

### 14.7 アニメーション & インタラクション

#### 画面遷移アニメーション
```css
/* ページ遷移 */
.page-transition-enter {
  opacity: 0;
  transform: translateY(16px);
}

.page-transition-enter-active {
  opacity: 1;
  transform: translateY(0);
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

/* メール読み込み */
.mail-item-enter {
  opacity: 0;
  transform: scale(0.95);
}

.mail-item-enter-active {
  opacity: 1;
  transform: scale(1);
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}

/* 新着メール通知 */
@keyframes newMailPulse {
  0%, 100% { 
    box-shadow: 0 0 0 0 rgba(59, 130, 246, 0.4); 
  }
  50% { 
    box-shadow: 0 0 0 8px rgba(59, 130, 246, 0); 
  }
}

.new-mail-notification {
  animation: newMailPulse 2s infinite;
}
```

#### マイクロインタラクション
```typescript
// ボタンホバー効果
const buttonHoverEffect = {
  scale: 1.02,
  boxShadow: "0 4px 12px rgba(0, 0, 0, 0.15)",
  transition: {
    type: "spring",
    stiffness: 400,
    damping: 30
  }
};

// メール既読アニメーション
const markAsReadAnimation = {
  backgroundColor: ["#fef3c7", "#f3f4f6"],
  transition: { duration: 0.5 }
};
```

### 14.8 アクセシビリティ対応

#### キーボードナビゲーション
```typescript
const keyboardShortcuts = {
  'h': 'ホーム画面へ',
  'i': '受信箱へ',
  'c': '新規作成',
  'r': '返信',
  'f': '転送',
  'd': '削除',
  '/': '検索',
  'Escape': 'モーダル閉じる',
  'Enter': '選択/実行',
  'ArrowUp/Down': 'メール選択'
};
```

#### スクリーンリーダー対応
```html
<!-- セマンティックHTML -->
<main role="main" aria-label="メールアプリケーション">
  <nav role="navigation" aria-label="サイドバーメニュー">
    <button aria-label="受信箱 (12件の未読メール)">
      📥
      <span class="sr-only">受信箱</span>
      <span aria-live="polite">12</span>
    </button>
  </nav>
  
  <section role="main" aria-label="メール一覧">
    <article aria-label="山田太郎からのメール: 会議の件について">
      <!-- メール内容 -->
    </article>
  </section>
</main>
```

### 14.9 ダークモード対応

#### ダークテーマカラーパレット
```css
[data-theme="dark"] {
  --color-bg-primary: #111827;
  --color-bg-secondary: #1f2937;
  --color-text-primary: #f9fafb;
  --color-text-secondary: #d1d5db;
  
  --color-mail-sent: #1e3a8a;
  --color-mail-received: #92400e;
  --color-sidebar: #0f172a;
}

/* 自動切り替え対応 */
@media (prefers-color-scheme: dark) {
  :root:not([data-theme]) {
    /* ダークモードの変数を適用 */
  }
}
```

この設計により、美しく実用的で、アクセシビリティに配慮したメールクライアントが実現できます。
