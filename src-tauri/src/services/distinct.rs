//! 利用者が「別人」と判断した連絡先の対（重複の整理で同じ組にしない）。DB にも People API にも
//! 触らない。
//!
//! 重複の整理で「別人（統合しない）」を押した組・統合でチェックを外した人を、学習ではなく
//! 判断の記録として持ち、重複の検出（[`crate::services::dedupe::group`]）とまとめての統合
//! （[`crate::services::sure_duplicates::sure_groups`]）が従う（利用者の判断 2026-10-10・
//! docs/CONTACT_MODEL.md §1-5）。記録そのものは `store::contact_distinct`。

use std::collections::HashSet;

/// 対の向きをそろえる（小さい ID が先）。
pub fn ordered(a: i64, b: i64) -> (i64, i64) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

/// 「別人」の対の集合。
#[derive(Debug, Default, Clone)]
pub struct DistinctPairs(HashSet<(i64, i64)>);

impl DistinctPairs {
    /// 対の並びから作る（向きは問わない）。
    pub fn new(pairs: impl IntoIterator<Item = (i64, i64)>) -> Self {
        Self(pairs.into_iter().map(|(a, b)| ordered(a, b)).collect())
    }

    /// `a` と `b` が「別人」と記録されているか。
    pub fn contains(&self, a: i64, b: i64) -> bool {
        self.0.contains(&ordered(a, b))
    }

    /// 組を、別人どうしが同じ組に入らないように分け直す。並び順に、入れられる最初の組へ入れ
    /// （入れられなければ新しい組を作る）、2 件以上の組だけを返す。`id` は要素の連絡先 ID。
    ///
    /// 3 人以上の組で一部だけが別人なら、残りで組を作り直すことになる。
    pub fn split<T>(&self, members: Vec<T>, id: impl Fn(&T) -> i64) -> Vec<Vec<T>> {
        let mut parts: Vec<Vec<T>> = Vec::new();
        for m in members {
            let mid = id(&m);
            match parts
                .iter_mut()
                .find(|p| p.iter().all(|o| !self.contains(id(o), mid)))
            {
                Some(p) => p.push(m),
                None => parts.push(vec![m]),
            }
        }
        parts.retain(|p| p.len() > 1);
        parts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_ignore_direction() {
        let d = DistinctPairs::new([(5, 2)]);
        assert!(d.contains(2, 5));
        assert!(d.contains(5, 2));
        assert!(!d.contains(2, 3));
    }

    #[test]
    fn split_keeps_the_rest_together() {
        // 1 と 3 だけが別人: 1・2 は組のまま、3 は 1 人になるので外れる。
        let d = DistinctPairs::new([(1, 3)]);
        assert_eq!(d.split(vec![1, 2, 3], |&x| x), vec![vec![1, 2]]);
        // 1 と 3、2 と 4 が別人: {1,2} と {3,4} に分かれる。
        let d = DistinctPairs::new([(1, 3), (2, 3), (1, 4), (2, 4)]);
        assert_eq!(
            d.split(vec![1, 2, 3, 4], |&x| x),
            vec![vec![1, 2], vec![3, 4]]
        );
    }

    #[test]
    fn all_distinct_leaves_no_group() {
        let d = DistinctPairs::new([(1, 2)]);
        assert!(d.split(vec![1, 2], |&x| x).is_empty());
        assert_eq!(DistinctPairs::default().split(vec![1, 2], |&x| x).len(), 1);
    }
}
