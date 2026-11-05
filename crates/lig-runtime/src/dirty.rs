//! Dirty masks (§7.2): `u64` fast path with an `alloc` side-array spill past
//! 64 reactions — one code path for setters, documented, allocation-free in
//! steady state for components within the limit.

use alloc::vec::Vec;

/// Reaction-count limit of the `u64` fast path.
pub const REACTION_LIMIT: usize = 64;

/// A set of dirty reaction bits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirtySet {
    low: u64,
    spill: Vec<u64>,
}

impl DirtySet {
    /// An empty set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            low: 0,
            spill: Vec::new(),
        }
    }

    /// Whether any bit is set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.low == 0 && self.spill.iter().all(|w| *w == 0)
    }

    /// Mark reaction `index`.
    pub fn set(&mut self, index: usize) {
        if index < REACTION_LIMIT {
            self.low |= 1 << index;
        } else {
            let word = (index - REACTION_LIMIT) / 64;
            let bit = (index - REACTION_LIMIT) % 64;
            if self.spill.len() <= word {
                self.spill.resize(word + 1, 0);
            }
            self.spill[word] |= 1 << bit;
        }
    }

    /// Union a whole mask (the fast-path setter pattern, §9.2).
    pub fn set_mask(&mut self, mask: u64) {
        self.low |= mask;
    }

    /// Read reaction `index`.
    #[must_use]
    pub fn get(&self, index: usize) -> bool {
        if index < REACTION_LIMIT {
            self.low & (1 << index) != 0
        } else {
            let word = (index - REACTION_LIMIT) / 64;
            let bit = (index - REACTION_LIMIT) % 64;
            self.spill.get(word).is_some_and(|w| w & (1 << bit) != 0)
        }
    }

    /// The low 64 bits (the fast-path schedule scan).
    #[must_use]
    pub const fn low(&self) -> u64 {
        self.low
    }

    /// Take all bits, leaving the set empty (`core::mem::take` of §7.2).
    #[must_use]
    pub fn take(&mut self) -> Self {
        core::mem::replace(self, Self::new())
    }

    /// Union `other` into `self`.
    pub fn union(&mut self, other: &Self) {
        self.low |= other.low;
        for (i, w) in other.spill.iter().enumerate() {
            if self.spill.len() <= i {
                self.spill.resize(i + 1, 0);
            }
            self.spill[i] |= w;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DirtySet, REACTION_LIMIT};

    #[test]
    fn low_bits_roundtrip() {
        let mut d = DirtySet::new();
        assert!(d.is_empty());
        d.set(0);
        d.set(63);
        assert!(d.get(0));
        assert!(d.get(63));
        assert!(!d.get(1));
        assert!(!d.is_empty());
        assert_eq!(d.low(), (1 << 0) | (1 << 63));
    }

    #[test]
    fn spill_beyond_64() {
        let mut d = DirtySet::new();
        d.set(64);
        d.set(130);
        assert!(d.get(64));
        assert!(d.get(130));
        assert!(!d.get(65));
        let taken = d.take();
        assert!(taken.get(130));
        assert!(d.is_empty());
    }

    #[test]
    fn union_and_mask() {
        let mut a = DirtySet::new();
        let mut b = DirtySet::new();
        a.set_mask(0b101);
        b.set(1);
        b.set(REACTION_LIMIT + 3);
        a.union(&b);
        assert!(a.get(0));
        assert!(a.get(1));
        assert!(a.get(2));
        assert!(a.get(REACTION_LIMIT + 3));
    }
}
