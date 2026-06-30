# AI 活用機能

**ステータス:** 計画（実装未着手）
**位置づけ:** メール作成・整理を AI で支援する。Primadoc の **マルチモデル AI（Claude / GPT / Gemini）＋ ローカル Ollama** 基盤を流用する。
**プライバシー方針（確定）:** **AI はオプトイン**。既定はクラウド（Claude）、**機密データはローカル（Ollama）を選択可**。ゲスト個人情報など業務データの扱いに配慮する。

関連: [FEATURE_SPEC.md](FEATURE_SPEC.md) / [THREADING.md](THREADING.md) / [DATABASE_SCHEMA.md](DATABASE_SCHEMA.md)

---

## 1. 機能一覧

| 機能 | 内容 | 既定モデルの目安 |
|---|---|---|
| **件名の自動生成** | 本文から件名案を生成（特に「新規メール」で件名が空のとき） | Haiku 4.5（軽量・高速・低コスト） |
| **本文ドラフト/リライト** | 下書き生成、トーン調整、敬体/常体、丁寧さ、要点整形 | Sonnet 4.6 / Opus 4.8 |
| **スレッド要約** | 長い会話・引用を要約して把握を速く | Sonnet 4.6 |
| **返信候補の提案** | 文脈から数パターンの返信案。ユーザーが選んで編集 | Sonnet 4.6 |
| **自動分類・タグ付け** | 内容から分類・タグ提案（仕分け補助） | Haiku 4.5 |

> いずれも **生成物は必ずユーザーが確認・編集してから送信**（ハルシネーション対策）。AI は提案、決定は人。

---

## 2. 「このアドレスへ新規メール」との連携

作成画面は **2 モード**（詳細は [FEATURE_SPEC.md](FEATURE_SPEC.md) §2.1 / 送信）:

- **返信** … スレッドを引き継ぐ
- **このアドレスへ新規メール** … 新しい `Message-ID`・参照ヘッダなしの**別案件**（＝新しい論理スレッド）

「新規メール」では件名が空になりがちなので、**本文から AI が件名案を生成**する導線を置く。これにより「返信で別件を送る」癖を、迷惑のかからない正しい新規送信へ誘導する（[THREADING.md](THREADING.md) の課題を発生源で抑制）。

---

## 3. アーキテクチャ（Primadoc 流用）

```
レンダラー（作成・閲覧UI）
   │ invoke
   ▼
src-tauri/src/services/ai/        … プロバイダ抽象（cloud / local）
   ├─ cloud: api.anthropic.com / api.openai.com / generativelanguage（HTTPS, ストリーミング）
   └─ local: Ollama（services/ollama）… 機密データ向け
```

- プロバイダ・モデルは**設定で切替**。用途ごとに既定モデルを持つ（§1）。
- ストリーミング出力で体感速度を確保。
- CSP・接続先は Primadoc の構成（`api.anthropic.com` 等）に準じる。

---

## 4. データの扱い（JSON は不要か？ → コア保存には不要）

**SQLite で構造化しているため、メール本体の保存に JSON は不要。** リレーショナル＋FTS5 を「索引付きの真実の源」として維持する。JSON が残るのは**保存形式ではなく限定的な役割**だけ:

| 役割 | JSON を使うか | 補足 |
|---|---|---|
| メール本体の保存 | **使わない** | SQLite のカラム＋FTS5。日付/差出人/スレッド検索・部分更新・JOIN のため |
| AI / IPC へ渡す表現 | **使う（自動）** | serde / ts-rs が**ただで**JSON 化。これは「保存」ではなくシリアライズ |
| AI 注釈の保存 | **限定的に使う** | 要約・分類・提案など可変な生成物は `ai_annotations` に格納（下記） |
| エクスポート/バックアップ | **使う** | JSONL は可搬・差分管理・再取込に有利。保存とは別概念 |

> 真に可変なデータ（任意の `X-*` ヘッダ群など）に限り JSON 列を“逃がし弁”として使う余地はあるが、**基本はテーブルで正規化**する。リレーショナルの核を JSON で置き換えない。

### AI 注釈テーブル（例）

AI 生成物は機能追加でスキーマが変わりやすいため、可変部分のみ JSON で保持する小さなテーブルに分離する。

```sql
CREATE TABLE ai_annotations (
    id INTEGER PRIMARY KEY,
    target_type TEXT NOT NULL,   -- 'email' | 'thread'
    target_id INTEGER NOT NULL,
    kind TEXT NOT NULL,          -- 'summary' | 'subject_suggest' | 'reply_suggest' | 'category'
    content_json TEXT,           -- 生成物（可変構造のため JSON）
    model TEXT,                  -- 使用モデル（監査・再現用）
    is_local BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_ai_annotations_target ON ai_annotations(target_type, target_id, kind);
```

---

## 5. Tauri コマンド（抜粋）

| コマンド | 用途 |
|---|---|
| `ai_generate_subject` | 本文から件名案 |
| `ai_draft_body` | 本文ドラフト/リライト（指示・トーン指定） |
| `ai_summarize_thread` | 論理スレッドの要約 |
| `ai_suggest_reply` | 返信候補の提案 |
| `ai_classify` | 分類・タグ提案 |
| `ai_settings_get` / `ai_settings_set` | プロバイダ・モデル・オプトイン設定 |

---

## 6. リスクと留意点

| 項目 | 内容 | 対応 |
|---|---|---|
| プライバシー | 業務・ゲスト個人情報をクラウドへ送る懸念 | **オプトイン**、機密は**ローカル(Ollama)**、送信前に対象を明示 |
| ハルシネーション | 誤った件名・本文 | **人が確認・編集してから送信**。AI は提案に留める |
| コスト/レート | クラウド API の従量・制限 | 用途別に軽量モデル（Haiku）を既定化、キャッシュ |
| 再現性・監査 | どのモデルで生成したか | `ai_annotations.model` に記録 |
