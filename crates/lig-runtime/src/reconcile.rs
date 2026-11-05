//! Keyed list reconciliation (§7.5): LIS over keys (Vue Vapor's
//! `getSequence` behavior — chosen over Solid's udomdiff/Svelte 5's
//! linked-list matcher because our fragments are slab-indexed arrays; the
//! bench gate in §14.2 covers `runtime::list_diff_keyed`).

use alloc::vec;
use alloc::vec::Vec;

/// One reconcile step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOp {
    /// Insert a new item with `key` at `index` (new-sequence index).
    Insert {
        /// The item key.
        key: u64,
        /// Target index in the new sequence.
        index: usize,
    },
    /// Move an existing item from an old-sequence index to a new index.
    Move {
        /// Source index in the old sequence.
        from: usize,
        /// Target index in the new sequence.
        to: usize,
    },
    /// Remove the item at an old-sequence index.
    Remove {
        /// Index in the old sequence.
        index: usize,
    },
}

/// Longest increasing subsequence of `keys` — indices into `keys`, returned
/// in increasing order. The Vapor `getSequence` behavior: returns the
/// sequence of positions that do NOT need to move.
#[must_use]
pub fn lis_sequence(keys: &[u64]) -> Vec<usize> {
    if keys.is_empty() {
        return Vec::new();
    }
    // tails[i] = index into keys of the smallest tail of an increasing
    // subsequence of length i+1; prev chains reconstruct the sequence.
    let mut tails: Vec<usize> = Vec::with_capacity(keys.len());
    let mut prev: Vec<Option<usize>> = vec![None; keys.len()];
    for (i, &k) in keys.iter().enumerate() {
        // binary search for the first tail with keys[tail] >= k
        let mut lo = 0usize;
        let mut hi = tails.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if keys[tails[mid]] < k {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo > 0 {
            prev[i] = Some(tails[lo - 1]);
        }
        if lo == tails.len() {
            tails.push(i);
        } else {
            tails[lo] = i;
        }
    }
    let mut out = Vec::with_capacity(tails.len());
    let mut cur = Some(tails[tails.len() - 1]);
    while let Some(c) = cur {
        out.push(c);
        cur = prev[c];
    }
    out.reverse();
    out
}

/// Keyed diff: prefix/suffix trim, LIS-stable middle, then removes, moves,
/// inserts — the minimal-op shape of Vue Vapor's `createFor`.
#[must_use]
pub fn keyed_diff(old: &[u64], new: &[u64]) -> Vec<ListOp> {
    let mut ops = Vec::new();

    // Prefix trim.
    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    // Suffix trim.
    let mut suffix = 0;
    while suffix < old.len() - prefix
        && suffix < new.len() - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let old_mid = &old[prefix..old.len() - suffix];
    let new_mid = &new[prefix..new.len() - suffix];
    if old_mid.is_empty() && new_mid.is_empty() {
        return ops;
    }

    if old_mid.is_empty() {
        for (i, &k) in new_mid.iter().enumerate() {
            ops.push(ListOp::Insert {
                key: k,
                index: prefix + i,
            });
        }
        return ops;
    }
    if new_mid.is_empty() {
        for i in 0..old_mid.len() {
            ops.push(ListOp::Remove { index: prefix + i });
        }
        return ops;
    }

    // Map old keys → old index for O(1) lookup.
    let mut old_pos: alloc::collections::BTreeMap<u64, usize> = alloc::collections::BTreeMap::new();
    for (i, &k) in old_mid.iter().enumerate() {
        old_pos.insert(k, i);
    }

    // Removes: old keys not present in the new middle.
    for (i, &k) in old_mid.iter().enumerate() {
        if !new_mid.contains(&k) {
            ops.push(ListOp::Remove { index: prefix + i });
        }
    }

    // Anchored (unmoved) indices: LIS over the new-middle positions of
    // surviving old keys.
    let surviving: Vec<Option<usize>> = new_mid.iter().map(|k| old_pos.get(k).copied()).collect();
    let sources: Vec<usize> = surviving.iter().filter_map(|s| *s).collect();
    let sources_keys: Vec<u64> = sources.iter().map(|&i| old_mid[i]).collect();
    let lis = lis_sequence(&sources_keys);
    let anchored: alloc::collections::BTreeSet<usize> = lis.iter().map(|&li| sources[li]).collect();

    // Moves for survivors not in the LIS, inserts for brand-new keys —
    // emitted in new-sequence order.
    for (ni, slot) in surviving.iter().enumerate() {
        match slot {
            Some(oi) => {
                if !anchored.contains(oi) {
                    ops.push(ListOp::Move {
                        from: prefix + *oi,
                        to: prefix + ni,
                    });
                }
            }
            None => {
                ops.push(ListOp::Insert {
                    key: new_mid[ni],
                    index: prefix + ni,
                });
            }
        }
    }
    ops
}

/// Unkeyed (index) diff: positional comparison only (`Index` semantics,
/// §6.1).
#[must_use]
pub fn unkeyed_diff(old_len: usize, new_len: usize) -> Vec<ListOp> {
    let mut ops = Vec::new();
    let common = old_len.min(new_len);
    for i in common..new_len {
        ops.push(ListOp::Insert {
            key: u64::from(u32::try_from(i).unwrap_or(u32::MAX)),
            index: i,
        });
    }
    for i in (common..old_len).rev() {
        ops.push(ListOp::Remove { index: i });
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::{ListOp, keyed_diff, lis_sequence, unkeyed_diff};
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn lis_finds_longest_increasing_run() {
        // Vapor getSequence's canonical case: LIS length 3.
        let seq = lis_sequence(&[5, 3, 4, 9, 1, 2, 3]);
        assert_eq!(seq.len(), 3, "{seq:?}");
        assert!(
            seq.windows(2).all(|w| w[0] < w[1]),
            "indices increase: {seq:?}"
        );
        let keys: Vec<u64> = seq.iter().map(|&i| [5, 3, 4, 9, 1, 2, 3][i]).collect();
        assert!(
            keys.windows(2).all(|w| w[0] < w[1]),
            "values increase: {keys:?}"
        );
    }

    #[test]
    fn lis_edge_cases() {
        assert_eq!(lis_sequence(&[]).len(), 0);
        assert_eq!(lis_sequence(&[1]), vec![0]);
        assert_eq!(lis_sequence(&[3, 2, 1]).len(), 1);
        assert_eq!(lis_sequence(&[1, 2, 3]), vec![0, 1, 2]);
    }

    #[test]
    fn prefix_suffix_trim_produces_no_ops() {
        let ops = keyed_diff(&[1, 2, 3], &[1, 2, 3]);
        assert_eq!(ops.len(), 0);
    }

    #[test]
    fn pure_insert_and_remove() {
        let ops = keyed_diff(&[1, 2], &[1, 2, 9, 3]);
        assert_eq!(
            ops,
            vec![
                ListOp::Insert { key: 9, index: 2 },
                ListOp::Insert { key: 3, index: 3 }
            ]
        );
        let ops = keyed_diff(&[1, 2, 3], &[1]);
        assert_eq!(
            ops,
            vec![ListOp::Remove { index: 1 }, ListOp::Remove { index: 2 }]
        );
    }

    #[test]
    fn move_is_lis_stable() {
        // [a, b, c] → [c, b, a]: LIS keeps one item anchored, two move.
        let ops = keyed_diff(&[10, 20, 30], &[30, 20, 10]);
        let moves = ops
            .iter()
            .filter(|op| matches!(op, ListOp::Move { .. }))
            .count();
        assert!(moves >= 2, "reversal requires moves: {ops:?}");
    }

    #[test]
    fn mixed_shuffle_minimizes_moves() {
        // a b c d → d a b e: LIS over survivors [a b] keeps them anchored;
        // d moves to front, e inserts.
        let ops = keyed_diff(&[1, 2, 3, 4], &[4, 1, 2, 5]);
        let moves = ops
            .iter()
            .filter(|op| matches!(op, ListOp::Move { .. }))
            .count();
        let inserts = ops
            .iter()
            .filter(|op| matches!(op, ListOp::Insert { .. }))
            .count();
        let removes = ops
            .iter()
            .filter(|op| matches!(op, ListOp::Remove { .. }))
            .count();
        assert_eq!((moves, inserts, removes), (1, 1, 1), "{ops:?}");
    }

    #[test]
    fn unkeyed_index_diff() {
        let ops = unkeyed_diff(2, 4);
        assert_eq!(ops.len(), 2);
        let ops = unkeyed_diff(4, 1);
        assert_eq!(ops.len(), 3);
        assert_eq!(unkeyed_diff(3, 3).len(), 0);
    }
}
