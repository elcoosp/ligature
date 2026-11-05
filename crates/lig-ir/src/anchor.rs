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

use crate::ViewModule;
use lig_core::id::FragmentId;

/// Fill `module.fragments[frag].dynamics[i].anchor` from the root-relative
/// child-index paths recorded during template extraction (§6.1
/// `anchor.rs::assign`). Shared by every frontend: anchor walks are a
/// property of the canonical IR, not of the surface syntax.
pub fn assign_anchor_paths(module: &mut ViewModule, frag: FragmentId, paths: &[Vec<u32>]) {
    let Some(f) = module.fragments.get_mut(frag.get() as usize) else {
        return;
    };
    for (di, path) in paths.iter().enumerate() {
        if di >= f.dynamics.len() {
            break;
        }
        let steps: Vec<AnchorStep> = path.iter().map(|&i| AnchorStep::Down(i)).collect();
        f.dynamics[di].anchor = Anchor(steps);
    }
}

#[cfg(test)]
mod tests {
    use super::{Anchor, AnchorStep, assign_anchor_paths};
    use crate::fragment::DynamicIr;
    use crate::template::TemplateIr;
    use crate::{DynamicKind, ExprId, FragmentIr, Frontend, ViewModule};
    use lig_core::id::FragmentId;

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

    #[test]
    fn paths_become_down_chains() {
        let mut m = ViewModule::new(Frontend::Solid);
        let name = m.strings.intern("F");
        let tpl = m.intern_template(TemplateIr::new(crate::template::NodeIr::Marker));
        m.fragments.push(FragmentIr {
            name,
            template: tpl,
            dynamics: vec![DynamicIr {
                anchor: Anchor::root(),
                kind: DynamicKind::Text,
                expr: ExprId::new(0),
                reaction: None,
                span: (0, 0),
            }],
            children: vec![],
            props: vec![],
            returns: vec![],
        });
        assign_anchor_paths(&mut m, FragmentId::new(0), &[vec![1, 2]]);
        let steps = m.fragments[0].dynamics[0].anchor.steps();
        assert_eq!(steps, &[AnchorStep::Down(1), AnchorStep::Down(2)]);
    }
}
