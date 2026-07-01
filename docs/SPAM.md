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

> 計画段階のスケッチ。実装時に [src-tauri/src/services/](../src-tauri/src/services/) 配下へ。型・命名は ts-rs 境界に合わせて調整。

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

### 7.4 検証（公開コーパス）

- **SpamAssassin Public Corpus**（生 `.eml`）で `mail-parser` → `tokenize` → 学習 → ホールドアウトで `spam_score` を評価。
- 指標: 精度（accuracy）よりも **誤検知率（ham を spam と誤る率）を最重視**（正当なメールの隔離は実害が大きい）。閾値は誤検知を抑える側に倒す。

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

- 設定は **`tauri-plugin-store`** に保存（機密ではないため keyring 不要）。Rust 側 `spam_score`/隔離ロジックは起動時とイベントで設定を読み、固定値を埋めない。
- ラベル・説明文は **i18next**（`settings` 名前空間、日英）で管理。設定キー自体はハードコードせず定数の単一ソースから参照。
- 既定値も「マジックナンバー直書き」を避け、設定スキーマ定義（既定値つき）を単一ソース化しておき、UI とロジックが同じ定義を見る。

### 9.3 設定リセット

- 「迷惑判定設定を既定に戻す」操作を用意（スキーマの既定値へ）。学習データ（`spam_tokens`）のリセットは**別操作**として分離（設定リセットで学習を消さない）。
