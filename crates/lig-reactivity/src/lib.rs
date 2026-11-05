//! Reactivity compilation (D-3, §7): from per-fragment reactions to the
//! **const topological schedule** — the graph replaced by two const arrays.
//!
//! The flush loop lives in `lig-runtime`; this crate computes the program
//! constants it consumes: reaction order (topological, tie-broken by
//! declaration order, §7.2), per-field reader masks (what setters mark), and
//! per-reaction write masks (what `flush` re-enters).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use lig_core::id::{FragmentId, ReactionId, StrId};
use lig_ir::ViewModule;

use std::collections::{BTreeMap, BTreeSet};

/// A static schedule error: the compile-time cycle Deny (R10) or capacity
/// overflow past the 64-reaction fast path (§7.2 spill note).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    /// Two memos (transitively) write/read each other's fields.
    Cycle {
        /// Reaction ids forming the cycle, in walk order.
        reactions: Vec<ReactionId>,
    },
    /// More than 64 reactions in one component — mask spill is an M1 item.
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

/// The component's reactive program constants (§7.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentSchedule {
    /// Reaction ids in topological order (memo before readers; ties by
    /// declaration order).
    pub order: Vec<ReactionId>,
    /// Per state field (in `field_order`): bitmask of reactions that read it.
    /// Setters OR this into the dirty set (§9.2 `READS_TO_DIRTY`).
    pub field_readers: BTreeMap<StrId, u64>,
    /// Per reaction (module order): bitmask of reactions to re-dirty after it
    /// runs (`WRITES[r]` in the flush loop, §7.2).
    pub reaction_writes: Vec<u64>,
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
/// past 64, `UnknownField` when a reaction reads or writes an undeclared field.
pub fn build_schedule(
    module: &ViewModule,
    root: FragmentId,
) -> Result<ComponentSchedule, ScheduleError> {
    let members = component_reactions(module, root);
    if members.len() > 64 {
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

    // field_readers[field] = bitmask of reactions reading it.
    // Bits are REACTION IDS — the flush loop scans `pending & (1 << r)` with r
    // a reaction id (§7.2), and masks must speak the same language.
    let mut field_readers: BTreeMap<StrId, u64> = BTreeMap::new();
    for &rid in &members {
        let r = &module.reactions[rid.get() as usize];
        for field in &r.reads {
            *field_readers.entry(*field).or_insert(0) |= 1u64 << rid.get();
        }
    }

    // WRITES[r] = OR of field_readers over the fields r writes (reactions to
    // re-dirty when r runs, §7.2). Self-dirtying is skipped: a reaction that
    // reads its own written field is a cycle the memo check catches.
    let mut reaction_writes = vec![0u64; module.reactions.len()];
    for &rid in &members {
        let r = &module.reactions[rid.get() as usize];
        let mut mask = 0u64;
        for field in &r.writes {
            mask |= field_readers.get(field).copied().unwrap_or(0);
        }
        reaction_writes[rid.get() as usize] = mask & !(1u64 << rid.get());
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
        reaction_writes,
        field_order,
    })
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
    fn past_64_reactions_is_reported() {
        let mut r = rig();
        let count = r.count;
        push_state(&mut r, count, StateKind::Signal);
        for i in 0..65 {
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
            Err(ScheduleError::TooManyReactions { found: 65 })
        ));
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
}
