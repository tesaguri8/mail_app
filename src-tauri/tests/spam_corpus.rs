//! §7.8 検証: 公開コーパス（SpamAssassin 等）で誤検知率を評価する。
//!
//! 既定は #[ignore]（コーパス無しの通常 `cargo test` では走らせない）。
//! 実行例:
//!   SPAM_CORPUS_DIR=/path/to/corpus \
//!     cargo test --test spam_corpus -- --ignored --nocapture
//! ディレクトリ構成: `$SPAM_CORPUS_DIR/spam/*` と `$SPAM_CORPUS_DIR/ham/*`
//! （拡張子は問わない。SpamAssassin の生ファイルをそのまま置く）。
//!
//! 指標: 精度よりも **誤検知率（ham を spam と誤る率）を最重視**（正当メールの
//! 隔離は実害が大きい。docs/SPAM.md §7.4 / §8）。安全側しきい値での FPR を assert。

use rondine_lib::services::{parser, spam};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

/// 生メールをパースしてトークン列にする（本番と同じ tokenize を通す）。
fn tokens_for(raw: &[u8]) -> Option<Vec<String>> {
    let p = parser::parse_message(raw)?;
    let body = p.clean_body.or(p.body_plain).unwrap_or_default();
    Some(spam::tokenize(
        p.from_address.as_deref(),
        p.subject.as_deref(),
        &body,
        p.auth_result.as_deref(),
        p.list_id.as_deref(),
    ))
}

/// ディレクトリ内の全ファイルをトークン化して返す（ファイル名順で決定的に）。
fn load_dir(dir: &Path) -> Vec<Vec<String>> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|p| fs::read(p).ok().and_then(|b| tokens_for(&b)))
        .collect()
}

/// 決定的な 80/20 分割: index % 5 == 0 をホールドアウト、それ以外を学習に使う。
fn is_holdout(i: usize) -> bool {
    i % 5 == 0
}

/// train 分のトークンから spam/ham 出現数を数え上げる（同一メール内は dedup）。
fn learn_into(
    counts: &mut HashMap<String, (i64, i64)>,
    docs: &[Vec<String>],
    is_spam: bool,
) -> i64 {
    let mut n = 0;
    for (i, toks) in docs.iter().enumerate() {
        if is_holdout(i) {
            continue;
        }
        n += 1;
        let mut seen = HashSet::new();
        for t in toks {
            if !seen.insert(t.as_str()) {
                continue;
            }
            let e = counts.entry(t.clone()).or_insert((0, 0));
            if is_spam {
                e.0 += 1;
            } else {
                e.1 += 1;
            }
        }
    }
    n
}

/// holdout 分について、しきい値以上を「spam 判定」とした件数と総数を返す。
fn eval_flagged(
    counts: &HashMap<String, (i64, i64)>,
    docs: &[Vec<String>],
    n_spam: i64,
    n_ham: i64,
    thr: f64,
) -> (usize, usize) {
    let mut total = 0;
    let mut flagged = 0;
    for (i, toks) in docs.iter().enumerate() {
        if !is_holdout(i) {
            continue;
        }
        total += 1;
        let (s, _) = spam::classifier::score(counts, toks, n_spam, n_ham);
        if s >= thr {
            flagged += 1;
        }
    }
    (flagged, total)
}

#[test]
#[ignore = "requires SPAM_CORPUS_DIR with spam/ and ham/ subdirs"]
fn evaluate_false_positive_rate() {
    let root = std::env::var("SPAM_CORPUS_DIR")
        .expect("set SPAM_CORPUS_DIR to a folder containing spam/ and ham/ subdirs");
    let root = Path::new(&root);
    let spam = load_dir(&root.join("spam"));
    let ham = load_dir(&root.join("ham"));
    assert!(
        !spam.is_empty() && !ham.is_empty(),
        "corpus empty: spam={} ham={}",
        spam.len(),
        ham.len()
    );

    let mut counts: HashMap<String, (i64, i64)> = HashMap::new();
    let n_spam = learn_into(&mut counts, &spam, true);
    let n_ham = learn_into(&mut counts, &ham, false);

    println!(
        "learned: spam={n_spam} ham={n_ham}, distinct tokens={}",
        counts.len()
    );
    for thr in [spam::DEFAULT_THRESHOLD_LOW, spam::DEFAULT_THRESHOLD_HIGH] {
        let (fp, ham_n) = eval_flagged(&counts, &ham, n_spam, n_ham, thr);
        let (tp, spam_n) = eval_flagged(&counts, &spam, n_spam, n_ham, thr);
        let fpr = fp as f64 / ham_n.max(1) as f64;
        let recall = tp as f64 / spam_n.max(1) as f64;
        println!(
            "thr={thr:.2}: FPR(ham誤判定)={fp}/{ham_n}={fpr:.3}  検出率(spam)={tp}/{spam_n}={recall:.3}"
        );
    }

    // 安全側しきい値(τ_high)での誤検知率が十分低いこと（正当メール隔離は実害大）。
    let (fp, ham_n) = eval_flagged(&counts, &ham, n_spam, n_ham, spam::DEFAULT_THRESHOLD_HIGH);
    let fpr = fp as f64 / ham_n.max(1) as f64;
    assert!(fpr < 0.02, "誤検知率が高すぎます: {fp}/{ham_n}={fpr:.3}");
}
