# オンボーディング / アカウント自動設定

**ステータス:** 計画（実装未着手）
**目的:** 最初の体験を滑らかに。**メールアドレスだけで IMAP/SMTP を自動判定**し、迷わず使い始められる。

関連: [POSITIONING.md](POSITIONING.md)（認証方針）/ [DATA_STORAGE.md](DATA_STORAGE.md)

---

## 1. 初回起動フロー

1. ウェルカム（美しいホームのプレビュー）→ 言語選択
2. **アカウント追加**（メールアドレス入力 → 自動設定）
3. パスワード/アプリパスワード入力 → 接続テスト → 完了
4. 同期ウィンドウ（[SYNC.md](SYNC.md)）と背景画像を初期選択 → ホームへ

> 既存メールがある人には[移行/インポート](IMPORT_EXPORT.md)を案内。

---

## 2. プロバイダ自動設定（autoconfig）

メールアドレスのドメインから接続設定を自動判定する。優先順:

1. **Mozilla ISPDB / autoconfig**（`autoconfig.<domain>` / `autoconfig.thunderbird.net`）
2. **Microsoft Autodiscover**（Exchange/Outlook 系）
3. **主要プロバイダの内蔵テーブル**（Gmail/Outlook/iCloud/主要日本プロバイダ等）
4. **MX/SRV からの推定**（`_imaps._tcp` SRV、MX ホスト名から推測）
5. **手動入力**（上記で決まらない場合。ホスト/ポート/暗号化方式）

- 認証は基本 **手動設定（OAuth 不要）**（[POSITIONING.md](POSITIONING.md) §5）。
- **Gmail/Outlook はアプリパスワードが必要な場合**があるため、**取得手順への分かりやすい誘導**（2FA→アプリパスワード）を用意。失敗時のエラーは原因と次の一手を明示。

---

## 3. データモデル / コマンド

- 設定自動判定の結果は `accounts`（imap/smtp host・port・暗号化）に保存（[DATABASE_SCHEMA.md](DATABASE_SCHEMA.md)）。資格情報は keyring。

| コマンド | 用途 |
|---|---|
| `account_autoconfig` | アドレスから接続設定を推定 |
| `account_test_connection` | 接続/認証テスト |
| `account_add` | アカウント登録（[FEATURE_SPEC.md](FEATURE_SPEC.md)） |
