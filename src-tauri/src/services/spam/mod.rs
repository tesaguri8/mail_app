//! 迷惑メールのローカル学習（docs/SPAM.md）。
//! フェーズA（端末内のみ）: トークン化 + Bayesian 分類 + 学習フィードバック。
//! TSG One 共有（フェーズB）は本モジュールには含めない（共有を切っても動く設計）。

pub mod apply;
pub mod classifier;
pub mod tokenize;

pub use tokenize::tokenize;

/// 段階1の既定しきい値（docs/SPAM.md §8.1 の二段しきい値）。
/// §9 でユーザー設定（tauri-plugin-store）へ移すため、ここは既定値のみ。
pub const DEFAULT_THRESHOLD_LOW: f64 = 0.5;
pub const DEFAULT_THRESHOLD_HIGH: f64 = 0.9;

/// スコアを 3 バンド（clean / uncertain / junk）へ分類する（§8.1）。
pub fn band(score: f64, low: f64, high: f64) -> &'static str {
    if score >= high {
        "junk"
    } else if score >= low {
        "uncertain"
    } else {
        "clean"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_splits_three_ways() {
        assert_eq!(band(0.2, 0.5, 0.9), "clean");
        assert_eq!(band(0.7, 0.5, 0.9), "uncertain");
        assert_eq!(band(0.95, 0.5, 0.9), "junk");
        // 境界は下側に含める（>=）。
        assert_eq!(band(0.9, 0.5, 0.9), "junk");
        assert_eq!(band(0.5, 0.5, 0.9), "uncertain");
    }
}
