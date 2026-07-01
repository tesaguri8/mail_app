# 迷惑メール対策（ローカル学習＋TSG One 共有インテリジェンス）

**ステータス:** 計画（実装未着手）
**方針:** **ローカル学習を主**に、**任意で TSG One に“シグナルだけ”を共有**して全ユーザーで強くする。**メール本文はサーバーに送らない**（プライバシー看板を厳守）。

関連: [MAIL_SECURITY.md](MAIL_SECURITY.md) / [FILTERING.md](FILTERING.md) / [AI_FEATURES.md](AI_FEATURES.md)（プライバシー方針）

---

## 1. 多層フィルタ

| 層 | 内容 |
|---|---|
| **プロバイダ側** | サーバーが付ける spam 判定（あれば）を尊重 |
| **ローカル学習（主）** | 端末内の統計分類（Bayesian 等）。ユーザーの「迷惑/非迷惑」マークで学習。**内容は端末から出ない** |
| **TSG One 共有インテリジェンス（任意）** | 全ユーザーの“シグナル”を集約した共有ブロックリスト/レピュテーションを購読 |
| **ヘッダ手掛り** | `List-Id`/`Precedence`/認証失敗（[MAIL_SECURITY.md](MAIL_SECURITY.md)）を素性に |

判定結果は迷惑フォルダへ隔離。**誤検知の復帰（非迷惑に戻す）**が学習にフィードバック。

---

## 2. TSG One 共有：何を送り、何を送らないか（最重要）

**送るのは“プライバシーに配慮した派生シグナルだけ”。本文・件名そのものは送らない。**

- **送る（任意・オプトイン）**: 迷惑URLのハッシュ、送信元/ドメインのレピュテーション、ヘッダ特徴、**本文のファジー指紋（SimHash 等）**、ユーザーの「迷惑」投票（匿名）。
- **送らない**: メール本文・件名の平文、宛先、個人情報。
- **集約→配布**: TSG One 側で集計し、**共有ブロックリスト/レピュテーション/指紋**として全クライアントへ配布。みんなの「迷惑」マークが全体を強くする。
- **既定はオプトイン**、匿名化、最小送信。共有を切ってもローカル学習は機能する。

> ねらい: 「中身は渡さず、迷惑の“形”だけ共有」。プライバシー看板と両立した集合知。

### 2.1 送信元レピュテーション共有の具体仕様

「スパム判定された送信元を集めて共有する」を、**生アドレスを送らず・誤検知を伝播させず**に実現する手順。

**① 何を送るか（粒度を上げてハッシュ化）**

生のメールアドレスは送らない（使い捨て・偽装が容易で、誤マーク時に第三者の個人情報が漏れる）。代わりに粒度を上げた派生値を送る。

| 送る値 | 形 | 効果/寿命 |
|---|---|---|
| 送信元ドメイン | `sha256(normalize(domain))` ＋ ソルト | 中〜強・中寿命 |
| 送信インフラ | 送信 IP の `/24` or ASN のハッシュ、SPF/DKIM/DMARC 結果 | 強い・長寿命 |
| 迷惑URL | 登録ドメイン部の `sha256`（パス・クエリは除去） | 強い・中寿命 |
| 本文ファジー指紋 | SimHash（64bit）。文面の使い回し検出 | 強い・中寿命 |
| 迷惑投票 | 上記キーに対する匿名の1票（ユーザーID・宛先は含めない） | — |

> 個別の From アドレスそのものは**送らない**。ドメイン／インフラ／指紋に集約する。

**② 配布の閾値（誤検知の伝播を防ぐ）**

1人の誤マークが全体に伝播しないよう、**独立した複数ユーザーの投票が集まったキーだけ**を配布対象にする。

- **k-匿名性**: 同一キーに対し独立ユーザー **k 件以上**（例: k≥5）の迷惑投票が集まって初めてレピュテーションに採用。
- **非迷惑投票で減衰**: `mail_mark_not_spam` 由来の票でスコアを下げる（誤検知の自己修復）。
- **減衰（time decay）**: 古い投票は重みを下げ、使い捨てドメインの賞味期限切れに追従。
- 配布物は「ブロック」ではなく **レピュテーションスコア**（0–1）。クライアント側でローカル学習と合算して最終判定（ハードブロックしない）。

**③ クライアント挙動**

- 受信レピュテーションは `spam_score` の一素性として加算（[§3.2](#32-言語非依存シグナル最優先全言語共通) の送信元レピュテーションに接続）。単独で隔離を確定させない。
- 共有をオフにしてもローカル学習だけで動作（オプトイン既定）。

> 結論: 「アドレスを集めて共有」は方向性として正しい。ただし **生アドレスではなくドメイン/インフラのハッシュ＋k件以上の匿名投票**にすることで、精度向上とプライバシー看板を両立する。

---

## 3. トークン化・多言語対応（日本語・英語必須）

**方針:** 言語に依存しないシグナルを土台にし、**まず文字 N-gram で全言語をカバー**、必要に応じて**日本語だけ形態素解析で格上げ**するハイブリッド。日英は必須対応、それ以外も「そこそこ」効く設計。

### 3.1 処理パイプライン

```
本文 → 正規化（Unicode NFKC・全角半角統一・小文字化・連続空白圧縮）
     → 言語ざっくり判定（文字種の割合: 漢字/かな vs ラテン）
        ├─ ラテン系（英語等） → 空白＋記号で単語分割
        └─ 日本語           → 段階1: 文字 2–3 gram ／ 段階2: 形態素解析
     → 言語非依存トークンを別名前空間で付与（後述）
     → spam_tokens(token, spam_count, ham_count) を更新／参照
```

### 3.2 言語非依存シグナル（最優先・全言語共通）

スパム判定で最も効く素性。本文の単語分割の精度に依存しない。

- **URL / ドメイン / IP**（`url:example.com` のように接頭辞付きトークン化）
- **ヘッダ特徴**: 認証失敗（SPF/DKIM/DMARC）、`List-Id` / `Precedence`（[MAIL_SECURITY.md](MAIL_SECURITY.md) 連携）
- **文字種の偏り**: 絵文字・全角記号の乱用、Unicode 混在偽装（キリル/ラテン混在など）
- **送信元レピュテーション**: ドメイン・送信元の評価（§2 の共有インテリジェンスと接続）

### 3.3 段階的な実装

| 段階 | 内容 | 依存 | 精度 |
|---|---|---|---|
| **段階1（まず作る）** | 文字 N-gram ＋ 言語非依存シグナル | 辞書バイナリ不要・軽量 | 日英とも「そこそこ」 |
| **段階2** | 日本語に形態素解析を追加（推奨: **Lindera**、代替: vibrato） | 辞書同梱（バイナリ増） | 日本語が大きく向上 |
| **段階3** | 利用の多い言語から順に専用トークナイザ追加 | 言語別辞書 | 言語ごとに最適化 |

> 段階1だけでも実用最小（MVP）として成立。[CLAUDE.md](../CLAUDE.md) の「軽くて美しい・端末内完結」方針に沿って、バイナリを重くせず立ち上げる。

> **UI 言語と判定対象言語は別**: アプリ UI（i18next）は当面 日英のみ。一方、迷惑判定は届くメールが何語でも動く。

## 4. データモデル

### 4.1 概要

- `emails.spam_score`（0–1）・`emails.is_junk`（隔離フラグ）
- ローカル学習: トークン統計テーブル `spam_tokens(token, spam_count, ham_count)`
  - トークンは名前空間付き（`w:無料` 本文語 / `ng:無料` N-gram / `url:example.com` / `hdr:spf_fail` / `from:example.com` など）で衝突を防ぐ
- 学習総数カウンタ: 学習済み spam/ham メール総数（`n_spam` / `n_ham`）は §7.2 のスコア計算に必須
- 共有購読（**後続フェーズ B**）: ブロックリスト/指紋/レピュテーションのローカルキャッシュ（定期更新）

### 4.2 DDL（フェーズA・SQLite/SQLCipher）

```sql
-- トークン別の spam/ham 出現メール数（同一メール内の重複は1カウント＝dedup後に加算）
CREATE TABLE IF NOT EXISTS spam_tokens (
    token       TEXT PRIMARY KEY,          -- 名前空間付き: "w:無料" / "ng:振込" / "url:example.com" ...
    spam_count  INTEGER NOT NULL DEFAULT 0,
    ham_count   INTEGER NOT NULL DEFAULT 0,
    updated_at  INTEGER NOT NULL DEFAULT 0 -- epoch 秒。古い語の刈り込み（vacuum）判断に使用
);

-- 学習メタ（総数カウンタ等）。1行 key-value で持ち、スキーマ追加に強くする
CREATE TABLE IF NOT EXISTS spam_meta (
    key   TEXT PRIMARY KEY,                -- "n_spam" / "n_ham" / "model_version" ...
    value INTEGER NOT NULL
);
-- 初期化: INSERT OR IGNORE で n_spam=0 / n_ham=0 を投入
```

- **emails 側**（既存メールテーブルに列追加）: `spam_score REAL`（0–1, NULL=未判定）、`is_junk INTEGER DEFAULT 0`（隔離フラグ）。
- **再マーク訂正**（§7.3）のため、各メールが「最後に学習した向き（spam/ham/未学習）」を保持する必要がある。`emails.spam_learned INTEGER`（-1=ham学習 / 0=未学習 / 1=spam学習）等で持ち、向きが変わったら旧カウントを打ち消してから付け替える。

### 4.3 カウンタ整合の指針

- `spam_tokens` の加算と `spam_meta.n_spam/n_ham` の増減は**同一トランザクション**で行い、ズレを防ぐ。
- トークン化時に同一メール内の重複トークンは **dedup してから1カウント**（同じ語の連呼でスコアが歪むのを防ぐ）。
- 肥大対策: `spam_count + ham_count` が極端に小さく `updated_at` が古いトークンは定期的に刈り込む（精度への影響は軽微）。

## 5. Tauri コマンド（抜粋）

| コマンド | 用途 |
|---|---|
| `mail_mark_spam` / `mail_mark_not_spam` | 迷惑/非迷惑のマーク（学習に反映） |
| `spam_score` | メールの判定スコア取得 |
| `spam_share_settings` | TSG One 共有のオプトイン/オフ・送信範囲設定（**後続フェーズ**） |
| `spam_intel_sync` | 共有インテリジェンスの取得・更新（**後続フェーズ**） |

## 6. 実装順序

**ローカル学習（端末内）を先行で完成させ、TSG One への送信・共有（§2 / §2.1）は最後に乗せる。** 共有を切ってもローカル学習だけで動く設計なので、依存関係としてもこの順序が正しい。

| フェーズ | 範囲 | 対応コマンド |
|---|---|---|
| **A（先に作る）** | トークン化（§3）＋ Bayesian 分類器＋学習フィードバック＋スコア出力。公開コーパス（SpamAssassin 等）で初期シード・動作検証 | `mail_mark_spam` / `mail_mark_not_spam` / `spam_score` |
| **B（後続）** | TSG One 共有（送信元レピュテーション・指紋・匿名投票）。送信・購読・k-匿名性閾値 | `spam_share_settings` / `spam_intel_sync` |

## 7. 段階1 実装スケッチ（フェーズA・端末内のみ）

> 計画段階のスケッチ。**下記は現行コードベース（[src-tauri/src/](../src-tauri/src/)）の規約に合わせた具体構成**。型・命名は ts-rs 境界と既存 `impl Store` パターンに合わせる。

### 7.0 モジュール構成（追加・変更するファイル）

現行の「コマンド＝`commands.rs` ／ 業務ロジック＝`services/` ／ DB＝`services/store/` の `impl Store` ／ 境界型＝`models.rs`（ts-rs）／ マイグレーション＝連番 SQL」に素直に乗せる。

| 追加・変更 | 役割 |
|---|---|
| `services/spam/mod.rs` | モジュール公開。[services/mod.rs](../src-tauri/src/services/mod.rs) に `pub mod spam;` |
| `services/spam/tokenize.rs` | §7.1 トークン化（純ロジック・DB 非依存） |
| `services/spam/classifier.rs` | §7.2 スコア計算（純関数・DB 非依存でテスト容易） |
| `services/store/spam.rs` | §7.4 `impl Store`：`spam_tokens`/`spam_meta` の読み書きと学習トランザクション。[store/mod.rs](../src-tauri/src/services/store/mod.rs) に `mod spam;` |
| `services/store/migrations/0011_spam.sql` | §7.6 スキーマ追加。[migrations.rs](../src-tauri/src/services/store/migrations.rs) の `MIGRATIONS` に version 11 を登録 |
| `models.rs` | §7.5 `SpamVerdict`（ts-rs 境界型） |
| `commands.rs` | §7.5 `mail_mark_spam` / `mail_mark_not_spam` / `spam_score` |
| `lib.rs` | §7.5 `invoke_handler!` に 3 コマンド追記 |

> **既存スキーマとの差分**: `emails.spam_score` / `emails.is_junk` は既に [0001_init.sql](../src-tauri/src/services/store/migrations/0001_init.sql) にある。0011 で足すのは `emails.spam_learned` 列と `spam_tokens` / `spam_meta` の 2 テーブルだけ（[DATABASE_SCHEMA.md](DATABASE_SCHEMA.md) の迷惑メール列と整合）。

> **入力データ（重要）**: `tokenize` は「同期時に保存済みの `emails` 行」を入力にする（`from_address` / `auth_result` / `list_id` / `clean_body`・`body_plain` / `raw_headers`）。ただし現行 [parser.rs](../src-tauri/src/services/parser.rs) は `auth_result` / `list_id` を**まだ抽出していない**。段階1の URL/from/N-gram 素性は既存データだけで動くが、ヘッダ素性（`hdr:spf_fail` 等）を効かせるには §7.7 のパーサ拡張が前提。

### 7.1 トークン化（言語非依存土台＋日本語 N-gram）

```rust
// services/spam/tokenize.rs（スケッチ）
fn tokenize(headers: &Headers, body: &str) -> Vec<String> {
    let mut toks = Vec::new();

    // (1) 言語非依存シグナル：最優先
    for d in extract_url_domains(body) { toks.push(format!("url:{d}")); }
    if headers.spf_failed()   { toks.push("hdr:spf_fail".into()); }
    if headers.dkim_failed()  { toks.push("hdr:dkim_fail".into()); }
    if headers.dmarc_failed() { toks.push("hdr:dmarc_fail".into()); }
    if let Some(d) = headers.from_domain() { toks.push(format!("from:{d}")); }
    if charset_anomaly(body) { toks.push("hdr:charset_mix".into()); } // Unicode混在偽装等

    // (2) 本文：正規化 → 言語ざっくり判定 → 分割
    let norm = normalize(body); // NFKC・全角半角統一・小文字化・空白圧縮
    if is_cjk_dominant(&norm) {
        for g in char_ngrams(&norm, 2, 3) { toks.push(format!("ng:{g}")); } // 段階1: 文字N-gram
        // 段階2でここを Lindera 形態素に差し替え/併用
    } else {
        for w in split_ascii_words(&norm) { toks.push(format!("w:{w}")); }
    }
    toks
}
```

### 7.2 Bayesian 判定（spam_tokens を使用）

```rust
// services/spam/classifier.rs（スケッチ）
// spam_tokens(token, spam_count, ham_count) と、総数 N_spam / N_ham を保持。
fn token_spamliness(t: &str, db: &Stats) -> f64 {
    let (s, h) = db.counts(t);                 // 当該トークンの spam/ham 出現数
    let ps = (s as f64 + 1.0) / (db.n_spam as f64 + 2.0); // ラプラス平滑化
    let ph = (h as f64 + 1.0) / (db.n_ham  as f64 + 2.0);
    (ps / (ps + ph)).clamp(0.01, 0.99)         // この語が spam である確率
}

// 各トークンの確率を対数オッズで合算（Paul Graham 方式の安定版）
fn spam_score(tokens: &[String], db: &Stats) -> f64 {
    let mut sum = 0.0_f64;
    for t in dedup_most_informative(tokens, 15) { // 偏りの強い語を上位採用
        let p = token_spamliness(t, db);
        sum += (p / (1.0 - p)).ln();
    }
    1.0 / (1.0 + (-sum).exp())                  // → 0..1 の spam_score
}
```

> **実装は上記スケッチから改良済み**（§7.8 の検証で誤検知が多発したため）: `token_spamliness` は
> ラプラス平滑化ではなく **クラス別の出現率＋ham 2 倍重み**を使い、`s + 2h < 5` の語は判定に使わない。
> 詳細と実測値は §7.8。

### 7.3 学習フィードバック

```rust
// mail_mark_spam / mail_mark_not_spam の実体
fn learn(tokens: &[String], is_spam: bool, db: &mut Stats) {
    for t in dedup(tokens) {
        if is_spam { db.inc_spam(t); } else { db.inc_ham(t); }
    }
    if is_spam { db.n_spam += 1; } else { db.n_ham += 1; }
    // 再マーク時は前回分を打ち消してから付け替える（誤マーク訂正）
}
```

### 7.4 ストア層（`impl Store` ／ `services/store/spam.rs`）

既存の [tags.rs](../src-tauri/src/services/store/tags.rs) と同じく `impl Store` に生やし、`self.conn.lock()` でアクセスする。カウンタ整合（§4.3）は**同一トランザクション**で担保する。

```rust
// services/store/spam.rs（スケッチ）
use std::collections::HashMap;
impl Store {
    /// 学習: dedup 済みトークンを一括加算し、総数(n_spam/n_ham)を同一 tx で更新（§4.3）。
    /// 再マーク時は emails.spam_learned を見て旧方向を打ち消してから付け替える（§7.3）。
    pub fn spam_learn(&self, email_id: i64, tokens: &[String], is_spam: bool) -> rusqlite::Result<()> { todo!() }
    /// スコア用: 対象トークンの (spam_count, ham_count) をまとめて引く。
    pub fn spam_token_counts(&self, tokens: &[String]) -> rusqlite::Result<HashMap<String, (i64, i64)>> { todo!() }
    /// 学習メール総数 (n_spam, n_ham)。§7.2 の平滑化に使う。
    pub fn spam_totals(&self) -> rusqlite::Result<(i64, i64)> { todo!() }
    /// 判定結果を保存（spam_score 列、隔離するなら is_junk も）。
    pub fn set_spam_score(&self, email_id: i64, score: f64, is_junk: bool) -> rusqlite::Result<()> { todo!() }
}
```

`classifier.rs`（§7.2）は `spam_token_counts` / `spam_totals` の戻り値だけを受け取る**純関数**にし、DB を触らない（ユニットテスト容易性を優先）。

### 7.5 コマンド・境界型・配線

コマンドは既存の一括操作規約（`mail_set_read(ids, ...)` / `mail_set_bookmarked(ids, ...)` 等）に合わせ `Vec<i64>` を受ける。しきい値（`τ_low`/`τ_high`）は §9 のユーザー設定（`tauri-plugin-store`）から読み、ハードコードしない。

```rust
// models.rs（ts-rs 境界型。f64/i32 は TS の number）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct SpamVerdict {
    pub score: f64,              // 0..1
    pub band: String,            // "clean" | "uncertain" | "junk"（§8.1）
    pub top_tokens: Vec<String>, // 効いた素性（§8.4 の根拠表示に使う）
}

// commands.rs（既存パターン: State<Store> を取り Result<_, String> を返す）
#[tauri::command]
pub fn mail_mark_spam(store: State<Store>, ids: Vec<i64>) -> Result<(), String> { /* spam_learn(.., true) */ Ok(()) }
#[tauri::command]
pub fn mail_mark_not_spam(store: State<Store>, ids: Vec<i64>) -> Result<(), String> { /* spam_learn(.., false) */ Ok(()) }
#[tauri::command]
pub fn spam_score(store: State<Store>, id: i64) -> Result<SpamVerdict, String> { todo!() }
```

[lib.rs](../src-tauri/src/lib.rs) の `invoke_handler![...]` に `commands::mail_mark_spam` / `mail_mark_not_spam` / `spam_score` を追記。境界型は `npm run gen:bindings` で `src/bindings/SpamVerdict.ts` に出力される。

### 7.6 マイグレーション `0011_spam.sql`

実列名に整合させる（`emails.spam_score` / `is_junk` は 0001 で既存のため触らない）。

```sql
-- 迷惑メールのローカル学習（docs/SPAM.md §4）。
ALTER TABLE emails ADD COLUMN spam_learned INTEGER DEFAULT 0; -- -1=ham学習 / 0=未学習 / 1=spam学習

CREATE TABLE spam_tokens (
    token      TEXT PRIMARY KEY,   -- 名前空間付き: "w:無料" / "ng:振込" / "url:example.com" / "hdr:spf_fail" ...
    spam_count INTEGER DEFAULT 0,
    ham_count  INTEGER DEFAULT 0,
    updated_at INTEGER DEFAULT 0   -- epoch 秒。刈り込み判断（§4.3）
);

CREATE TABLE spam_meta (
    key   TEXT PRIMARY KEY,        -- "n_spam" / "n_ham" / "model_version"
    value INTEGER NOT NULL
);
INSERT OR IGNORE INTO spam_meta(key, value) VALUES ('n_spam', 0), ('n_ham', 0);

CREATE INDEX idx_emails_junk ON emails(is_junk) WHERE is_junk = 1; -- 迷惑フォルダ一覧
```

[migrations.rs](../src-tauri/src/services/store/migrations.rs) の `MIGRATIONS` に `Migration { version: 11, sql: include_str!("migrations/0011_spam.sql") }` を追加し、同ファイルの `migrations_apply_and_fts_works` テストの想定バージョンを 11 に更新する。

### 7.7 パーサ拡張の前提（ヘッダ素性）

§3.2 の言語非依存シグナルの主力＝認証結果とメール種別。現行 [parser.rs](../src-tauri/src/services/parser.rs) の `ParsedEmail` はこれらを未抽出なので、以下を足す（[MAIL_SECURITY.md](MAIL_SECURITY.md) の認証バッジと素性を共有できる）。

- `Authentication-Results` から SPF / DKIM / DMARC の pass/fail を抽出 → `emails.auth_result` に保存 → トークン `hdr:spf_fail` 等へ。
- `List-Id` / `Precedence` を抽出 → `emails.list_id` 等へ → メルマガ/一斉配信の素性に。

> 段階1はこの拡張なしでも（URL/from/N-gram で）動く。ただし誤検知抑制に効くヘッダ素性なので早期対応が望ましい。

### 7.8 検証（公開コーパス）

- **SpamAssassin Public Corpus**（生 `.eml`）で `mail-parser` → `tokenize` → 学習 → ホールドアウトで `spam_score` を評価。
- 指標: 精度（accuracy）よりも **誤検知率（ham を spam と誤る率）を最重視**（正当なメールの隔離は実害が大きい）。閾値は誤検知を抑える側に倒す。
- 実装: [src-tauri/tests/spam_corpus.rs](../src-tauri/tests/spam_corpus.rs)（`#[ignore]`。コーパスがある時だけ実行）。
  ```
  SPAM_CORPUS_DIR=<spam/ と ham/ を含むフォルダ> \
    cargo test --test spam_corpus -- --ignored --nocapture
  ```

**検証で判明した重要事項（分類器を修正済み）**: 初期実装（ラプラス平滑化 `(s+1)/(N_spam+2)`）は、
クラス不均衡（例 spam 400 / ham 2000）下で**未知語が spam 寄りに評価**され、正当メールを大量に
誤隔離した（FPR ≈ 20%）。対策として §7.2 の分類器を次のとおり修正:
- **クラス別の出現率**で比較（不均衡に非依存）。
- **ham 側を 2 倍重み**（Paul Graham 方式。誤検知＜見逃しの原則で ham 寄りに倒す）。
- **学習の乏しい語（`s + 2h < 5`）は判定に使わない**（未知語の偏りを除去）。

修正後の SpamAssassin(easy_ham 2500 / spam 500, 80/20 分割)実測: **FPR 0/500 = 0.0**、
spam 検出率 76–81%（τ_high=0.9 / τ_low=0.5）。誤検知ゼロを優先し、検出率は学習の蓄積と
段階2（形態素・ヘッダ素性）で伸ばす方針。

## 8. 閾値・隔離運用

**原則: 誤検知（正当メールの隔離）は実害が大きい。スコアは消さずに分け、ユーザーがいつでも復帰できる。完全削除は自動でしない。**

### 8.1 二段しきい値（バンド分け）

単一しきい値で白黒つけず、3バンドに分ける。

| バンド | spam_score | 扱い |
|---|---|---|
| **clean** | `< τ_low`（例 0.5） | 受信トレイ。通常表示 |
| **uncertain** | `τ_low 〜 τ_high` | 受信トレイに残すが「迷惑かも」マーク（控えめ）。隔離はしない |
| **junk** | `≥ τ_high`（例 0.9） | 迷惑フォルダへ隔離（`is_junk=1`）。削除はしない |

- 既定は **`τ_high` を高め**（誤検知優先で安全側）。設定で調整可（`spam_share_settings` とは別のローカル設定）。
- **ホワイトリスト最優先**: 連絡先・過去にやり取りのある送信元・ユーザーが「非迷惑」にした送信元は、スコアに関わらず clean に固定（[FILTERING.md](FILTERING.md) の「知り合い/取引実績」と連携）。

> **実装状況（重要な順序）**: 受信時の自動採点・隔離は [services/spam/apply.rs](../src-tauri/src/services/spam/apply.rs) に実装済み（`score_incoming`）。ただし **ホワイトリスト（住所録連携）を先に実装するため、まだ同期（imap_sync）や UI には配線していない**。知り合いのメールを誤って隔離しないよう、`apply.rs` を発火させる前に「連絡先・返信実績・手動 not-spam は score に関わらず clean 固定」を通す。手動の「迷惑メールに設定」（学習＋隔離）は先行して有効。

### 8.2 隔離のライフサイクル（自動削除しない）

```
junk 判定 → 迷惑フォルダへ隔離（is_junk=1, サーバー側は既読化しない）
          → ユーザーが「非迷惑に戻す」= mail_mark_not_spam → 受信トレイ復帰＋ham学習
          → 一定期間（例 30 日）経過した迷惑メールは「自動削除候補」として提示（削除は確認の上）
```

- **サーバー側操作は最小限**: 隔離はローカルのフラグ／ローカルフォルダ移動を基本とし、IMAP の Junk フォルダへ動かすかは設定で選択（プロバイダ仕様差を吸収）。
- **完全自動削除はしない**。古い迷惑メールの削除は提案のみ（取りこぼし防止の看板と矛盾させない）。

### 8.3 ヒステリシス（バタつき防止）

- 学習で `spam_tokens` が更新されても、**既に表示済みのメールのバンドを頻繁に上下させない**（再分類は新着・明示再スキャン時に限定）。
- ユーザーが手で動かした分類（spam/not_spam）は**自動判定より常に優先**し、上書きしない。

### 8.4 誤検知時のリカバリ UX

- 迷惑フォルダには「なぜ迷惑と判定したか」の根拠（効いたトークン上位、認証失敗、レピュテーション）を控えめに表示し、ワンタップで復帰。
- 復帰操作は §7.3 の学習へ即フィードバック（次回から同種を clean 寄りに）。

## 9. ユーザー設定（ハードコードしない）

**§8 の数値・挙動はすべて既定値であり、ユーザーが設定パネルで変更できる。** [CLAUDE.md](../CLAUDE.md) の「ハードコード排除」「ユーザー主権（勝手に決めない）」方針に従い、しきい値・削除可否・ホワイトリスト優先などをコードに固定しない。

### 9.1 設定項目（既定値）

| 設定キー | 内容 | 既定値 | UI |
|---|---|---|---|
| `spam.enabled` | 迷惑判定の有効/無効 | `true` | トグル |
| `spam.threshold_low` | uncertain 帯の下限 `τ_low` | `0.5` | スライダー |
| `spam.threshold_high` | junk 隔離の `τ_high` | `0.9` | スライダー |
| `spam.show_uncertain_badge` | uncertain に「迷惑かも」表示するか | `true` | トグル |
| `spam.junk_action` | junk の扱い | `move_local`（ローカル隔離）／`move_imap_junk`／`flag_only` | 選択 |
| `spam.auto_delete` | 古い迷惑メールの扱い | `suggest`（提案のみ。他に `never` / `delete_after`） | 選択 |
| `spam.auto_delete_days` | `delete_after` 時の日数 | `30` | 数値（`auto_delete=delete_after` の時のみ有効） |
| `spam.whitelist_priority` | ホワイトリストを判定より優先するか | `true` | トグル |
| `spam.whitelist_sources` | clean 固定の対象（複数選択） | `[contacts, has_replied, manual_not_spam]`（連絡先／返信実績／手動指定） | チェック群 |
| `spam.rescan_on_learn` | 学習時に表示済みを再分類するか | `false`（ヒステリシス） | トグル |

> **削除は既定で「提案のみ（`suggest`）」**。`delete_after` を選んだ場合でも自動削除前に確認するか（`confirm`）を別途持たせ、無確認の完全自動削除は既定では選べないようにする（取りこぼし防止の看板）。

### 9.2 保存先・配り方

- 設定は **DB の `app_settings`（key-value）** に保存（機密ではないため keyring 不要）。Rust 側 `spam_score`/隔離ロジックが設定を読んで判定を切り替えるため、既存の `sync_window`/`storage_limit` と同じく **DB を単一ソース**にする（`tauri-plugin-store` は UI 専用の表示設定に限定）。既定値は spam モジュールの定数を参照し、固定値を埋めない。
- ラベル・説明文は **i18next**（`settings` 名前空間、日英）で管理。設定キー自体はハードコードせず定数の単一ソースから参照。
- 既定値も「マジックナンバー直書き」を避け、設定スキーマ定義（既定値つき）を単一ソース化しておき、UI とロジックが同じ定義を見る。

### 9.3 設定リセット

- 「迷惑判定設定を既定に戻す」操作を用意（スキーマの既定値へ）。学習データ（`spam_tokens`）のリセットは**別操作**として分離（設定リセットで学習を消さない）。
