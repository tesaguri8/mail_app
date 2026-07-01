//! Bayesian 判定（docs/SPAM.md §7.2）。
//! spam_tokens の出現数と学習総数から spam 確率を出す純関数群（DB 非依存）。

use std::collections::{HashMap, HashSet};

/// スコア合算に採用する「偏りの強い語」の上限（Paul Graham 方式）。
const MOST_INFORMATIVE: usize = 15;

/// この回数（spam+ham の学習出現数）に満たない語は判定に使わない。
/// 学習の少ない語は推定が不安定で、特にクラス不均衡下では未知語が
/// 一方のクラスへ偏る（例: spam<ham だと未知語が spam 寄りに出る）。
/// 公開コーパス検証（§7.8）で誤検知の主因と判明したためのガード。
const MIN_TOKEN_COUNT: i64 = 5;

/// 単語 1 語の spam らしさ（0.01..0.99 にクランプ）。
/// クラス別の「出現率」で比較し、クラス不均衡（ham≫spam 等）に依存しないようにする。
/// さらに ham 側を 2 倍に重み付けする（Paul Graham 方式）。誤検知（正当メールの
/// 隔離）は実害が大きいため、ham 寄りに倒して false positive を抑える（§7.4 / §7.8）。
pub fn token_spamliness(spam: i64, ham: i64, n_spam: i64, n_ham: i64) -> f64 {
    let b = (spam as f64 / n_spam.max(1) as f64).min(1.0); // spam 出現率
    let g = (2.0 * ham as f64 / n_ham.max(1) as f64).min(1.0); // ham 出現率（誤検知回避で2倍重み）
    if b + g == 0.0 {
        return 0.5; // 学習の手掛かりなし＝中立
    }
    (b / (b + g)).clamp(0.01, 0.99)
}

/// トークン群から spam_score(0..1) と「効いた素性（spam 寄りの語）上位」を返す。
/// `counts` は当該トークンの (spam_count, ham_count)。未知語は (0,0) 扱い。
/// 学習がゼロのうちは中立（0.0）を返す。
pub fn score(
    counts: &HashMap<String, (i64, i64)>,
    tokens: &[String],
    n_spam: i64,
    n_ham: i64,
) -> (f64, Vec<String>) {
    if n_spam == 0 && n_ham == 0 {
        return (0.0, Vec::new());
    }

    // 同一メール内の重複語は 1 回だけ数える（連呼でスコアが歪むのを防ぐ。§4.3）。
    let mut seen = HashSet::new();
    let mut scored: Vec<(String, f64)> = Vec::new();
    for t in tokens {
        if !seen.insert(t.as_str()) {
            continue;
        }
        let (s, h) = counts.get(t).copied().unwrap_or((0, 0));
        // 学習の乏しい語（未知語含む）は判定に使わない（§7.8 の誤検知対策）。
        // ham は 2 倍重みに合わせて閾値判定する（token_spamliness と同じ扱い）。
        if s + 2 * h < MIN_TOKEN_COUNT {
            continue;
        }
        let p = token_spamliness(s, h, n_spam, n_ham);
        let logodds = (p / (1.0 - p)).ln();
        scored.push((t.clone(), logodds));
    }

    // 偏りの強い（|logodds| が大きい）語を上位採用。
    scored.sort_by(|a, b| {
        b.1.abs()
            .partial_cmp(&a.1.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(MOST_INFORMATIVE);

    let sum: f64 = scored.iter().map(|(_, lo)| lo).sum();
    let score = 1.0 / (1.0 + (-sum).exp());

    // 根拠表示（§8.4）用に spam 寄りに効いた語だけ返す。
    let top: Vec<String> = scored
        .into_iter()
        .filter(|(_, lo)| *lo > 0.0)
        .map(|(t, _)| t)
        .collect();
    (score, top)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_when_untrained() {
        let (s, top) = score(&HashMap::new(), &["w:hello".into()], 0, 0);
        assert_eq!(s, 0.0);
        assert!(top.is_empty());
    }

    #[test]
    fn spammy_tokens_push_score_up() {
        let mut counts = HashMap::new();
        counts.insert("w:viagra".to_string(), (20i64, 0i64)); // spam のみに出る
        counts.insert("w:hello".to_string(), (1i64, 20i64)); // ham 寄り
        let (spammy, top) = score(&counts, &["w:viagra".into()], 30, 30);
        let (hammy, _) = score(&counts, &["w:hello".into()], 30, 30);
        assert!(spammy > 0.5, "spam 語でスコアが上がる: {spammy}");
        assert!(hammy < 0.5, "ham 語でスコアが下がる: {hammy}");
        assert!(top.iter().any(|t| t == "w:viagra"));
    }
}
