//! Anchor walks: the universal node address (D-2, C2/C3, §5.3).
//!
//! The compiler computes each anchor once; generated code replays it as
//! `first_child`/`next_sibling` calls over slab handles — no queries, no
//! searches.

use serde::{Deserialize, Serialize};

/// A static walk from a template root to a hole.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Anchor(pub Vec<AnchorStep>);

impl Anchor {
    /// An anchor resolving to the template root itself.
    #[must_use]
    pub fn root() -> Self {
        Self(Vec::new())
    }

    /// Append a step.
    pub fn push(&mut self, step: AnchorStep) {
        self.0.push(step);
    }

    /// The steps, for replay.
    #[must_use]
    pub fn steps(&self) -> &[AnchorStep] {
        &self.0
    }
}

/// One addressing step (dom-expressions' `el$2 = el$1.firstChild` chain,
/// formalized).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnchorStep {
    /// Enter the i-th child.
    Down(u32),
    /// Advance to the next sibling.
    Next,
    /// Enter through a marker hole (the `<!>` of all three frameworks), i-th
    /// of the current node.
    MarkerDown(u32),
}

/// A reference the parent fragment may address: an anchor into the child's
/// template (§5.3 `returns`).
pub type NodeRef = Anchor;

#[cfg(test)]
mod tests {
    use super::{Anchor, AnchorStep};

    #[test]
    fn root_walk_is_empty() {
        let a = Anchor::root();
        assert!(a.steps().is_empty(), "root walk has no steps");
    }

    #[test]
    fn serde_shape_is_readable() {
        let mut a = Anchor::root();
        a.push(AnchorStep::Down(1));
        a.push(AnchorStep::Next);
        a.push(AnchorStep::MarkerDown(0));
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(json, r#"[{"Down":1},"Next",{"MarkerDown":0}]"#);
        let back: Anchor = serde_json::from_str(&json).unwrap();
        assert_eq!(back, a);
    }
}
