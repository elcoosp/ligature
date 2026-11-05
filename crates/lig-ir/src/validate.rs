//! Structural invariant checks (§13 `fixtures verify` substrate).
//!
//! The harness validates expected IR fixtures through this checker: template
//! hashes, anchor resolvability, id referential integrity, unique state
//! names, and the static memo cycle rule (§7.3 item 4 / R10).

use crate::ViewModule;
use crate::anchor::AnchorStep;
use crate::fragment::DynamicKind;
use crate::module::ReactionKind;
use crate::template::NodeIr;
use std::collections::{HashMap, HashSet};

/// One invariant violation, with a human-readable path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Where the violation was found, e.g. `fragments[2].dynamics[0].anchor`.
    pub path: String,
    /// What is wrong.
    pub message: String,
}

/// Validate a module; empty result means all invariants hold.
// One violation per rule keeps the checker flat and auditable.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn validate(m: &ViewModule) -> Vec<Violation> {
    let mut v: Vec<Violation> = Vec::new();

    if m.version != crate::VIEW_IR_VERSION {
        v.push(Violation {
            path: "version".into(),
            message: format!(
                "expected view_ir_version {}, found {}",
                crate::VIEW_IR_VERSION,
                m.version
            ),
        });
    }

    // Template hashes match content (C1 content addressing).
    for (ti, t) in m.templates.iter().enumerate() {
        let computed = crate::template::TemplateIr::compute_hash(&t.root);
        if computed != t.hash {
            v.push(Violation {
                path: format!("templates[{ti}].hash"),
                message: format!(
                    "content hash mismatch: stored {}, computed {computed}",
                    t.hash
                ),
            });
        }
    }

    // Referential integrity.
    let n_frags = m.fragments.len();
    for (ci, c) in m.components.iter().enumerate() {
        if c.root.get() as usize >= n_frags {
            v.push(Violation {
                path: format!("components[{ci}].root"),
                message: "dangling fragment id".into(),
            });
        }
    }
    for (fi, f) in m.fragments.iter().enumerate() {
        if f.template.get() as usize >= m.templates.len() {
            v.push(Violation {
                path: format!("fragments[{fi}].template"),
                message: "dangling template id".into(),
            });
        }
        for (di, d) in f.dynamics.iter().enumerate() {
            let path = format!("fragments[{fi}].dynamics[{di}]");
            if d.expr.get() as usize >= m.exprs.len() {
                v.push(Violation {
                    path: format!("{path}.expr"),
                    message: "dangling expr id".into(),
                });
            }
            if let Some(r) = d.reaction {
                if r.get() as usize >= m.reactions.len() {
                    v.push(Violation {
                        path: format!("{path}.reaction"),
                        message: "dangling reaction id".into(),
                    });
                } else if m.reactions[r.get() as usize].owner.get() as usize != fi {
                    v.push(Violation {
                        path: format!("{path}.reaction"),
                        message: "reaction owned by another fragment".into(),
                    });
                }
            }
            check_anchor(
                m,
                f.template.get() as usize,
                &d.anchor,
                &format!("{path}.anchor"),
                &mut v,
            );
            match &d.kind {
                DynamicKind::If(ifir) => {
                    if ifir.then.get() as usize >= n_frags {
                        v.push(Violation {
                            path: format!("{path}.if.then"),
                            message: "dangling fragment id".into(),
                        });
                    }
                }
                DynamicKind::For(forir) => {
                    if forir.body.get() as usize >= n_frags {
                        v.push(Violation {
                            path: format!("{path}.for.body"),
                            message: "dangling fragment id".into(),
                        });
                    }
                }
                DynamicKind::Component(c) => {
                    if c.component.get() as usize >= m.components.len() {
                        v.push(Violation {
                            path: format!("{path}.component"),
                            message: "dangling component id".into(),
                        });
                    }
                }
                DynamicKind::PropPass { child, .. } if child.get() as usize >= n_frags => {
                    v.push(Violation {
                        path: format!("{path}.prop_pass"),
                        message: "dangling fragment id".into(),
                    });
                }
                _ => {}
            }
        }
        for child in &f.children {
            if child.get() as usize >= n_frags {
                v.push(Violation {
                    path: format!("fragments[{fi}].children"),
                    message: "dangling fragment id".into(),
                });
            }
        }
    }

    // Pure-fn references valid.
    for (ei, e) in m.exprs.as_slice().iter().enumerate() {
        if let crate::expr::Expr::Call(f, _) = e
            && f.get() as usize >= m.pure_fns.len()
        {
            v.push(Violation {
                path: format!("exprs[{ei}]"),
                message: "dangling pure-fn id".into(),
            });
        }
    }

    // State names unique per owner.
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    for (si, s) in m.states.iter().enumerate() {
        let key = (s.owner.get(), s.name.get());
        if !seen.insert(key) {
            v.push(Violation {
                path: format!("states[{si}]"),
                message: "duplicate state name in fragment".into(),
            });
        }
        if s.owner.get() as usize >= n_frags {
            v.push(Violation {
                path: format!("states[{si}].owner"),
                message: "dangling fragment id".into(),
            });
        }
    }

    // Reaction owners valid.
    for (ri, r) in m.reactions.iter().enumerate() {
        if r.owner.get() as usize >= n_frags {
            v.push(Violation {
                path: format!("reactions[{ri}].owner"),
                message: "dangling fragment id".into(),
            });
        }
    }

    // Static memo cycle check (§7.3, R10): memos writing fields that other
    // memos read, transitively, are a compile-time cycle.
    v.extend(cycle_check(m));

    v
}

/// Resolve an anchor against a template root; `None` means the walk leaves
/// the tree (an invalid anchor — the compiler's bug, never the runtime's).
#[must_use]
pub fn resolve_anchor(m: &ViewModule, tpl: usize, anchor: &[AnchorStep]) -> Option<usize> {
    let root = m.templates.get(tpl)?.root.clone();
    // siblings stack: (children of current parent, current index)
    let mut siblings: Vec<NodeIr> = vec![root];
    let mut idx: usize = 0;
    for step in anchor {
        match step {
            AnchorStep::Down(i) => {
                let cur = siblings.get(idx)?;
                let NodeIr::Element { children, .. } = cur else {
                    return None;
                };
                let i = *i as usize;
                if i >= children.len() {
                    return None;
                }
                siblings = children.clone();
                idx = i;
            }
            AnchorStep::Next => {
                idx += 1;
                siblings.get(idx)?;
            }
            AnchorStep::MarkerDown(i) => {
                let cur = siblings.get(idx)?;
                let NodeIr::Element { children, .. } = cur else {
                    return None;
                };
                let i = *i as usize;
                if i >= children.len() || !matches!(children[i], NodeIr::Marker) {
                    return None;
                }
                siblings = children.clone();
                idx = i;
            }
        }
    }
    siblings.get(idx).map(|_| idx)
}

fn check_anchor(
    m: &ViewModule,
    tpl: usize,
    anchor: &crate::anchor::Anchor,
    path: &str,
    v: &mut Vec<Violation>,
) {
    if resolve_anchor(m, tpl, anchor.steps()).is_none() {
        v.push(Violation {
            path: path.to_string(),
            message: "anchor walk leaves the template".into(),
        });
    }
}

/// Field-level dataflow cycle detection over memos (static Deny rule).
fn cycle_check(m: &ViewModule) -> Vec<Violation> {
    // writer[field] = reaction indices writing it
    let mut writers: HashMap<u32, Vec<usize>> = HashMap::new();
    for (ri, r) in m.reactions.iter().enumerate() {
        if matches!(r.kind, ReactionKind::Memo { .. }) {
            for w in &r.writes {
                writers.entry(w.get()).or_default().push(ri);
            }
        }
    }
    // edges: memo ri -> memos writing any field ri reads
    let mut edges: Vec<Vec<usize>> = vec![Vec::new(); m.reactions.len()];
    for (ri, r) in m.reactions.iter().enumerate() {
        if matches!(r.kind, ReactionKind::Memo { .. }) {
            for read in &r.reads {
                if let Some(ws) = writers.get(&read.get()) {
                    for &w in ws {
                        if w != ri {
                            edges[ri].push(w);
                        }
                    }
                }
            }
        }
    }
    // DFS cycle detection
    let mut color = vec![0u8; m.reactions.len()];
    let mut cycles: Vec<Violation> = Vec::new();
    for start in 0..m.reactions.len() {
        if color[start] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        color[start] = 1;
        let mut path: Vec<usize> = vec![start];
        while let Some(&mut (node, ref mut next_ptr)) = stack.last_mut() {
            let next = *next_ptr;
            if next < edges[node].len() {
                stack.last_mut().unwrap().1 += 1;
                let target = edges[node][next];
                if color[target] == 1 {
                    cycles.push(Violation {
                        path: "reactions".into(),
                        message: format!(
                            "static memo cycle detected through reaction {target} (LIG-E020-family rule)"
                        ),
                    });
                } else if color[target] == 0 {
                    color[target] = 1;
                    path.push(target);
                    stack.push((target, 0));
                }
            } else {
                color[node] = 2;
                path.pop();
                stack.pop();
            }
        }
    }
    cycles
}

#[cfg(test)]
mod tests {
    use crate::anchor::{Anchor, AnchorStep};
    use crate::expr::{Expr, Lit};
    use crate::fragment::{
        ComponentDecl, DynamicIr, DynamicKind, FragmentIr, StateDecl, StateKind,
    };
    use crate::module::{ReactionDecl, ReactionKind, ViewModule};
    use crate::template::{NodeIr, TemplateIr};
    use crate::{ExprId, Frontend, Ty};

    fn base_module() -> ViewModule {
        let mut m = ViewModule::new(Frontend::Solid);
        let name = m.strings.intern("App");
        let tpl = m.intern_template(TemplateIr::new(NodeIr::Marker));
        let frag = m.push_fragment(FragmentIr {
            name,
            template: tpl,
            dynamics: vec![],
            children: vec![],
            props: vec![],
            returns: vec![],
        });
        m.components.push(ComponentDecl {
            name,
            root: frag,
            props: vec![],
        });
        m
    }

    #[test]
    fn valid_minimal_module_has_no_violations() {
        let m = base_module();
        assert!(
            crate::validate::validate(&m).is_empty(),
            "expected no violations"
        );
    }

    #[test]
    fn anchor_must_resolve_inside_the_template() {
        let mut m = base_module();
        let count = m.strings.intern("count");
        let e = m.insert_expr(Expr::State(count));
        let bad_anchor = Anchor(vec![AnchorStep::Down(3)]);
        m.fragments[0].dynamics.push(DynamicIr {
            anchor: bad_anchor,
            kind: DynamicKind::Text,
            expr: e,
            reaction: None,
            span: (0, 0),
        });
        let v = crate::validate::validate(&m);
        assert!(
            v.iter().any(|x| x.message.contains("anchor")),
            "expected anchor violation, got {v:?}"
        );
    }

    #[test]
    fn template_hash_mismatch_is_caught() {
        let mut m = base_module();
        m.templates[0].hash = 0xDEAD_BEEF;
        let v = crate::validate::validate(&m);
        assert!(v.iter().any(|x| x.message.contains("hash mismatch")));
    }

    #[test]
    fn memo_cycles_are_denied_statically() {
        let mut m = base_module();
        let a = m.strings.intern("a");
        let b = m.strings.intern("b");
        m.fragments[0].children.clear();
        let ea = m.insert_expr(Expr::State(b));
        let eb = m.insert_expr(Expr::State(a));
        m.reactions.push(ReactionDecl {
            kind: ReactionKind::Memo { target: a },
            reads: vec![b],
            writes: vec![a],
            body: ea,
            owner: crate::FragmentId::new(0),
        });
        m.reactions.push(ReactionDecl {
            kind: ReactionKind::Memo { target: b },
            reads: vec![a],
            writes: vec![b],
            body: eb,
            owner: crate::FragmentId::new(0),
        });
        let v = crate::validate::validate(&m);
        assert!(
            v.iter().any(|x| x.message.contains("cycle")),
            "expected static cycle violation, got {v:?}"
        );
    }

    #[test]
    fn dangling_ids_are_reported() {
        let mut m = base_module();
        let count = m.strings.intern("count");
        m.fragments[0].dynamics.push(DynamicIr {
            anchor: Anchor::root(),
            kind: DynamicKind::Text,
            expr: ExprId::new(999),
            reaction: None,
            span: (0, 0),
        });
        let init = m.insert_expr(Expr::Lit(Lit::Int(0)));
        m.states.push(StateDecl {
            name: count,
            owner: crate::FragmentId::new(77),
            kind: StateKind::Signal,
            ty: Ty::Int,
            init: Some(init),
        });
        let v = crate::validate::validate(&m);
        assert!(v.iter().any(|x| x.message.contains("dangling expr id")));
        assert!(v.iter().any(|x| x.message.contains("dangling fragment id")));
    }
}
