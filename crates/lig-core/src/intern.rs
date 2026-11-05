//! Insertion-stable string interning (§5.1: "index-stable, sorted on
//! serialize").
//!
//! During lowering ids are assigned in insertion order (stable while the table
//! grows); [`StringTable::canonicalize`] rewrites the table into sorted order
//! and returns the remap so producers can canonicalize a module before
//! serialization (§5.6).

use crate::id::StrId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The module-wide string table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringTable {
    entries: Vec<String>,
    /// Build-only reverse index; excluded from serialization (canonical form
    /// must be reconstructible from `entries` alone).
    #[serde(skip)]
    reverse: BTreeMap<String, StrId>,
}

impl StringTable {
    /// An empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            reverse: BTreeMap::new(),
        }
    }

    /// Intern `s`, returning its stable id.
    pub fn intern(&mut self, s: impl Into<String>) -> StrId {
        let s = s.into();
        if let Some(&id) = self.reverse.get(&s) {
            return id;
        }
        let id = StrId::new(
            self.entries
                .len()
                .try_into()
                .expect("string table overflow"),
        );
        self.entries.push(s.clone());
        self.reverse.insert(s, id);
        id
    }

    /// Resolve an id to its string.
    #[must_use]
    pub fn get(&self, id: StrId) -> &str {
        self.entries
            .get(id.get() as usize)
            .map_or("<dangling StrId>", String::as_str)
    }

    /// Number of interned strings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate `(id, string)` in current (insertion) order.
    pub fn iter(&self) -> impl Iterator<Item = (StrId, &str)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(i, s)| (StrId::new(i as u32), s.as_str()))
    }

    /// Rewrite into sorted order; returns `old_id -> new_id`.
    ///
    /// After this call ids refer to positions in the sorted `entries`, the
    /// canonical serialization shape (§5.6). Idempotent.
    pub fn canonicalize(&mut self) -> Vec<StrId> {
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        order.sort_by(|&a, &b| self.entries[a].cmp(&self.entries[b]));
        let mut remap = vec![StrId::new(0); self.entries.len()];
        for (new_pos, &old_pos) in order.iter().enumerate() {
            remap[old_pos] = StrId::new(new_pos as u32);
        }
        let sorted: Vec<String> = order.iter().map(|&i| self.entries[i].clone()).collect();
        self.entries = sorted;
        self.reverse = BTreeMap::from_iter(
            self.entries
                .iter()
                .enumerate()
                .map(|(i, s)| (s.clone(), StrId::new(i as u32))),
        );
        remap
    }
}

#[cfg(test)]
mod tests {
    use super::StringTable;
    use crate::id::StrId;

    #[test]
    fn interning_is_deduped() {
        let mut t = StringTable::new();
        let a = t.intern("count");
        let b = t.intern("count");
        let c = t.intern("doubled");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(t.get(a), "count");
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn canonicalize_sorts_and_is_idempotent() {
        let mut t = StringTable::new();
        for s in ["zeta", "alpha", "mid"] {
            t.intern(s);
        }
        let remap = t.canonicalize();
        assert_eq!(t.get(StrId::new(0)), "alpha");
        assert_eq!(t.get(StrId::new(1)), "mid");
        assert_eq!(t.get(StrId::new(2)), "zeta");
        assert_eq!(remap[0], StrId::new(2)); // zeta moved 0 -> 2

        let before = t.clone();
        let remap2 = t.canonicalize();
        assert_eq!(t, before);
        assert!(remap2.iter().enumerate().all(|(i, r)| r.get() == i as u32));
    }

    #[test]
    fn serde_skips_build_index() {
        let mut t = StringTable::new();
        t.intern("solo");
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#"{"entries":["solo"]}"#);
        let back: StringTable = serde_json::from_str(&json).unwrap();
        assert_eq!(back.get(StrId::new(0)), "solo");
    }
}
