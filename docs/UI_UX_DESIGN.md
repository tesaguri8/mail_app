# UI/UX 設計

**ステータス:** 計画（実装未着手）
**出典:** 旧 `README_plan.md` §14 を整理。内容はスタック非依存（デザイン意図）であり、ほぼそのまま維持。

---

## 1. 基本デザインコンセプト

### デザイン哲学
- **美しさと機能性の両立**: 美しい視覚体験と実用的な機能を組み合わせ
- **アンビエント（常駐したくなる）**: 画面のどこかに常に置いておきたくなる佇まい。デスクトップに馴染む“風景”のようなアプリ
- **情報の階層化**: ホームでは最小限、作業は詳細ページで
- **温かみのあるコミュニケーション**: 手紙のような温かい交流体験
- **シンプルで直感的**: 複雑な機能を簡潔なインターフェースで提供

### ターゲット体験
```
起動 → 全面が美しい画像のホーム（ダッシュボード）→ 概要を一望 → 詳細ページで作業
     ↑                                                              ↓
  小さくして常駐                                                  集中作業
  （時計・日付ウィジェット化）
```

このアプリは「メール / 住所録 / カレンダー」を束ねる**パーソナルなホーム**であり、普段は小さくして
時計・日付ウィジェットのように置いておけることを重視する。

---

## 1.5 ウィンドウモデル（フレームレス全面ビジュアル）

- **フレームレス**: OS 標準のタイトルバー枠を持たず、ウィンドウ全面を背景画像が覆う（カスタムメニューバー領域も画像の上に重ねる）。Tauri の `decorations: false` を使用し、ドラッグ移動は `data-tauri-drag-region` で実現する（Primadoc / Doculator と同パターン）。
- **カスタムタイトルバー**: 最小化・最大化・閉じる等は画像に溶け込む半透明オーバーレイで自作。
- **常駐性**: 「最前面に固定（always-on-top）」をトグルで提供。小さくしてもデスクトップ上に残せる。
- **2 つの表示モード**: ウィンドウサイズに応じて自動的に切り替える（下記）。

| モード | 条件（目安） | 表示内容 |
|--------|-------------|---------|
| **ダッシュボード** | 通常〜大サイズ | 背景画像 + 概要パネル（未読・新着・次の予定 等）+ ナビゲーション |
| **ウィジェット（コンパクト）** | 小サイズ（しきい値以下） | 背景画像 + 大きな時計・日付。最小限のバッジ（未読数・次の予定）のみ |

### ナビゲーションモデル
```
ホーム（ダッシュボード）
 ├─ 統合インボックス（メール + SNS DM/コメントを横断表示・チャット形式）
 ├─ メール（一覧 / チャット表示 / 作成）
 ├─ 住所録（連絡先 / グループ）
 ├─ カレンダー（月 / 週 / 予定詳細）
 └─ 設定
```
ホームを起点に各詳細ページへ遷移し、作業後はホームへ戻る。ホーム自体が「置いておく」対象。

> **メッセージハブ**: メールと SNS（LINE / Instagram / Messenger / WhatsApp）の DM・コメントを共通スキーマに正規化し、同じチャット形式ビュー（§4）で一元表示する。チャネルはアイコンで識別。ホーム/ウィジェットの未読バッジは**全チャネル合算**で表示し、取りこぼしを防ぐ。詳細は [SNS_INTEGRATION.md](SNS_INTEGRATION.md)。

> **保護領域の表示トグル**: 本文は伏字／実値をワンタッチ切替。作成時は「相手・AIに渡る姿（伏字）」を**送信前に目視確認**でき、安心して送れる。保護領域はハイライト表示。詳細は [PROTECTED_REGIONS.md](PROTECTED_REGIONS.md) §5.5。

---

## 2. ホーム画面設計（ダッシュボードモード）

ウィンドウ全面が背景画像。その上に半透明オーバーレイで概要パネルとナビゲーションを重ねる。
枠（タイトルバー）も画像領域に含め、ウィンドウの隅々まで“風景”が広がる。

### レイアウト構成
```
┌─────────────────────────────────────────────────────┐
│ ○ ○ ○                              ⤢ 📌 _ □ ✕ │ ← 画像に溶け込む自作タイトルバー
│  [美しい背景写真 - 時間帯/季節対応グラデーションオーバーレイ]    │
│                                                     │
│  09:41                                     🌅 朝    │ ← 時計（ホームでも常時表示）
│  2026年6月30日(火)                                  │
│                                                     │
│       📧 未読メール: 12件                           │
│       📩 今日の新着: 5件                           │
│       ⏰ 次の予定: 14:00 会議                       │
│       👥 今日の連絡: 佐藤さん 誕生日                 │
│                                                     │
│  最新メッセージ                                      │
│  ┌─────────────────────────────────────────────────┐  │
│  │ 👤 山田太郎                          15分前    │  │
│  │ 件名: 会議の件について確認したいことが...        │  │
│  └─────────────────────────────────────────────────┘  │
│                                                     │
│   [メール]   [住所録]   [カレンダー]   [設定]        │ ← 詳細ページへの導線
└─────────────────────────────────────────────────────┘
```

> `📌` は always-on-top トグル、`⤢` はウィジェット/ダッシュボード切替（または単にリサイズで自動切替）。

## 2.5 ウィジェット（コンパクト）モード

ウィンドウを小さくすると、ダッシュボードの情報量を落とし、**美しい背景の上に大きな時計・日付**を
表示する“置き時計”のような佇まいに切り替わる。デスクトップの隅に常駐させても画面に馴染む。

```
┌───────────────────────────┐
│ [美しい背景写真]          📌 │ ← always-on-top トグルのみ
│                           │
│        09:41              │ ← 大きな時計
│     2026/6/30 (火)        │ ← 日付
│                           │
│   📧 12   ⏰ 14:00 会議    │ ← 最小限のバッジ（未読数・次の予定）
└───────────────────────────┘
```

- **切替**: ウィンドウ幅/高さのしきい値で自動切替（明示トグルも用意）。
- **表示要素**: 時計・日付を主役に、未読数・直近予定・誕生日などを控えめなバッジで。
- **操作**: クリックで該当詳細ページへ。ホバーで情報をわずかに展開。
- **常駐**: フレームレス + always-on-top + ドラッグ移動で、ウィジェットアプリのように扱える。
- **省電力**: 非アクティブ時は更新頻度を下げる（時計は分単位、同期は間隔制御）。

---

### 背景写真システム

背景は **アプリ同梱画像** と **ユーザーが取り込んだ画像** の両方から選べる。既定は**自動ローテーション**（時間帯／日替わり）。

```typescript
type BackgroundSource = 'app' | 'user';

// 表示モード（既定は 'time' or 'daily' の自動ローテーション）
type BackgroundMode =
  | 'fixed'    // 1枚を固定
  | 'time'     // 時間帯（朝/昼/夕/夜）で切替
  | 'daily'    // 日替わり
  | 'season'   // 季節で切替
  | 'random';  // ランダム

interface BackgroundImage {
  id: string;
  source: BackgroundSource;     // 'app' = 同梱, 'user' = 取り込み
  path: string;                 // user: ローカルパス / app: リソースキー
  thumbnail?: string;
  timeOfDay?: 'morning' | 'afternoon' | 'evening' | 'night';
  season?: 'spring' | 'summer' | 'autumn' | 'winter';
}

interface BackgroundConfig {
  mode: BackgroundMode;         // 既定: 'time'（時間帯）/ 'daily'（日替わり）
  fixedImageId?: string;        // mode='fixed' のとき
  pool: BackgroundImage[];      // ローテーション対象（app + user の混在可）
  weather?: boolean;            // 将来: 天気連携
}
```

#### ユーザー画像の取り込み
- **インポート**: ファイル選択（`@tauri-apps/plugin-dialog`）→ 形式/サイズ検証（jpg/png/webp・上限）→ アプリ保存領域へ**コピー**（元ファイルは参照しない）→ サムネ生成（Rust `image`）→ DB 登録。巨大画像は縮小も検討。
- **保存先**: `…\tesaguri.rondine.dev\media\backgrounds\`（サムネは `cache\thumbnails\`）。詳細は [DATA_STORAGE.md](DATA_STORAGE.md)。
- **選択 UI**: 同梱画像とユーザー画像をギャラリー表示し、固定表示にするか、ローテーション対象に含めるかを選ぶ。
- **WebView 表示**: ローカル画像表示には Tauri の **asset プロトコル**有効化＋**CSP `img-src`** 許可が必要（Primadoc と同構成）。

> ローテーション既定（時間帯／日替わり）でも、ユーザーは設定でモード（固定/時間/日替わり/季節/ランダム）を切替できる。

### 情報表示の優先順位
1. **最重要**: 未読メール数、緊急フラグ付きメール
2. **重要**: 今日の新着、VIPからのメール
3. **参考**: 予定、リマインダー
4. **詳細**: 最新メッセージのプレビュー

---

## 3. サイドバー設計

### サイドバー構成
```
🏠 ホーム (H)
📥 受信箱 (I) + 未読数バッジ
📝 下書き (D) + 下書き数
📤 送信済み (S)
🗑️ ゴミ箱 (T)
🏷️ タグ管理 (G)
👥 アドレス帳 (C)
🔍 検索 (/)
⚙️ 設定 (,)

下部固定:
🌙 ダークモード切替
🌐 言語切替
❓ ヘルプ
```

### アイコン仕様
- **サイズ**: 24x24px（高DPI対応48x48px）
- **スタイル**: アウトライン + フィル状態
- **アニメーション**: ホバー時の微細なスケールアップ（1.05倍）
- **通知バッジ**: 赤色の小さな円、数字表示

### インタラクション詳細
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

---

## 4. 手紙風チャット表示設計

### 基本レイアウト
```
スレッド表示例:
┌─────────────────────────────────────────────────┐
│  件名: Re: プロジェクトについて          🔒 暗号化  │
│  参加者: 山田太郎, 佐藤花子, 自分                  │
├─────────────────────────────────────────────────┤
│  [受信メール - 左寄せ]                          │
│  ┌─────────────────────────────────────────┐     │
│  │ 📥 山田太郎                 2024/01/15  │     │
│  │ こんにちは！                            │     │
│  │ プロジェクトの件でご相談があります。      │     │
│  └─────────────────────────────────────────┘     │
│                                                 │
│     [送信メール - 右寄せ]                        │
│     ┌─────────────────────────────────────────┐   │
│     │ 📤 自分                     2024/01/15 │   │
│     │ 明日の午前中でしたら空いています。        │   │
│     │ 添付: 会議資料.pdf 📎                  │   │
│     └─────────────────────────────────────────┘   │
└─────────────────────────────────────────────────┘
```

### 視覚的デザイン要素

**受信メール（左寄せ）**
```css
.received-message {
  background: linear-gradient(135deg, #fffbeb 0%, #fef3c7 100%);
  border: 1px solid #f59e0b;
  border-radius: 12px 12px 12px 4px;
  box-shadow: 0 2px 4px rgba(245, 158, 11, 0.1), inset 0 1px 0 rgba(255, 255, 255, 0.8);
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
  box-shadow: 0 1px 3px rgba(59, 130, 246, 0.1), inset 0 1px 0 rgba(255, 255, 255, 0.6);
  margin-left: auto;
  margin-right: 12px;
  position: relative;
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

**時間表示・日付セパレータ**
```css
.message-time {
  font-family: 'Caveat', cursive; /* 手書き風フォント */
  font-size: 12px;
  color: #6b7280;
  margin-bottom: 8px;
}
.day-separator { text-align: center; margin: 24px 0; position: relative; }
.day-separator::before {
  content: "";
  position: absolute; top: 50%; left: 0; right: 0; height: 1px;
  background: linear-gradient(to right, transparent, #d1d5db, transparent);
}
.day-separator span {
  background: #f9fafb; padding: 4px 16px; color: #6b7280; font-size: 12px; font-weight: 500;
}
```

---

## 5. カラーパレット & デザインシステム

### メインカラーパレット
```css
:root {
  /* プライマリ */
  --color-primary-50: #eff6ff;
  --color-primary-500: #3b82f6;
  --color-primary-600: #2563eb;
  --color-primary-700: #1d4ed8;
  /* セカンダリ */
  --color-secondary-50: #f9fafb;
  --color-secondary-100: #f3f4f6;
  --color-secondary-500: #6b7280;
  --color-secondary-700: #374151;
  /* アクセント */
  --color-accent-emerald: #10b981;
  --color-accent-amber: #f59e0b;
  --color-accent-rose: #f43f5e;
  /* セマンティック */
  --color-success: #10b981;
  --color-warning: #f59e0b;
  --color-error: #ef4444;
  --color-info: #3b82f6;
  /* メール状態 */
  --color-mail-sent: #eff6ff;
  --color-mail-received: #fffbeb;
  --color-mail-unread: #fef3c7;
  --color-mail-important: #fef2f2;
}
```

### タイポグラフィ
```css
:root {
  --font-primary: 'Inter', 'Hiragino Kaku Gothic ProN', 'Meiryo', sans-serif;
  --font-handwriting: 'Caveat', 'Klee One', cursive;
  --font-mono: 'JetBrains Mono', 'Consolas', monospace;

  --text-xs: 0.75rem;   --text-sm: 0.875rem; --text-base: 1rem;
  --text-lg: 1.125rem;  --text-xl: 1.25rem;  --text-2xl: 1.5rem; --text-3xl: 1.875rem;

  --leading-tight: 1.25; --leading-normal: 1.5; --leading-relaxed: 1.625;
}
```

---

## 6. レスポンシブ対応

```css
/* デスクトップファースト */
.layout-grid { display: grid; grid-template-columns: 64px 1fr; height: 100vh; }

/* タブレット (768px以下) */
@media (max-width: 768px) {
  .layout-grid { grid-template-columns: 1fr; }
  .sidebar {
    position: fixed; left: 0; top: 0; height: 100vh; width: 64px; z-index: 1000;
    transform: translateX(-100%);
    transition: transform 0.3s cubic-bezier(0.4, 0, 0.2, 1);
  }
  .sidebar.open { transform: translateX(0); }
}

/* スマートフォン (480px以下) */
@media (max-width: 480px) {
  .home-preview-card { margin: 8px; padding: 12px; }
  .chat-message { max-width: 85%; margin: 8px 12px; }
}
```

---

## 7. アニメーション & インタラクション

```css
/* ページ遷移 */
.page-transition-enter { opacity: 0; transform: translateY(16px); }
.page-transition-enter-active {
  opacity: 1; transform: translateY(0);
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}
/* メール読み込み */
.mail-item-enter { opacity: 0; transform: scale(0.95); }
.mail-item-enter-active {
  opacity: 1; transform: scale(1);
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}
/* 新着メール通知 */
@keyframes newMailPulse {
  0%, 100% { box-shadow: 0 0 0 0 rgba(59, 130, 246, 0.4); }
  50%      { box-shadow: 0 0 0 8px rgba(59, 130, 246, 0); }
}
.new-mail-notification { animation: newMailPulse 2s infinite; }
```

> マイクロインタラクション（ボタンホバー、既読アニメ等）はライブラリ非依存で実装。Framer Motion 等を使う場合は採用時に別途検討。

---

## 8. アクセシビリティ対応

### キーボードナビゲーション
```
h: ホーム / i: 受信箱 / c: 新規作成 / r: 返信 / f: 転送 / d: 削除
/: 検索 / Escape: モーダル閉じる / Enter: 選択・実行 / ↑↓: メール選択
```

### スクリーンリーダー対応（セマンティック HTML 例）
```html
<main role="main" aria-label="メールアプリケーション">
  <nav role="navigation" aria-label="サイドバーメニュー">
    <button aria-label="受信箱 (12件の未読メール)">
      📥 <span class="sr-only">受信箱</span>
      <span aria-live="polite">12</span>
    </button>
  </nav>
  <section role="main" aria-label="メール一覧">
    <article aria-label="山田太郎からのメール: 会議の件について"></article>
  </section>
</main>
```

---

## 9. ダークモード対応

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
@media (prefers-color-scheme: dark) {
  :root:not([data-theme]) { /* ダークモード変数を適用 */ }
}
```

---

> 美しく実用的で、アクセシビリティに配慮したメールクライアントを目指す。
