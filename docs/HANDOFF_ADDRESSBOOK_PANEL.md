# 引き継ぎ: 住所録の「＋自動入力」＆「メールを見ながら編集する右パネル」

最終更新: 2026-07-07 / 対象コミット: `f0daa1c`（ブランチ `wt-1`、未 push）

## 1. 目的・背景

メール本文/ヘッダのアドレス横にある **＋（住所録に追加）／✎（編集）** の体験を 2 点改善する。

1. **＋追加時の自動入力**: 差出人情報から予測できる範囲（名前・姓・名・メール）を自動で埋める。
2. **メールを見ながら編集**: 住所録ページへ画面遷移せず、メール一覧サイドバーを隠して、閲覧ペインの右にコンパクトな編集パネルを出す。メール本文など他の情報を見ながら連絡先を入力・編集できるようにする。

## 2. 現状（完了・動作するもの）

- ✅ ＋を押すと **名前・姓・名・メール**を埋めた新規フォームが右パネルで開く。
- ✅ 差出人名の姓/名 推定分割（`splitPersonName`）。
  - `末松 慎吾` / `末松　慎吾`（全角空白）→ 姓=末松・名=慎吾
  - `John Smith` → 名=John・姓=Smith、`Smith, John` → 姓=Smith・名=John
  - 空白なしの日本語名（`末松慎吾`）や組織名（株式会社・Inc・サポート等）は分割せず**表示名のみ**（誤分割を避ける安全側）。
- ✅ アドレスの ＋/✎ を押すと、**左サイドバー（メール一覧）が自動で隠れ**、閲覧ペインの右に **編集パネル**が出る。× で閉じると元のサイドバー表示に戻る。パネル幅はドラッグ可変（`localStorage: rondine.contactPanelW`）。
- ✅ 連絡先フォームを `ContactEditor` として切り出し、**住所録ページ**と**メール画面の右パネル**の両方で共有（保存・削除・重複検知はパネル内で完結）。
- ✅ `npm run typecheck` / `npm run lint` / `npm test`（`splitPersonName` の 7 ケース）すべてパス。

## 3. 実装の詳細

### 追加・変更ファイル（コミット `f0daa1c`、8 files）

| ファイル | 種別 | 内容 |
|---|---|---|
| `src/renderer/utils/name.ts` | 新規 | `splitPersonName(display)` — 表示名を姓/名に推定分割。全角空白はソース上 `　` エスケープ（`no-irregular-whitespace` 回避）。 |
| `src/renderer/utils/name.test.ts` | 新規 | Vitest ユニットテスト（日本語/欧文/カンマ/空白なし/組織/引用符/空）。 |
| `src/renderer/components/ContactEditor.tsx` | 新規 | 連絡先編集フォーム本体。`ContactsView` から draft/保存/削除/重複検知ロジックと JSX を移設。`draftFromPrefill` が `splitPersonName` を使って ＋追加の初期値を作る。 |
| `src/renderer/components/ContactsView.tsx` | 変更 | 一覧・検索・タグ絞り込み・取込・ゴミ箱・重複整理は残し、右ペインは `<ContactEditor>` を呼ぶだけに。`ContactPrefill` は `ContactEditor` から再輸出。 |
| `src/renderer/components/MailboxView.tsx` | 変更 | `contactPanel` 状態＋開閉/リサイズ/サイドバー自動退避を追加。会話ハンドラの `onAddContact`/`onEditContact` が住所録遷移ではなく右パネルを開くように変更。 |
| `src/renderer/App.tsx` | 変更 | 旧「メール→住所録ページへ遷移して prefill」の配線（`contactPrefill`/`contactOpenId`/`addContactFromMail`/`openContactFromMail`）を撤去。 |
| `src/renderer/locales/{ja,en}/common.json` | 変更 | `contact.panelNew` / `contact.panelEdit` / `contact.closePanel` を追加。 |

### 設計判断と理由

- **`ContactEditor` への切り出し**: 住所録ページの右ペインをそのまま右パネルに置くと一覧まで付いてきて幅を食う。編集フォームだけを共有部品にし、両画面で再利用。
- **`EditorRequest`（`{kind:'new'|'prefill'|'existing'}`）で駆動**: 親（`ContactsView` / `MailboxView`）が「何を開くか」を state として渡し、`ContactEditor` が参照変化時に下書きを作り直す。参照が変わらない限り再初期化しない（保存後に下書きが吹き飛ばない）。
- **右パネルはトップレベルの分岐**: `MailboxView` の描画は `compose ? … : contactPanel ? （閲覧+編集の 2 画面）: layout==='side' ? … : …`。作成画面に入ったらパネルは自動で閉じる。
- **サイドバー自動退避**: パネルを開く直前の `sidebarOpen` を ref に控え、閉じるとき復元。
- **キーボード衝突なし**: 既存の Del/Ctrl+Z/矢印ハンドラは入力欄（INPUT/TEXTAREA/contentEditable）フォーカス時に既に no-op なので、フォーム入力中に誤発火しない。

## 4. 残タスク・改善余地

- [ ] 実機（`npm run tauri dev`）での見た目・操作の最終確認（下記 §5）。
- [ ] ＋で追加した直後、同じ本文中の ＋アイコンが ✎（登録済み）に**即座には切り替わらない**。`MailBody.tsx` の `emailLookupCache`（モジュール内・メール離脱時のみクリア）が原因で、マウント済み `EmailAdd` は再照会しない。改善するなら保存時にキャッシュを無効化＋再照会するシグナルが必要（現状はメールを開き直せば反映）。
- [ ] 空白なしの日本語名の姓/名 推定（辞書なしでは誤りが多いため現状は非対応。要否は要検討）。
- [ ] `package-lock.json` のバージョン同期差分（`0.1.0`→`0.1.0-alpha.1`）が未コミットで残置。今回の作業とは無関係なので別途扱う。

## 5. 動作確認の方法

```bash
# 型・lint・テスト
npm run typecheck
npm run lint
npm test            # splitPersonName の 7 ケース

# 実機
npm run tauri dev
```

実機での手順:
1. 受信箱で任意のメールを開く。
2. ヘッダの差出人（または本文中）のアドレスにマウスを乗せ、**＋** を押す。
3. 右にパネルが開き、左のメール一覧サイドバーが消えることを確認。名前・姓・名・メールが埋まっているか確認（`末松 慎吾` のような空白入り差出人だと姓/名が入る）。
4. 保存 → × で閉じ、サイドバーが元に戻ることを確認。
5. 登録済みアドレスの ✎ で既存連絡先が同じパネルで開くことを確認。
6. パネル右端をドラッグして幅が変わることを確認。

## 6. 注意点・既知の問題

- 全角空白を **正規表現リテラルに直書きしない**（eslint `no-irregular-whitespace`）。`name.ts` は `　` エスケープ済み。文字列リテラル内（テスト）は `skipStrings` により許容。
- `AddressBook`（住所録ページ）は `prefill`/`openId` プロップを今も受け取るが、`App` からは渡さない（＝メールからの prefill は右パネル経由に一本化）。将来もし住所録ページ側に prefill を戻すならこの配線を復活させる。
- この作業は**迷惑メール復活バグとは無関係**（別調査。`docs/SPAM.md` と後述の別診断を参照）。
