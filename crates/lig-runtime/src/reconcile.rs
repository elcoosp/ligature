//! Keyed list reconciliation (§7.5): LIS over keys (Vue Vapor's
//! `getSequence` behavior — chosen over Solid's udomdiff/Svelte 5's
//! linked-list matcher because our fragments are slab-indexed arrays; the
//! bench gate in §14.2 covers `runtime::list_diff_keyed`).
//!
//! Two layers, both pinned by tests:
//!
//! - `keyed_diff` — the *diff*: the minimal-op intent (removes, LIS-stable
//!   moves, inserts) as old/new-index ops;
//! - `plan_keyed` — the *plan*: sequential `SeqOp`s against the live list,
//!   what generated reconcile methods execute (an op list is not directly
//!   executable — removals and moves shift the indices later ops address).
//!   Since M5 the generated keyed path runs `plan_keyed`; the diff stays the
//!   minimal-op reference, and its permutation pins keep the two honest
//!   against each other.

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

    // Anchored (unmoved) indices: LIS over the surviving old indices in new
    // order — Vue's `getSequence` criterion (relative old order preserved).
    // The LIS must run over the *indices*, not the key values: for the swap
    // `[2, 1] → [1, 2]` a value-LIS would anchor both items and freeze the
    // swap (caught by the M5 permutation pins).
    let surviving: Vec<Option<usize>> = new_mid.iter().map(|k| old_pos.get(k).copied()).collect();
    let sources: Vec<usize> = surviving.iter().filter_map(|s| *s).collect();
    let sources_u64: Vec<u64> = sources.iter().map(|&i| i as u64).collect();
    let lis = lis_sequence(&sources_u64);
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

/// One *sequenced* reconcile step: every index is a live position at the
/// moment the step applies.
///
/// A raw diff op list is not directly executable against the live slab (an
/// early removal shifts every later index; a move shifts every survivor
/// past it). `plan_keyed` emits steps the generated reconcile runs as a
/// plain sequential `match` — each step addressing the list exactly as it
/// stands at that moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqOp {
    /// Remove the live item at `index`.
    Remove {
        /// Live index at apply time.
        index: usize,
    },
    /// Move the live item at `from` to `to` (both live at apply time).
    Move {
        /// Live index at apply time.
        from: usize,
        /// Live index at apply time.
        to: usize,
    },
    /// Insert a new item with `key` at live `index`.
    Insert {
        /// The item key (as reported by `keyed_diff`).
        key: u64,
        /// Live index at apply time.
        index: usize,
    },
}

/// The executable keyed plan: sequential `SeqOp`s transforming a live list
/// holding `old` into exactly `new` (§7.5).
///
/// Two phases, both simulated on a key model so every emitted index is a
/// live index at apply time:
///
/// 1. **removes** — old items the new sequence does not keep (multiset-aware;
///    one step per dropped instance);
/// 2. **prefix walk** — positions left of `k` hold the final sequence, so
///    settling position `k` is one step: an existing item found right of the
///    prefix moves onto it, a missing key inserts. Each step keeps the
///    prefix invariant, so the walk lands on `new` by construction.
///
/// Keys are the item identities (the keyed contract, §7.5 — unique per
/// list; `key_hash` normalizes string keys). Move counts stay within n−1
/// and match the LIS bound for reversals and front insertions; the planner
/// trades the diff's move-minimality for index-free sequential execution.
#[must_use]
pub fn plan_keyed(old: &[u64], new: &[u64]) -> Vec<SeqOp> {
    use alloc::collections::BTreeMap;
    let mut model: Vec<u64> = old.to_vec();
    let mut steps: Vec<SeqOp> = Vec::new();

    // Phase 1 — removes.
    let mut want: BTreeMap<u64, usize> = BTreeMap::new();
    for &k in new {
        *want.entry(k).or_insert(0) += 1;
    }
    let mut kept: BTreeMap<u64, usize> = BTreeMap::new();
    let mut i = 0;
    while i < model.len() {
        let k = model[i];
        let room = want.get(&k).copied().unwrap_or(0);
        let used = kept.get(&k).copied().unwrap_or(0);
        if used < room {
            kept.insert(k, used + 1);
            i += 1;
        } else {
            model.remove(i);
            steps.push(SeqOp::Remove { index: i });
        }
    }

    // Phase 2 — prefix walk.
    for (k, &key) in new.iter().enumerate() {
        if model.get(k) == Some(&key) {
            continue;
        }
        // With unique keys the item for position k cannot sit inside the
        // settled prefix, so the search starts right of it.
        let found = model
            .iter()
            .skip(k + 1)
            .position(|&m| m == key)
            .map(|p| p + k + 1);
        if let Some(q) = found {
            model.remove(q);
            model.insert(k, key);
            steps.push(SeqOp::Move { from: q, to: k });
        } else {
            let index = k.min(model.len());
            model.insert(index, key);
            steps.push(SeqOp::Insert { key, index });
        }
    }
    steps
}

/// The u64 normalization for string keys (§7.5): FNV-1a over the UTF-8
/// bytes — dependency-free, deterministic across hosts and runs.
#[must_use]
pub fn key_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::{ListOp, SeqOp, key_hash, keyed_diff, lis_sequence, plan_keyed, unkeyed_diff};
    use alloc::vec;
    use alloc::vec::Vec;

    /// Apply `plan_keyed(old, new)` on a plain model list — the same
    /// remove/move/insert semantics the slab `Fragment` methods implement
    /// (`list_remove` / `list_move` / `list_insert`).
    fn apply_keyed(old: &[u64], new: &[u64]) -> Vec<u64> {
        let mut live: Vec<u64> = old.to_vec();
        for step in plan_keyed(old, new) {
            match step {
                SeqOp::Remove { index } => {
                    live.remove(index);
                }
                SeqOp::Move { from, to } => {
                    let item = live.remove(from);
                    live.insert(to.min(live.len()), item);
                }
                SeqOp::Insert { key, index } => {
                    live.insert(index.min(live.len()), key);
                }
            }
        }
        live
    }

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

    #[test]
    fn key_hash_is_fnv1a() {
        // Standard FNV-1a 64 test vectors.
        assert_eq!(key_hash(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(key_hash("a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(key_hash("foobar"), 0x8594_4171_f739_67e8);
        assert_ne!(key_hash("alpha"), key_hash("beta"));
    }

    #[test]
    fn keyed_diff_move_criterion_is_the_source_order() {
        // The LIS runs over the surviving old indices, not the key values:
        // a two-item swap must emit a move (the value-LIS would anchor both
        // items and freeze the swap).
        let ops = keyed_diff(&[2, 1], &[1, 2]);
        assert_eq!(
            ops,
            vec![ListOp::Move { from: 1, to: 0 }],
            "swap requires one move: {ops:?}"
        );
    }

    #[test]
    fn sequenced_plan_reorders_without_recreating() {
        // Rotation: one move, the LIS-stable shape.
        let live = apply_keyed(&[1, 2, 3], &[3, 1, 2]);
        assert_eq!(live, vec![3, 1, 2]);
        // Full reversal: the hard case — move targets shift as moves apply.
        let live = apply_keyed(&[1, 2, 3], &[3, 2, 1]);
        assert_eq!(live, vec![3, 2, 1]);
        // Remove + insert in the middle.
        let live = apply_keyed(&[1, 2, 3], &[1, 9, 3]);
        assert_eq!(live, vec![1, 9, 3]);
        // Identical lists: no steps at all.
        assert_eq!(plan_keyed(&[7, 8, 9], &[7, 8, 9]), []);
    }

    #[test]
    fn sequenced_plan_survives_every_permutation_pair() {
        // Exhaustive over 4-key permutations on both sides (24 × 24) plus
        // shrink/grow pairs — the sequential application must land exactly
        // on the new sequence every time, or the generated reconcile lies.
        let keys = [1u64, 2, 3, 4];
        let perms = permutate(&keys);
        for old in &perms {
            for new in &perms {
                assert_eq!(apply_keyed(old, new), *new, "old {old:?} → new {new:?}");
            }
        }
        // Shrinks: drop to every 2-key subsequence.
        for new in [[1u64, 2], [2, 3], [3, 4], [1, 4], [2, 4], [1, 3]] {
            for old in &perms {
                assert_eq!(apply_keyed(old, &new), new, "old {old:?} → new {new:?}");
            }
        }
        // Grows: every 2-key seed into every full permutation.
        for old in [[1u64, 2], [3, 4], [2, 1]] {
            for new in &perms {
                assert_eq!(apply_keyed(&old, new), *new, "old {old:?} → new {new:?}");
            }
        }
    }

    fn permutate(rest: &[u64]) -> Vec<Vec<u64>> {
        if rest.is_empty() {
            return vec![Vec::new()];
        }
        let mut out = Vec::new();
        for i in 0..rest.len() {
            let mut tail = Vec::new();
            tail.extend_from_slice(&rest[..i]);
            tail.extend_from_slice(&rest[i + 1..]);
            for mut p in permutate(&tail) {
                p.insert(0, rest[i]);
                out.push(p);
            }
        }
        out
    }

    #[test]
    fn plan_shape_is_removes_then_prefix_walk() {
        // [1 2 3] → [2 4 1]: drop 3, move 2 over 1, insert 4, then 1
        // settles into the tail position untouched.
        let steps = plan_keyed(&[1, 2, 3], &[2, 4, 1]);
        assert_eq!(
            steps,
            vec![
                SeqOp::Remove { index: 2 },
                SeqOp::Move { from: 1, to: 0 },
                SeqOp::Insert { key: 4, index: 1 },
            ],
            "{steps:?}"
        );
    }
}
