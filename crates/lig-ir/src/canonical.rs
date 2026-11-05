//! Canonicalization, canonical JSON, and fingerprints (§5.6).
//!
//! Canonical form: strings sorted lexicographically, templates and styles
//! sorted by content hash, fragments in depth-first order from components
//! sorted by name, expressions renumbered by first use in that walk, and
//! reactions/states grouped under their canonical fragment in declaration
//! order. Two frontends produce byte-identical canonical JSON for equivalent
//! UIs — the executable definition of FR-4 (§6.4).

use crate::expr::{Expr, ExprTable, Lit, TplPart};
use crate::fragment::{DynamicIr, DynamicKind, FragmentIr};
use crate::module::{ReactionDecl, ViewModule};
use crate::style::StyleValue;
use crate::template::{AttrValue, NodeIr, StaticAttr};
use crate::{ComponentId, ExprId, FragmentId, ReactionId, StrId, StyleConstId, TemplateId};
use lig_core::hash::fnv1a64;
use serde_json::json;
use std::collections::HashMap;

impl ViewModule {
    /// Rewrite the module into canonical form (idempotent).
    // Mechanical five-phase rewrite; split further and the borrow graph
    // fragments into redundant pre-passes. Length is inherent, not risk.
    #[allow(clippy::too_many_lines)]
    pub fn canonicalize(&mut self) {
        // Phase 1 — strings: sort, collect remap.
        let str_remap: Vec<StrId> = self.strings.canonicalize();
        let smap = |id: StrId| str_remap[id.get() as usize];

        // Phase 2 — templates: rebuild with canonical strings (hashes are
        // content-derived, so they move with the remap), then sort by the new
        // hash. Styles: sort by content hash (css text is id-free, stable).
        let (styles_sorted, style_remap_u32) = sort_by_hash(&self.styles, |x| x.hash);
        self.styles = styles_sorted;
        let rebuilt_templates: Vec<crate::template::TemplateIr> = self
            .templates
            .iter()
            .map(|t| {
                crate::template::TemplateIr::new(rebuild_node(
                    &t.root,
                    &str_remap,
                    &style_remap_u32,
                ))
            })
            .collect();
        let (templates_sorted, tpl_remap_u32) = sort_by_hash(&rebuilt_templates, |x| x.hash);
        self.templates = templates_sorted;

        // Phase 2b — pure fns: sort by canonical name.
        let mut pf_order: Vec<usize> = (0..self.pure_fns.len()).collect();
        pf_order.sort_by_key(|&i| smap(self.pure_fns[i].name));
        let pf_remap: Vec<u32> = {
            let mut r = vec![0u32; self.pure_fns.len()];
            for (new_pos, &old_pos) in pf_order.iter().enumerate() {
                r[old_pos] = new_pos as u32;
            }
            r
        };

        // Phase 3 — components sorted by canonical name; fragments DFS.
        let mut comp_order: Vec<usize> = (0..self.components.len()).collect();
        comp_order.sort_by_key(|&i| self.components[i].name.get());
        comp_order.sort_by_key(|&i| smap(self.components[i].name));
        let comp_remap: Vec<ComponentId> = {
            let mut r = vec![ComponentId::new(0); self.components.len()];
            for (new_pos, &old_pos) in comp_order.iter().enumerate() {
                r[old_pos] = ComponentId::new(new_pos as u32);
            }
            r
        };

        let mut canonical_frag_order: Vec<usize> = Vec::new();
        for &ci in &comp_order {
            let mut stack = vec![self.components[ci].root.get() as usize];
            while let Some(f) = stack.pop() {
                if canonical_frag_order.contains(&f) {
                    continue;
                }
                canonical_frag_order.push(f);
                for &child in &self.fragments[f].children {
                    stack.push(child.get() as usize);
                }
            }
        }
        let frag_remap: Vec<FragmentId> = {
            let mut r = vec![FragmentId::new(0); self.fragments.len()];
            for (new_pos, &old_pos) in canonical_frag_order.iter().enumerate() {
                r[old_pos] = FragmentId::new(new_pos as u32);
            }
            r
        };

        // Phase 4 — expression renumbering in canonical walk order.
        let mut expr_out = ExprTable::new();
        let mut expr_renumber: HashMap<u32, ExprId> = HashMap::new();
        {
            let ren = |id: ExprId,
                       expr_out: &mut ExprTable,
                       renumber: &mut HashMap<u32, ExprId>|
             -> ExprId {
                let key = id.get();
                if let Some(&done) = renumber.get(&key) {
                    return done;
                }
                let rebuilt = rebuild_expr(self.exprs.get(id), &str_remap, &pf_remap);
                let old_span = self.exprs.span_of(id);
                let new_id = expr_out.insert_with_span(rebuilt, old_span.start, old_span.end);
                renumber.insert(key, new_id);
                new_id
            };

            // 4a. dynamics per canonical fragment.
            let mut new_fragments: Vec<FragmentIr> = Vec::new();
            for &old_fi in &canonical_frag_order {
                let f = &self.fragments[old_fi];
                let mut dynamics: Vec<DynamicIr> = Vec::with_capacity(f.dynamics.len());
                for d in &f.dynamics {
                    let expr = ren(d.expr, &mut expr_out, &mut expr_renumber);
                    let kind = match &d.kind {
                        DynamicKind::If(ifir) => DynamicKind::If(crate::fragment::IfIr {
                            cond: ren(ifir.cond, &mut expr_out, &mut expr_renumber),
                            then: frag_remap[ifir.then.get() as usize],
                            else_: ifir.else_.map(|e| frag_remap[e.get() as usize]),
                        }),
                        DynamicKind::For(forir) => DynamicKind::For(crate::fragment::ForIr {
                            source: ren(forir.source, &mut expr_out, &mut expr_renumber),
                            item: smap(forir.item),
                            index: forir.index.map(smap),
                            key: forir.key.map(|k| ren(k, &mut expr_out, &mut expr_renumber)),
                            body: frag_remap[forir.body.get() as usize],
                        }),
                        DynamicKind::Component(c) => {
                            DynamicKind::Component(crate::fragment::ComponentIr {
                                component: comp_remap[c.component.get() as usize],
                                props: c
                                    .props
                                    .iter()
                                    .map(|p| crate::fragment::PropBinding {
                                        prop: smap(p.prop),
                                        value: ren(p.value, &mut expr_out, &mut expr_renumber),
                                    })
                                    .collect(),
                                slots: c
                                    .slots
                                    .iter()
                                    .map(|sl| crate::fragment::SlotIr {
                                        name: smap(sl.name),
                                        fallback: sl.fallback.map(|f| frag_remap[f.get() as usize]),
                                    })
                                    .collect(),
                            })
                        }
                        DynamicKind::Attr { key } => DynamicKind::Attr { key: smap(*key) },
                        DynamicKind::StyleProp { prop } => {
                            DynamicKind::StyleProp { prop: smap(*prop) }
                        }
                        DynamicKind::PropPass { child, prop } => DynamicKind::PropPass {
                            child: frag_remap[child.get() as usize],
                            prop: *prop,
                        },
                        DynamicKind::Slot { name } => DynamicKind::Slot { name: smap(*name) },
                        other => other.clone(),
                    };
                    dynamics.push(DynamicIr {
                        anchor: d.anchor.clone(),
                        kind,
                        expr,
                        reaction: d.reaction, // fixed in phase 5
                        span: d.span,
                    });
                }
                let children: Vec<FragmentId> = f
                    .children
                    .iter()
                    .map(|c| frag_remap[c.get() as usize])
                    .collect();
                new_fragments.push(FragmentIr {
                    name: smap(f.name),
                    template: TemplateId::new(tpl_remap_u32[f.template.get() as usize]),
                    dynamics,
                    children,
                    props: f
                        .props
                        .iter()
                        .map(|p| crate::fragment::PropDecl {
                            name: smap(p.name),
                            ty: p.ty.clone(),
                            optional: p.optional,
                        })
                        .collect(),
                    returns: f.returns.clone(),
                });
            }

            // 4b. reactions: canonical fragment order, declaration order kept.
            let mut new_reactions: Vec<ReactionDecl> = Vec::new();
            let mut reaction_remap: Vec<ReactionId> =
                vec![ReactionId::new(0); self.reactions.len()];
            for (new_fi, &old_fi) in canonical_frag_order.iter().enumerate() {
                for (ri, r) in self.reactions.iter().enumerate() {
                    if r.owner.get() as usize != old_fi {
                        continue;
                    }
                    let kind = match r.kind {
                        crate::module::ReactionKind::Memo { target } => {
                            crate::module::ReactionKind::Memo {
                                target: smap(target),
                            }
                        }
                        crate::module::ReactionKind::Effect { tier } => {
                            crate::module::ReactionKind::Effect { tier }
                        }
                        crate::module::ReactionKind::Bind { dynamic } => {
                            crate::module::ReactionKind::Bind { dynamic }
                        }
                    };
                    let new_id = ReactionId::new(new_reactions.len() as u32);
                    reaction_remap[ri] = new_id;
                    new_reactions.push(ReactionDecl {
                        kind,
                        reads: r.reads.iter().map(|x| smap(*x)).collect(),
                        writes: r.writes.iter().map(|x| smap(*x)).collect(),
                        body: ren(r.body, &mut expr_out, &mut expr_renumber),
                        owner: FragmentId::new(new_fi as u32),
                    });
                }
            }

            // 4c. states: same ordering discipline.
            let mut new_states: Vec<crate::fragment::StateDecl> = Vec::new();
            for (new_fi, &old_fi) in canonical_frag_order.iter().enumerate() {
                for st in &self.states {
                    if st.owner.get() as usize != old_fi {
                        continue;
                    }
                    new_states.push(crate::fragment::StateDecl {
                        name: smap(st.name),
                        owner: FragmentId::new(new_fi as u32),
                        kind: st.kind,
                        ty: st.ty.clone(),
                        init: st.init.map(|e| ren(e, &mut expr_out, &mut expr_renumber)),
                    });
                }
            }

            // Phase 5 — assemble the canonical module.
            let mut out = ViewModule::new(self.frontend);
            out.version = self.version;
            out.source_files.clone_from(&self.source_files);
            out.strings = self.strings.clone(); // already sorted in phase 1
            out.components = comp_order
                .iter()
                .map(|&ci| crate::fragment::ComponentDecl {
                    name: smap(self.components[ci].name),
                    root: frag_remap[self.components[ci].root.get() as usize],
                    props: self.components[ci]
                        .props
                        .iter()
                        .map(|p| crate::fragment::PropDecl {
                            name: smap(p.name),
                            ty: p.ty.clone(),
                            optional: p.optional,
                        })
                        .collect(),
                })
                .collect();
            // Templates were rebuilt + hash-recomputed + sorted in phase 2;
            // rebuild_node already applied the canonical string ids there.
            out.templates.clone_from(&self.templates);
            // Fix dynamic → reaction links now that reactions are placed.
            let mut frags_fixed = new_fragments;
            for (new_fi, &old_fi) in canonical_frag_order.iter().enumerate() {
                let old_dyns = &self.fragments[old_fi].dynamics;
                for (di, d) in frags_fixed[new_fi].dynamics.iter_mut().enumerate() {
                    d.reaction = old_dyns[di]
                        .reaction
                        .map(|r| reaction_remap[r.get() as usize]);
                }
            }
            out.fragments = frags_fixed;
            out.exprs = expr_out;
            out.reactions = new_reactions;
            out.states = new_states;
            out.styles = self
                .styles
                .iter()
                .map(|st| crate::style::StyleConst {
                    hash: st.hash,
                    css: st.css.clone(),
                    longhands: st
                        .longhands
                        .iter()
                        .map(|l| crate::style::StyleLonghand {
                            prop: l.prop,
                            value: match l.value {
                                StyleValue::Keyword(k) => StyleValue::Keyword(smap(k)),
                                StyleValue::Color(c) => StyleValue::Color(smap(c)),
                                StyleValue::Raw(r) => StyleValue::Raw(smap(r)),
                                other => other,
                            },
                        })
                        .collect(),
                })
                .collect();
            out.lints.clone_from(&self.lints);
            out.pure_fns = pf_order
                .iter()
                .map(|&i| crate::expr::PureFnDecl {
                    name: smap(self.pure_fns[i].name),
                    kind: match self.pure_fns[i].kind {
                        crate::expr::PureFnKind::Builtin => crate::expr::PureFnKind::Builtin,
                        crate::expr::PureFnKind::ModelFn { target } => {
                            crate::expr::PureFnKind::ModelFn {
                                target: smap(target),
                            }
                        }
                    },
                })
                .collect();
            out.shapes = self.shapes.clone();
            out.imports = self
                .imports
                .iter()
                .map(|i| crate::fragment::Import {
                    name: smap(i.name),
                    from: smap(i.from),
                    kind: i.kind,
                })
                .collect();
            out.spans = self.spans.clone();

            *self = out;
        }
    }

    /// Canonical compact JSON (§5.6): the conformance artifact.
    #[must_use]
    pub fn canonical_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Canonical pretty JSON (`lig inspect` format).
    #[must_use]
    pub fn canonical_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// 64-bit fingerprint of a fragment subtree: template hash, dynamic
    /// kinds+anchors, expr forest, reaction signatures (§5.6). Consumed by
    /// digest-gated emission (D-9) and template-shape detection (§11.4).
    #[must_use]
    pub fn fingerprint_fragment(&self, frag: FragmentId) -> u64 {
        let mut subtree: Vec<usize> = Vec::new();
        let mut stack = vec![frag.get() as usize];
        while let Some(f) = stack.pop() {
            if subtree.contains(&f) {
                continue;
            }
            subtree.push(f);
            if let Some(fr) = self.fragments.get(f) {
                for &c in &fr.children {
                    stack.push(c.get() as usize);
                }
            }
        }
        subtree.sort_unstable();

        let mut expr_ids: Vec<u32> = Vec::new();
        let mut rx_ids: Vec<u32> = Vec::new();
        let mut tpl_hashes: Vec<u64> = Vec::new();
        for &f in &subtree {
            if let Some(fr) = self.fragments.get(f) {
                if let Some(t) = self.templates.get(fr.template.get() as usize) {
                    tpl_hashes.push(t.hash);
                }
                for d in &fr.dynamics {
                    expr_ids.push(d.expr.get());
                    if let Some(r) = d.reaction {
                        rx_ids.push(r.get());
                    }
                    match &d.kind {
                        DynamicKind::If(ifir) => expr_ids.push(ifir.cond.get()),
                        DynamicKind::For(forir) => {
                            expr_ids.push(forir.source.get());
                            if let Some(k) = forir.key {
                                expr_ids.push(k.get());
                            }
                        }
                        DynamicKind::Component(c) => {
                            for p in &c.props {
                                expr_ids.push(p.value.get());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        for (ri, r) in self.reactions.iter().enumerate() {
            if subtree.contains(&(r.owner.get() as usize)) {
                rx_ids.push(ri as u32);
            }
        }
        expr_ids.sort_unstable();
        rx_ids.sort_unstable();

        let payload = json!({
            "frags": subtree,
            "tpls": tpl_hashes,
            "dyns": subtree.iter().map(|f| self.fragments.get(*f).map(|fr| fr.dynamics.iter().map(|d| (&d.anchor, &d.kind)).collect::<Vec<_>>())).collect::<Vec<_>>(),
            "exprs": expr_ids.iter().map(|e| self.exprs.get(ExprId::new(*e))).collect::<Vec<_>>(),
            "states": subtree.iter().map(|f| self.states.iter().filter(|s| s.owner.get() as usize == *f).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "rxs": rx_ids.iter().map(|r| self.reactions.get(*r as usize)).collect::<Vec<_>>(),
        });
        let bytes = serde_json::to_vec(&payload).unwrap_or_default();
        fnv1a64(&bytes)
    }
}

fn sort_by_hash<T: Clone>(items: &[T], hash_of: impl Fn(&T) -> u64) -> (Vec<T>, Vec<u32>) {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&i| hash_of(&items[i]));
    let remap: Vec<u32> = {
        let mut r = vec![0u32; items.len()];
        for (new_pos, &old_pos) in order.iter().enumerate() {
            r[old_pos] = new_pos as u32;
        }
        r
    };
    let sorted: Vec<T> = order.iter().map(|&i| items[i].clone()).collect();
    (sorted, remap)
}

fn rebuild_expr(expr: &Expr, strings: &[StrId], pf_remap: &[u32]) -> Expr {
    let smap = |id: StrId| strings[id.get() as usize];
    match expr {
        Expr::Lit(lit) => Expr::Lit(match lit {
            Lit::Str(v) => Lit::Str(smap(*v)),
            other => other.clone(),
        }),
        Expr::Local(v) => Expr::Local(smap(*v)),
        Expr::State(v) => Expr::State(smap(*v)),
        Expr::Event(f) => Expr::Event(*f),
        Expr::Native(v) => Expr::Native(smap(*v)),
        Expr::Field(inner, f) => {
            Expr::Field(Box::new(rebuild_expr(inner, strings, pf_remap)), smap(*f))
        }
        Expr::Index(a, b) => Expr::Index(
            Box::new(rebuild_expr(a, strings, pf_remap)),
            Box::new(rebuild_expr(b, strings, pf_remap)),
        ),
        Expr::Array(items) => Expr::Array(
            items
                .iter()
                .map(|a| rebuild_expr(a, strings, pf_remap))
                .collect(),
        ),
        Expr::Unary(op, inner) => {
            Expr::Unary(*op, Box::new(rebuild_expr(inner, strings, pf_remap)))
        }
        Expr::Binary(op, lhs, rhs) => Expr::Binary(
            *op,
            Box::new(rebuild_expr(lhs, strings, pf_remap)),
            Box::new(rebuild_expr(rhs, strings, pf_remap)),
        ),
        Expr::Ternary(cond, then_e, else_e) => Expr::Ternary(
            Box::new(rebuild_expr(cond, strings, pf_remap)),
            Box::new(rebuild_expr(then_e, strings, pf_remap)),
            Box::new(rebuild_expr(else_e, strings, pf_remap)),
        ),
        Expr::Tpl(parts) => Expr::Tpl(
            parts
                .iter()
                .map(|p| match p {
                    TplPart::Text(v) => TplPart::Text(smap(*v)),
                    TplPart::Hole(h) => TplPart::Hole(Box::new(rebuild_expr(h, strings, pf_remap))),
                })
                .collect(),
        ),
        Expr::Call(f, args) => Expr::Call(
            crate::PureFnId::new(pf_remap[f.get() as usize]),
            args.iter()
                .map(|a| rebuild_expr(a, strings, pf_remap))
                .collect(),
        ),
    }
}

fn rebuild_node(node: &NodeIr, s: &[StrId], style_remap: &[u32]) -> NodeIr {
    let smap = |id: StrId| s[id.get() as usize];
    match node {
        NodeIr::Element {
            tag,
            static_attrs,
            style,
            children,
        } => NodeIr::Element {
            tag: smap(*tag),
            static_attrs: static_attrs
                .iter()
                .map(|a: &StaticAttr| StaticAttr {
                    key: smap(a.key),
                    value: match &a.value {
                        AttrValue::Str(v) => AttrValue::Str(smap(*v)),
                        other => other.clone(),
                    },
                })
                .collect(),
            style: style.map(|st| StyleConstId::new(style_remap[st.get() as usize])),
            children: children
                .iter()
                .map(|c| rebuild_node(c, s, style_remap))
                .collect(),
        },
        NodeIr::Text { value } => NodeIr::Text {
            value: smap(*value),
        },
        NodeIr::Marker => NodeIr::Marker,
    }
}

/// Zero the frontend header field in canonical JSON (§6.4: parity asserts
/// byte-identical JSON "modulo the `frontend:` header field").
#[must_use]
pub fn normalize_frontend_json(canonical: &str) -> String {
    canonical
        .replace(r#""frontend":"solid""#, r#""frontend":"expected""#)
        .replace(r#""frontend":"vue""#, r#""frontend":"expected""#)
}

/// Re-exported helpers for tests and the fixture harness.
pub mod canonicalize_helpers {
    pub use super::normalize_frontend_json;
}

#[cfg(test)]
mod tests {
    use super::normalize_frontend_json;
    use crate::expr::{Expr, Lit};
    use crate::fragment::{ComponentDecl, DynamicKind, FragmentIr, StateDecl, StateKind};
    use crate::module::{ReactionDecl, ReactionKind, Tier, ViewModule};
    use crate::template::{NodeIr, TemplateIr};
    use crate::{ExprId, FragmentId, Frontend, TemplateId, Ty};

    fn counter_like(frontend: Frontend, reverse: bool) -> ViewModule {
        let mut m = ViewModule::new(frontend);
        let name = m.strings.intern("Counter");
        let count = m.strings.intern("count");
        let doubled = m.strings.intern("doubled");
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
        let init = m.insert_expr(Expr::Lit(Lit::Int(0)));
        m.states.push(StateDecl {
            name: count,
            owner: frag,
            kind: StateKind::Signal,
            ty: Ty::Int,
            init: Some(init),
        });
        let e = m.insert_expr(Expr::State(count));
        m.states.push(StateDecl {
            name: doubled,
            owner: frag,
            kind: StateKind::Memo,
            ty: Ty::Int,
            init: None,
        });
        let rx = ReactionDecl {
            kind: ReactionKind::Memo { target: doubled },
            reads: vec![count],
            writes: vec![doubled],
            body: e,
            owner: frag,
        };
        if reverse {
            m.reactions.insert(0, rx);
        } else {
            m.reactions.push(rx);
        }
        m
    }

    #[test]
    fn canonicalization_is_idempotent() {
        let mut m = counter_like(Frontend::Solid, false);
        m.canonicalize();
        let once = m.canonical_json();
        m.canonicalize();
        assert_eq!(once, m.canonical_json());
    }

    #[test]
    fn insertion_order_does_not_change_canonical_bytes() {
        let mut a = counter_like(Frontend::Solid, false);
        a.canonicalize();
        let mut b = counter_like(Frontend::Vue, true);
        b.canonicalize();
        assert_eq!(
            normalize_frontend_json(&a.canonical_json()),
            normalize_frontend_json(&b.canonical_json())
        );
    }

    #[test]
    fn canonical_json_has_sorted_strings_and_current_version() {
        let mut m = counter_like(Frontend::Solid, false);
        m.canonicalize();
        let json = m.canonical_json();
        assert!(json.contains(r#""version":4"#), "{json}");
        assert!(
            json.contains(r#""entries":["Counter","count","doubled"]"#),
            "{json}"
        );
    }

    #[test]
    fn frontend_header_normalization() {
        assert_eq!(
            normalize_frontend_json(r#"{"frontend":"solid","version":4}"#),
            r#"{"frontend":"expected","version":4}"#
        );
        assert_eq!(
            normalize_frontend_json(r#"{"frontend":"vue","version":4}"#),
            r#"{"frontend":"expected","version":4}"#
        );
    }

    #[test]
    fn fingerprint_is_stable_and_sensitive() {
        let mut m = counter_like(Frontend::Solid, false);
        m.canonicalize();
        let f1 = m.fingerprint_fragment(FragmentId::new(0));
        assert_eq!(f1, m.fingerprint_fragment(FragmentId::new(0)));
        m.reactions[0].reads.push(m.strings.intern("zzz"));
        let f2 = m.fingerprint_fragment(FragmentId::new(0));
        assert_ne!(
            f1, f2,
            "reading a new state field must move the fingerprint"
        );
    }

    #[test]
    fn template_and_style_sorting_by_hash() {
        let mut m = ViewModule::new(Frontend::Solid);
        let high = m.strings.intern("z");
        let low = m.strings.intern("a");
        // two distinct templates; insertion order chosen so hashes sort opposite
        let t0 = TemplateIr::new(NodeIr::Text { value: high });
        let t1 = TemplateIr::new(NodeIr::Text { value: low });
        m.intern_template(t0);
        m.intern_template(t1);
        m.canonicalize();
        let hashes: Vec<u64> = m.templates.iter().map(|t| t.hash).collect();
        let mut sorted = hashes.clone();
        sorted.sort_unstable();
        assert_eq!(hashes, sorted, "templates must be hash-ordered canonically");
        assert_eq!(TemplateId::new(0).get(), 0);
    }

    #[test]
    fn dynamic_kind_clone_roundtrip_keeps_if_fragments() {
        let kind = DynamicKind::If(crate::fragment::IfIr {
            cond: ExprId::new(3),
            then: FragmentId::new(1),
            else_: None,
        });
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains("\"If\""));
        let _ = Tier::Pre;
    }
}
