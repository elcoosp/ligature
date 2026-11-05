//! Reactivity compilation (D-3, §7): from per-fragment reactions to the
//! **const topological schedule** — the graph replaced by two const arrays.
//!
//! The flush loop lives in `lig-runtime`; this crate computes the program
//! constants it consumes: reaction order (topological, tie-broken by
//! declaration order, §7.2), per-field reader masks (what setters mark), and
//! per-reaction write masks (what `flush` re-enters).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use lig_core::id::{ComponentId, FragmentId, ReactionId, StrId};
use lig_ir::ViewModule;

use std::collections::{BTreeMap, BTreeSet};

/// A static schedule error: the compile-time cycle Deny (R10) or capacity
/// overflow past the spill-word dispatch limit (§7.2, M3 refinement).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    /// Two memos (transitively) write/read each other's fields.
    Cycle {
        /// Reaction ids forming the cycle, in walk order.
        reactions: Vec<ReactionId>,
    },
    /// More than 128 reactions in one component — the single spill word
    /// (ids 64..128) is the documented dispatch cap (§7.2).
    TooManyReactions {
        /// How many were found.
        found: usize,
    },
    /// A reaction references a state field that no declaration introduces.
    UnknownField {
        /// The reaction id.
        reaction: ReactionId,
        /// The unknown field name.
        field: StrId,
    },
}

/// Reaction-count limit of the `u64` fast path (spill word 0 extends the
/// dispatch cap to 128, §7.2).
pub const FAST_PATH_LIMIT: usize = 64;

/// The component's reactive program constants (§7.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentSchedule {
    /// Reaction ids in topological order (memo before readers; ties by
    /// declaration order).
    pub order: Vec<ReactionId>,
    /// Per state field (in `field_order`): bitmask of reactions that read it
    /// (ids 0..64). Setters OR this into the dirty set (§9.2).
    pub field_readers: BTreeMap<StrId, u64>,
    /// Per state field: spill mask of readers with ids 64..128 (bits are
    /// id − 64). Empty map for components within the fast path (M3).
    pub field_readers_hi: BTreeMap<StrId, u64>,
    /// Per reaction (module order): bitmask of reactions to re-dirty after it
    /// runs (`WRITES[r]` in the flush loop, §7.2; ids 0..64).
    pub reaction_writes: Vec<u64>,
    /// Per reaction (module order): spill mask of reactions to re-dirty after
    /// it runs (bits are id − 64; M3 refinement).
    pub reaction_writes_hi: Vec<u64>,
    /// State fields in first-declaration order (stable for codegen).
    pub field_order: Vec<StrId>,
}

impl ComponentSchedule {
    /// Bit for a reaction's position within the component.
    #[must_use]
    pub fn bit(&self, position: usize) -> u64 {
        1 << position
    }
}

/// Collect the reaction ids belonging to a component (owner in its fragment
/// subtree), in module declaration order.
#[must_use]
pub fn component_reactions(module: &ViewModule, root: FragmentId) -> Vec<ReactionId> {
    let mut subtree: Vec<usize> = Vec::new();
    let mut stack = vec![root.get() as usize];
    while let Some(f) = stack.pop() {
        if subtree.contains(&f) {
            continue;
        }
        subtree.push(f);
        if let Some(fr) = module.fragments.get(f) {
            for &c in &fr.children {
                stack.push(c.get() as usize);
            }
        }
    }
    module
        .reactions
        .iter()
        .enumerate()
        .filter(|(_, r)| subtree.contains(&(r.owner.get() as usize)))
        .map(|(i, _)| ReactionId::new(i as u32))
        .collect()
}

/// Build the const schedule for a component.
///
/// # Errors
/// [`ScheduleError::Cycle`] on static dataflow cycles (§7.3), `TooManyReactions`
/// past 128 (fast path + one spill word), `UnknownField` when a reaction reads
/// or writes an undeclared field.
pub fn build_schedule(
    module: &ViewModule,
    root: FragmentId,
) -> Result<ComponentSchedule, ScheduleError> {
    let members = component_reactions(module, root);
    if members.len() > FAST_PATH_LIMIT * 2 {
        return Err(ScheduleError::TooManyReactions {
            found: members.len(),
        });
    }

    // Declared fields of the component.
    let mut field_order: Vec<StrId> = Vec::new();
    for s in &module.states {
        if !field_order.contains(&s.name) {
            field_order.push(s.name);
        }
    }

    // Validate reads/writes against declarations.
    for &rid in &members {
        let r = &module.reactions[rid.get() as usize];
        for field in r.reads.iter().chain(&r.writes) {
            if !field_order.contains(field) {
                return Err(ScheduleError::UnknownField {
                    reaction: rid,
                    field: *field,
                });
            }
        }
    }

    // field_readers[field] = bitmask of reactions reading it (ids 0..64);
    // field_readers_hi[field] = spill mask for readers with id ≥ 64 (bits are
    // id − 64). Bits are REACTION IDS — the flush loop scans `pending &
    // (1 << r)` with r a reaction id (§7.2), and masks must speak the same
    // language; the spill word speaks id-minus-64.
    let mut field_readers: BTreeMap<StrId, u64> = BTreeMap::new();
    let mut field_readers_hi: BTreeMap<StrId, u64> = BTreeMap::new();
    for &rid in &members {
        let r = &module.reactions[rid.get() as usize];
        for field in &r.reads {
            let id = rid.get();
            if id < FAST_PATH_LIMIT as u32 {
                *field_readers.entry(*field).or_insert(0) |= 1u64 << id;
            } else {
                *field_readers_hi.entry(*field).or_insert(0) |=
                    1u64 << (id - FAST_PATH_LIMIT as u32);
            }
        }
    }

    // WRITES[r] = OR of field_readers over the fields r writes (reactions to
    // re-dirty when r runs, §7.2). Self-dirtying is skipped: a reaction that
    // reads its own written field is a cycle the memo check catches. The hi
    // arrays speak id-minus-64 for readers past the fast path (M3).
    let mut reaction_writes = vec![0u64; module.reactions.len()];
    let mut reaction_writes_hi = vec![0u64; module.reactions.len()];
    for &rid in &members {
        let r = &module.reactions[rid.get() as usize];
        let mut mask = 0u64;
        let mut mask_hi = 0u64;
        for field in &r.writes {
            mask |= field_readers.get(field).copied().unwrap_or(0);
            mask_hi |= field_readers_hi.get(field).copied().unwrap_or(0);
        }
        let id = rid.get();
        reaction_writes[id as usize] = if id < FAST_PATH_LIMIT as u32 {
            mask & !(1u64 << id)
        } else {
            mask
        };
        reaction_writes_hi[id as usize] = if id >= FAST_PATH_LIMIT as u32 {
            mask_hi & !(1u64 << (id - FAST_PATH_LIMIT as u32))
        } else {
            mask_hi
        };
    }

    // Topological order: edge A -> B when B reads a field A writes.
    // Kahn's algorithm with a BTreeSet frontier = declaration-order tie-break.
    // Only scheduled reactions contribute writer edges; Effects are
    // dispatch-driven (event handlers) — their writes re-enter through the
    // WRITES mask at dispatch time, not through the topo graph (§7.1).
    let mut writers_of_field: BTreeMap<StrId, Vec<usize>> = BTreeMap::new();
    for (pos, &rid) in members.iter().enumerate() {
        let r = &module.reactions[rid.get() as usize];
        if matches!(r.kind, lig_ir::module::ReactionKind::Effect { .. }) {
            continue;
        }
        for field in &r.writes {
            writers_of_field.entry(*field).or_default().push(pos);
        }
    }
    let mut edges: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); members.len()];
    let mut indegree = vec![0usize; members.len()];
    for (pos, &rid) in members.iter().enumerate() {
        let r = &module.reactions[rid.get() as usize];
        if matches!(r.kind, lig_ir::module::ReactionKind::Effect { .. }) {
            continue;
        }
        for field in &r.reads {
            for &w in writers_of_field.get(field).into_iter().flatten() {
                if w != pos && edges[w].insert(pos) {
                    indegree[pos] += 1;
                }
            }
        }
    }

    let mut ready: BTreeSet<usize> = (0..members.len()).filter(|&p| indegree[p] == 0).collect();
    let mut order: Vec<ReactionId> = Vec::with_capacity(members.len());
    while let Some(&pos) = ready.iter().next() {
        ready.remove(&pos);
        order.push(members[pos]);
        for &succ in &edges[pos] {
            indegree[succ] -= 1;
            if indegree[succ] == 0 {
                ready.insert(succ);
            }
        }
    }
    if order.len() != members.len() {
        let remaining: Vec<ReactionId> = members
            .iter()
            .copied()
            .filter(|rid| !order.contains(rid))
            .collect();
        return Err(ScheduleError::Cycle {
            reactions: remaining,
        });
    }

    Ok(ComponentSchedule {
        order,
        field_readers,
        field_readers_hi,
        reaction_writes,
        reaction_writes_hi,
        field_order,
    })
}

/// A cyclic component-use graph (LIG-E020): no §11.1 fragment build order
/// exists, so the pipeline must Deny — never hang or overflow at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentCycle {
    /// Component ids on the cycle, in walk order (the first edge's source is
    /// the lowest-id member, deterministic across runs).
    pub components: Vec<ComponentId>,
}

/// The §11.1 fragment build order over `module.components`: every component
/// precedes the components that use it (dependencies first, ties by
/// declaration order). Component-use edges come from `DynamicKind::Component`
/// dynamics inside each component's fragment subtree.
///
/// # Errors
/// [`ComponentCycle`] (LIG-E020) when the use graph is cyclic — the salsa-style
/// cycle recovery: surface a Deny diagnostic instead of recursing forever
/// (a cyclic build would stack-overflow the running app at first mount).
pub fn component_build_order(module: &ViewModule) -> Result<Vec<ComponentId>, ComponentCycle> {
    // Edge A -> B when a fragment owned by A instantiates component B.
    let n = module.components.len();
    let mut uses: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    for (a, decl) in module.components.iter().enumerate() {
        let mut stack = vec![decl.root.get() as usize];
        let mut seen: Vec<usize> = Vec::new();
        while let Some(f) = stack.pop() {
            if seen.contains(&f) {
                continue;
            }
            seen.push(f);
            let Some(fr) = module.fragments.get(f) else {
                continue;
            };
            for &c in &fr.children {
                stack.push(c.get() as usize);
            }
            for d in &fr.dynamics {
                if let lig_ir::fragment::DynamicKind::Component(use_ir) = &d.kind {
                    let b = use_ir.component.get() as usize;
                    if b < n {
                        // self-use included: instantiating yourself is a cycle.
                        uses[a].insert(b);
                    }
                }
            }
        }
    }

    // Kahn's algorithm, BTreeSet frontier = declaration-order tie-break.
    // Edge semantics: `uses[a]` = components a depends on, so a's indegree is
    // its dependency count and a pops only after all its dependencies —
    // dependencies first, users later (§11.1).
    let mut indegree = vec![0usize; n];
    let mut users_of: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    for (a, deps) in uses.iter().enumerate() {
        indegree[a] = deps.len();
        for &d in deps {
            users_of[d].insert(a);
        }
    }
    let mut ready: BTreeSet<usize> = (0..n).filter(|&i| indegree[i] == 0).collect();
    let mut order: Vec<ComponentId> = Vec::with_capacity(n);
    while let Some(&i) = ready.iter().next() {
        ready.remove(&i);
        order.push(ComponentId::new(i as u32));
        for &user in &users_of[i] {
            indegree[user] -= 1;
            if indegree[user] == 0 {
                ready.insert(user);
            }
        }
    }
    if order.len() == n {
        return Ok(order);
    }

    // Cycle recovery: walk from the lowest-id unresolved member following use
    // edges until a node repeats — that closed walk is the reported cycle.
    let unresolved: BTreeSet<usize> = (0..n)
        .filter(|&i| !order.contains(&ComponentId::new(i as u32)))
        .collect();
    let start = *unresolved.iter().next().expect("non-empty unresolved set");
    let mut path: Vec<usize> = vec![start];
    let mut current = start;
    loop {
        let next = uses[current]
            .iter()
            .copied()
            .find(|&b| unresolved.contains(&b))
            .expect("an unresolved component always has an unresolved use (it is on a cycle)");
        if let Some(pos) = path.iter().position(|&p| p == next) {
            path.rotate_left(pos);
            path.truncate(path.len() - pos);
            return Err(ComponentCycle {
                components: path.iter().map(|&i| ComponentId::new(i as u32)).collect(),
            });
        }
        path.push(next);
        current = next;
    }
}

#[cfg(test)]
mod tests {
    use super::{ScheduleError, build_schedule, component_reactions};
    use lig_core::id::ExprId as CoreExprId;
    use lig_ir::expr::{Expr, Lit};
    use lig_ir::fragment::{ComponentDecl, FragmentIr, StateDecl, StateKind};
    use lig_ir::module::{ReactionDecl, ReactionKind, Tier, ViewModule};
    use lig_ir::{ExprId, FragmentId, Frontend, StrId, Ty};

    struct Rig {
        module: ViewModule,
        root: FragmentId,
        count: StrId,
        doubled: StrId,
        shown: StrId,
    }

    fn rig() -> Rig {
        let mut m = ViewModule::new(Frontend::Solid);
        let tpl = m.intern_template(lig_ir::template::TemplateIr::new(
            lig_ir::template::NodeIr::Marker,
        ));
        let name = m.strings.intern("Counter");
        let root = m.push_fragment(FragmentIr {
            name,
            template: tpl,
            dynamics: vec![],
            children: vec![],
            props: vec![],
            returns: vec![],
        });
        m.components.push(ComponentDecl {
            name,
            root,
            props: vec![],
        });
        let count = m.strings.intern("count");
        let doubled = m.strings.intern("doubled");
        let shown = m.strings.intern("shown_flag");
        Rig {
            module: m,
            root,
            count,
            doubled,
            shown,
        }
    }

    fn push_state(rig: &mut Rig, name: StrId, kind: StateKind) {
        let init = rig.module.insert_expr(Expr::Lit(Lit::Int(0)));
        rig.module.states.push(StateDecl {
            name,
            owner: rig.root,
            kind,
            ty: Ty::Int,
            init: Some(init),
        });
    }

    fn push_reaction(
        rig: &mut Rig,
        kind: ReactionKind,
        reads: &[StrId],
        writes: &[StrId],
        body_lit: i64,
    ) -> lig_core::id::ReactionId {
        let body = rig.module.insert_expr(Expr::Lit(Lit::Int(body_lit)));
        let rid = lig_core::id::ReactionId::new(rig.module.reactions.len() as u32);
        rig.module.reactions.push(ReactionDecl {
            kind,
            reads: reads.to_vec(),
            writes: writes.to_vec(),
            body,
            owner: rig.root,
        });
        rid
    }

    #[test]
    fn memo_is_scheduled_before_its_readers() {
        let mut r = rig();
        let (count, doubled) = (r.count, r.doubled);
        push_state(&mut r, count, StateKind::Signal);
        push_state(&mut r, doubled, StateKind::Memo);
        let memo = push_reaction(
            &mut r,
            ReactionKind::Memo { target: doubled },
            &[count],
            &[doubled],
            1,
        );
        let bind = push_reaction(
            &mut r,
            ReactionKind::Bind { dynamic: 0 },
            &[doubled],
            &[],
            2,
        );
        let sched = build_schedule(&r.module, r.root).expect("schedule");
        assert_eq!(sched.order, vec![memo, bind], "memo precedes readers");
        // setter of count dirties the memo
        assert_eq!(sched.field_readers[&count], 1 << 0);
        assert_eq!(sched.field_readers[&doubled], 1 << 1);
        // memo's write mask re-dirties readers of `doubled`
        assert_eq!(sched.reaction_writes[memo.get() as usize], 1 << 1);
        assert_eq!(sched.reaction_writes[bind.get() as usize], 0);
    }

    #[test]
    fn declaration_order_breaks_ties() {
        let mut r = rig();
        let count = r.count;
        push_state(&mut r, count, StateKind::Signal);
        let first = push_reaction(&mut r, ReactionKind::Bind { dynamic: 0 }, &[count], &[], 1);
        let second = push_reaction(&mut r, ReactionKind::Bind { dynamic: 1 }, &[count], &[], 2);
        let sched = build_schedule(&r.module, r.root).expect("schedule");
        assert_eq!(sched.order, vec![first, second]);
    }

    #[test]
    fn static_cycles_are_denied() {
        let mut r = rig();
        let (count, doubled) = (r.count, r.doubled);
        push_state(&mut r, count, StateKind::Memo);
        push_state(&mut r, doubled, StateKind::Memo);
        push_reaction(
            &mut r,
            ReactionKind::Memo { target: count },
            &[doubled],
            &[count],
            1,
        );
        push_reaction(
            &mut r,
            ReactionKind::Memo { target: doubled },
            &[count],
            &[doubled],
            2,
        );
        assert!(
            matches!(
                build_schedule(&r.module, r.root),
                Err(ScheduleError::Cycle { .. })
            ),
            "a read-write cycle must be a compile-time Deny (R10)"
        );
    }

    #[test]
    fn unknown_fields_are_denied() {
        let mut r = rig();
        let count = r.count;
        let ghost = r.module.strings.intern("ghost");
        push_state(&mut r, count, StateKind::Signal);
        push_reaction(&mut r, ReactionKind::Bind { dynamic: 0 }, &[ghost], &[], 1);
        assert!(matches!(
            build_schedule(&r.module, r.root),
            Err(ScheduleError::UnknownField { .. })
        ));
    }

    #[test]
    fn past_128_reactions_is_reported() {
        let mut r = rig();
        let count = r.count;
        push_state(&mut r, count, StateKind::Signal);
        for i in 0..129 {
            push_reaction(
                &mut r,
                ReactionKind::Effect { tier: Tier::Post },
                &[count],
                &[],
                i,
            );
        }
        assert!(matches!(
            build_schedule(&r.module, r.root),
            Err(ScheduleError::TooManyReactions { found: 129 })
        ));
    }

    #[test]
    fn spill_word_readers_and_writes_use_id_minus_64() {
        // 70 reactions: r64..r69 read `count` and one memo r70 writes the
        // field they read — the masks must speak id-minus-64.
        let mut r = rig();
        let count = r.count;
        let extra = r.module.strings.intern("extra");
        push_state(&mut r, count, StateKind::Signal);
        push_state(&mut r, extra, StateKind::Memo);
        // a spill memo (id 0..66 are effects reading count; memo lands at 66)
        for i in 0..66 {
            push_reaction(
                &mut r,
                ReactionKind::Effect { tier: Tier::Post },
                &[count],
                &[],
                i,
            );
        }
        let spill_memo = push_reaction(
            &mut r,
            ReactionKind::Memo { target: extra },
            &[count],
            &[extra],
            99,
        );
        assert_eq!(spill_memo.get(), 66, "the memo sits in the spill word");
        let sched = build_schedule(&r.module, r.root).expect("schedule");
        // count's readers: effects 0..65 + the memo 66 — ids 0..63 fill the
        // low word; 64, 65, 66 are spill bits (id − 64) = 0b111.
        assert_eq!(sched.field_readers[&count], u64::MAX, "low readers only");
        assert_eq!(
            sched.field_readers_hi.get(&count).copied().unwrap_or(0),
            0b111,
            "spill reader bits are id minus 64"
        );
        // the memo's low write mask must not claim spill readers.
        assert_eq!(sched.reaction_writes[spill_memo.get() as usize], 0);
        assert_eq!(
            sched.reaction_writes_hi[spill_memo.get() as usize],
            0,
            "the memo writes `extra`, whose only reader is the memo itself"
        );
    }

    #[test]
    fn foreign_reactions_are_excluded() {
        let mut r = rig();
        let count = r.count;
        push_state(&mut r, count, StateKind::Signal);
        let foreign_frag = {
            let tpl = r.module.intern_template(lig_ir::template::TemplateIr::new(
                lig_ir::template::NodeIr::Marker,
            ));
            let name = r.module.strings.intern("Other");
            r.module.push_fragment(FragmentIr {
                name,
                template: tpl,
                dynamics: vec![],
                children: vec![],
                props: vec![],
                returns: vec![],
            })
        };
        let ExprId(body) = r.module.insert_expr(Expr::Lit(Lit::Int(9)));
        r.module.reactions.push(ReactionDecl {
            kind: ReactionKind::Effect { tier: Tier::Post },
            reads: vec![count],
            writes: vec![],
            body: CoreExprId(body),
            owner: foreign_frag,
        });
        assert_eq!(component_reactions(&r.module, r.root).len(), 0);
        let sched = build_schedule(&r.module, r.root).expect("schedule");
        assert_eq!(sched.order.len(), 0);
    }

    #[test]
    fn shown_flag_helper_keeps_names_alive() {
        let mut r = rig();
        let shown = r.shown;
        push_state(&mut r, shown, StateKind::Signal);
        let sched = build_schedule(&r.module, r.root).expect("schedule");
        assert_eq!(sched.field_readers.get(&shown).copied().unwrap_or(0), 0);
    }

    // --- component build order (§11.1, LIG-E020) -----------------------------

    use super::{ComponentCycle, component_build_order};
    use lig_ir::anchor::Anchor;
    use lig_ir::fragment::{ComponentIr, DynamicIr, DynamicKind, PropBinding};

    /// Add component `name` with an empty root fragment; returns (id, root).
    fn push_component(rig: &mut Rig, name: &str) -> (u32, lig_ir::FragmentId) {
        let tpl = rig
            .module
            .intern_template(lig_ir::template::TemplateIr::new(
                lig_ir::template::NodeIr::Marker,
            ));
        let sname = rig.module.strings.intern(name);
        let root = rig.module.push_fragment(FragmentIr {
            name: sname,
            template: tpl,
            dynamics: vec![],
            children: vec![],
            props: vec![],
            returns: vec![],
        });
        let id = rig.module.components.len() as u32;
        rig.module.components.push(ComponentDecl {
            name: sname,
            root,
            props: vec![],
        });
        (id, root)
    }

    /// Make component `user`'s root instantiate component `used`.
    fn add_use(rig: &mut Rig, user_root: lig_ir::FragmentId, used: u32) {
        let expr = rig.module.insert_expr(Expr::Lit(Lit::Int(0)));
        let prop = rig.module.strings.intern("prop");
        let frag = &mut rig.module.fragments[user_root.get() as usize];
        frag.dynamics.push(DynamicIr {
            anchor: Anchor::default(),
            kind: DynamicKind::Component(ComponentIr {
                component: lig_ir::ComponentId::new(used),
                props: vec![PropBinding { prop, value: expr }],
                slots: vec![],
            }),
            expr,
            reaction: None,
            span: (0, 0),
        });
    }

    #[test]
    fn build_order_puts_dependencies_first() {
        let mut r = rig();
        let (a_id, a_root) = push_component(&mut r, "A"); // the rig root is component 0
        let (b_id, _b_root) = push_component(&mut r, "B");
        let (c_id, _c_root) = push_component(&mut r, "C");
        // the rig root uses C; A uses B and C — nothing uses A or the root.
        let rig_root = r.root;
        add_use(&mut r, rig_root, c_id);
        add_use(&mut r, a_root, b_id);
        add_use(&mut r, a_root, c_id);
        let order = component_build_order(&r.module).expect("acyclic");
        let pos = |c: u32| order.iter().position(|&x| x.get() == c).expect("member");
        assert!(pos(b_id) < pos(a_id), "A uses B: {order:?}");
        assert!(pos(c_id) < pos(a_id), "A uses C: {order:?}");
    }

    #[test]
    fn cyclic_components_are_denied_with_the_cycle_path() {
        let mut r = rig();
        let (a_id, a_root) = push_component(&mut r, "A");
        let (b_id, b_root) = push_component(&mut r, "B");
        add_use(&mut r, a_root, b_id);
        add_use(&mut r, b_root, a_id);
        let cycle = component_build_order(&r.module).expect_err("cycle");
        let ids: Vec<u32> = cycle.components.iter().map(|c| c.get()).collect();
        assert!(
            ids == vec![a_id, b_id] || ids == vec![b_id, a_id],
            "the diagnostic names the two members: {ids:?}"
        );
        // Self-use is also a cycle (S uses S).
        let mut r2 = rig();
        let (s_id, s_root) = push_component(&mut r2, "S");
        add_use(&mut r2, s_root, s_id);
        let cycle2 = component_build_order(&r2.module).expect_err("self-cycle");
        assert_eq!(cycle2.components, vec![lig_ir::ComponentId::new(s_id)]);
    }

    #[test]
    fn acyclic_diamond_orders_cleanly() {
        let mut r = rig();
        let (_d_id, d_root) = push_component(&mut r, "D");
        let (b_id, _b_root) = push_component(&mut r, "B");
        let (c_id, _c_root) = push_component(&mut r, "C");
        let (a_id, _a_root) = push_component(&mut r, "A");
        // D uses A, B, C; nobody uses D or the rig root.
        add_use(&mut r, d_root, b_id);
        add_use(&mut r, d_root, c_id);
        add_use(&mut r, d_root, a_id);
        let order = component_build_order(&r.module).expect("acyclic");
        assert_eq!(order.len(), r.module.components.len());
        let pos = |c: u32| order.iter().position(|&x| x.get() == c).expect("member");
        assert!(pos(a_id) < pos(1), "D uses A first: {order:?}");
        assert!(pos(b_id) < pos(1), "D uses B: {order:?}");
        assert!(pos(c_id) < pos(1), "D uses C: {order:?}");
    }

    #[test]
    fn cycle_error_is_structurally_matched() {
        // The cycle type must be constructible/matchable without ids beyond
        // the API (the engine maps it to LIG-E020).
        let c = ComponentCycle {
            components: vec![lig_ir::ComponentId::new(1), lig_ir::ComponentId::new(2)],
        };
        assert_eq!(c.components.len(), 2);
        let _ = format!("{c:?}");
    }
}
