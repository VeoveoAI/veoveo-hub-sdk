//! Reciprocal rank fusion: several rankings of the same kind of item become one.
use std::collections::BTreeMap;

/// The constant of reciprocal rank fusion: an item at rank `r` of a list, counted from 1,
/// scores `1 / (RRF_K + r)` there.
pub const RRF_K: u32 = 60;

/// Fuses `lists`, each ranked best first, by reciprocal rank (k = 60): an item's score is
/// the sum of `1 / (60 + rank)` over the lists that hold it, and the result orders items
/// by that score, then by their best rank in any list, then by the items themselves, so
/// equal inputs give equal results. An item a list repeats counts at its first rank there.
pub fn fuse<T: Clone + Ord>(lists: &[Vec<T>]) -> Vec<T> {
    let mut scores: BTreeMap<T, (f64, usize)> = BTreeMap::new();
    for list in lists {
        let mut seen = std::collections::BTreeSet::new();
        for (index, item) in list.iter().enumerate() {
            if !seen.insert(item) {
                continue;
            }
            let rank = index + 1;
            let entry = scores.entry(item.clone()).or_insert((0.0, rank));
            entry.0 += 1.0 / (f64::from(RRF_K) + rank as f64);
            entry.1 = entry.1.min(rank);
        }
    }
    let mut fused: Vec<(T, f64, usize)> = scores
        .into_iter()
        .map(|(item, (score, best))| (item, score, best))
        .collect();
    fused.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then(a.2.cmp(&b.2))
            .then_with(|| a.0.cmp(&b.0))
    });
    fused.into_iter().map(|(item, _, _)| item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_both_lists_rank_rises_above_one_ranked_once() {
        let text = vec!["a", "b", "c"];
        let vector = vec!["c", "d"];
        // c: 1/63 + 1/61; a: 1/61; d: 1/62; b: 1/62 (b's best rank 2 ties d's 2).
        assert_eq!(fuse(&[text, vector]), ["c", "a", "b", "d"]);
    }

    #[test]
    fn one_list_keeps_its_order_and_none_is_empty() {
        assert_eq!(fuse(&[vec![3, 1, 2]]), [3, 1, 2]);
        assert!(fuse::<u8>(&[]).is_empty());
        assert!(fuse::<u8>(&[vec![], vec![]]).is_empty());
        // A repeat counts once, at its first rank.
        assert_eq!(fuse(&[vec![1, 2, 1], vec![2]]), [2, 1]);
    }
}
