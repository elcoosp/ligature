//! Byte-range spans: `(start, end)` `u32` pairs, 8 bytes, oxc-compatible
//! (§2.5: `oxc_span/src/span.rs:90`).

use serde::{Deserialize, Serialize};

/// A half-open byte range `[start, end)` into a source file.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct Span {
    /// Start byte offset (inclusive).
    pub start: u32,
    /// End byte offset (exclusive).
    pub end: u32,
}

impl Span {
    /// The placeholder span used where no source position is known.
    pub const PLACEHOLDER: Self = Self { start: 0, end: 0 };

    /// Build a span from byte offsets.
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// Length in bytes; `end - start`, clamped to 0 for inverted spans.
    #[must_use]
    pub const fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    /// A zero-length span.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.end <= self.start
    }

    /// Smallest span covering both `self` and `other`.
    #[must_use]
    pub const fn join(self, other: Self) -> Self {
        let start = if self.start <= other.start {
            self.start
        } else {
            other.start
        };
        let end = if self.end >= other.end {
            self.end
        } else {
            other.end
        };
        Self { start, end }
    }

    /// Whether `other` lies fully inside `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        other.start >= self.start && other.end <= self.end
    }
}

#[cfg(test)]
mod tests {
    use super::Span;

    #[test]
    fn len_and_empty() {
        assert_eq!(Span::new(2, 7).len(), 5);
        assert!(Span::new(3, 3).is_empty());
        assert!(Span::new(5, 2).is_empty()); // inverted spans clamp to empty
    }

    #[test]
    fn join_and_contains() {
        let a = Span::new(0, 10);
        let b = Span::new(4, 20);
        assert_eq!(a.join(b), Span::new(0, 20));
        assert!(a.contains(Span::new(1, 9)));
        assert!(!a.contains(b));
    }

    #[test]
    fn is_eight_bytes() {
        assert_eq!(size_of::<Span>(), 8);
    }

    #[test]
    fn serde_roundtrip() {
        let s = Span::new(12, 34);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"start":12,"end":34}"#);
        assert_eq!(serde_json::from_str::<Span>(&json).unwrap(), s);
    }
}
