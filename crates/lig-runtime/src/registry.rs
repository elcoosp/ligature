//! The `dyn` component registry: the shipped container the mount seam's
//! report named as its embedding follow-up — a host-side store of mounted
//! components behind the object-safe [`ComponentUnit`] trait, with the
//! mount-seam lifecycle (`mount` → `update`/`flush` → `dispose`) driven
//! through `&mut dyn ComponentUnit` instead of a concrete state type.
//!
//! [`Mounted`](crate::mount::Mounted) is the per-component front: one
//! handle, generic over the state. A host that mounts many components —
//! a scene of counters, a grid of tiles — needs the container: an id-keyed
//! registry that stores every mounted unit boxed behind the trait and
//! drives the same checked lifecycle per id (and across all of them) —
//! the object-safety the mount seam's test proved possible, shipped as
//! the container it promised.
//!
//! Shape: ids are `Copy` handles (`CompId`, the `NodeId`/`HandlerId`
//! convention), never reused after disposal; every tick rides the checked
//! front (`flush_checked`, the typed [`CycleError`], LIG-E401 — never a
//! panic); disposal is the mount seam's total teardown per unit; and the
//! registry is synchronous on the driving thread, the same seam discipline
//! as `&mut dyn Host` (§10.1) and `&mut dyn ArtifactSink` (§9.5).

use alloc::boxed::Box;
use alloc::collections::BTreeMap;

use crate::host::Host;
use crate::mount::ComponentUnit;
use crate::schedule::{CycleError, flush_checked};

/// A mounted-component handle: `Copy`, unique per registry, never reused
/// after disposal (the `NodeId` discipline — a stale id can never name a
/// newer component's state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CompId(pub u32);

impl CompId {
    /// Raw index.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl core::fmt::Display for CompId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "c{}", self.0)
    }
}

/// The host-side store of mounted components: every unit boxed behind
/// [`ComponentUnit`], keyed by [`CompId`], the mount seam's lifecycle
/// available per id and across the whole registry.
///
/// The container is the embedding milestone the mount seam's report named:
/// `Mounted` stays the per-component front, and hosts that keep a scene of
/// components store them here — mounted through the same build contract
/// (the build runs its own init flush), driven through the checked front,
/// disposed through the same total teardown.
#[derive(Default)]
pub struct ComponentRegistry {
    next_id: u32,
    units: BTreeMap<u32, Box<dyn ComponentUnit>>,
}

impl core::fmt::Debug for ComponentRegistry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // The trait objects have no Debug surface; the count is the state.
        f.debug_struct("ComponentRegistry")
            .field("mounted", &self.units.len())
            .finish_non_exhaustive()
    }
}

impl ComponentRegistry {
    /// An empty registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            next_id: 0,
            units: BTreeMap::new(),
        }
    }

    /// Mount a component into the registry: run `build` against the host
    /// (the build performs its own init flush, §9.2), box the state behind
    /// the trait, and return its fresh [`CompId`].
    pub fn mount<B: ComponentUnit + 'static>(
        &mut self,
        host: &mut dyn Host,
        build: impl FnOnce(&mut dyn Host) -> B,
    ) -> CompId {
        let id = CompId(self.next_id);
        self.next_id += 1;
        let comp: Box<dyn ComponentUnit> = Box::new(build(host));
        self.units.insert(id.0, comp);
        id
    }

    /// The mounted component behind the trait object — setters reach it
    /// through their own path (the consumer downcasts nothing; generated
    /// state is driven via [`ComponentRegistry::update`]), reactions and
    /// the fragment stay trait-reachable.
    #[must_use]
    pub fn get(&mut self, id: CompId) -> Option<&mut dyn ComponentUnit> {
        let unit: &mut Box<dyn ComponentUnit> = self.units.get_mut(&id.0)?;
        Some(&mut **unit)
    }

    /// One save-tick on the named component — apply the setter batch, then
    /// the checked flush — through the trait object. `None` names an
    /// unknown id: nothing ran, nothing mutated (the stale-id discipline).
    ///
    /// The checked front: a non-converging schedule surfaces as the typed
    /// [`CycleError`] (LIG-E401), never a panic; the batch has been applied
    /// — only the re-render is missing (the `Mounted::update` contract).
    pub fn update(
        &mut self,
        id: CompId,
        host: &mut dyn Host,
        f: impl FnOnce(&mut dyn ComponentUnit),
    ) -> Option<Result<(), CycleError>> {
        let unit = self.get(id)?;
        f(unit);
        Some(flush_checked(unit, host))
    }

    /// Flush the named component's pending reactions (the bare tick).
    /// `None` names an unknown id.
    ///
    /// # Errors
    /// [`CycleError`] when that component's schedule does not converge
    /// within [`MAX_PASSES`](crate::schedule::MAX_PASSES) passes.
    pub fn flush(&mut self, id: CompId, host: &mut dyn Host) -> Option<Result<(), CycleError>> {
        let unit = self.get(id)?;
        Some(flush_checked(unit, host))
    }

    /// Flush every mounted component in id order — the scene tick. Fail
    /// closed on the first non-converging schedule: the typed [`CycleError`]
    /// rides out, every earlier component stays drained, and the failing
    /// component keeps its dirty set (the caller can tick it alone via
    /// [`ComponentRegistry::flush`]).
    ///
    /// # Errors
    /// [`CycleError`] when any mounted component's schedule does not
    /// converge within [`MAX_PASSES`](crate::schedule::MAX_PASSES) passes.
    pub fn flush_all(&mut self, host: &mut dyn Host) -> Result<(), CycleError> {
        for unit in self.units.values_mut() {
            flush_checked(&mut **unit, host)?;
        }
        Ok(())
    }

    /// Dispose the named component: the mount seam's total teardown (every
    /// node the build created reaches the host as a `destroy` commit,
    /// recursively), then removal from the registry. `false` names an
    /// unknown id: nothing ran.
    pub fn dispose(&mut self, id: CompId, host: &mut dyn Host) -> bool {
        match self.units.remove(&id.0) {
            Some(mut unit) => {
                unit.fragment().teardown(host);
                true
            }
            None => false,
        }
    }

    /// Dispose every mounted component in id order and empty the registry —
    /// the scene teardown. Total by the fragment contract: every node every
    /// mounted build created is destroyed exactly once.
    pub fn dispose_all(&mut self, host: &mut dyn Host) {
        let units = core::mem::take(&mut self.units);
        for (_, mut unit) in units {
            unit.fragment().teardown(host);
        }
    }

    /// How many components are mounted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.units.len()
    }

    /// Whether the registry mounts nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{CompId, ComponentRegistry};
    use crate::dirty::DirtySet;
    use crate::fragment::Fragment;
    use crate::host::{EventKind, HandlerId, HookId, Host, NodeId, TemplateDesc};
    use crate::mount::ComponentUnit;
    use crate::schedule::{Reactions, Schedule};
    use crate::template::{AnchorStep, TplNode};
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;

    /// The counter-shaped template the mount seam's tests drive.
    static TPL: &[TplNode] = &[TplNode::Element {
        tag: "div",
        attrs: &[],
        style: None,
        children: &[
            TplNode::Element {
                tag: "button",
                attrs: &[],
                style: None,
                children: &[TplNode::Text { value: "Add" }],
            },
            TplNode::Element {
                tag: "span",
                attrs: &[],
                style: None,
                children: &[TplNode::Marker],
            },
        ],
    }];

    static HOLE_ANCHORS: &[&[AnchorStep]] = &[
        &[AnchorStep::Down(0)],
        &[AnchorStep::Down(1), AnchorStep::MarkerDown(0)],
    ];

    static BRANCH_ANCHORS: &[&[AnchorStep]] = &[];

    /// The commit stream a test asserts on — creates, text writes, and
    /// destroys, the lifecycle commit families.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Commit {
        Create(NodeId),
        Text(NodeId, String),
        Destroy(NodeId),
    }

    /// The recording host: the registry tests' instrument.
    #[derive(Debug, Default)]
    struct RecHost {
        commits: Vec<Commit>,
    }

    impl RecHost {
        fn text_commits(&self) -> usize {
            self.commits
                .iter()
                .filter(|c| matches!(c, Commit::Text(..)))
                .count()
        }

        fn created(&self) -> Vec<NodeId> {
            self.commits
                .iter()
                .filter_map(|c| match c {
                    Commit::Create(n) => Some(*n),
                    _ => None,
                })
                .collect()
        }

        fn destroyed(&self) -> Vec<NodeId> {
            self.commits
                .iter()
                .filter_map(|c| match c {
                    Commit::Destroy(n) => Some(*n),
                    _ => None,
                })
                .collect()
        }

        fn last_text(&self) -> Option<&str> {
            self.commits.iter().rev().find_map(|c| match c {
                Commit::Text(_, t) => Some(t.as_str()),
                _ => None,
            })
        }
    }

    impl Host for RecHost {
        fn create(&mut self, node: NodeId, _desc: TemplateDesc<'_>) {
            self.commits.push(Commit::Create(node));
        }

        fn destroy(&mut self, node: NodeId) {
            self.commits.push(Commit::Destroy(node));
        }

        fn set_text(&mut self, node: NodeId, text: &str) {
            self.commits.push(Commit::Text(node, text.to_string()));
        }

        fn set_attr(&mut self, _n: NodeId, _k: &str, _v: Option<crate::host::AttrVal<'_>>) {}
        fn set_class(&mut self, _n: NodeId, _c: Option<&str>) {}
        fn set_style(&mut self, _n: NodeId, _s: crate::host::StyleConstId) {}
        fn set_style_dyn(&mut self, _n: NodeId, _p: &[(u32, crate::host::PropVal<'_>)]) {}
        fn set_html(&mut self, _n: NodeId, _h: &str) {}
        fn insert(&mut self, _p: NodeId, _c: NodeId, _i: u32) {}
        fn remove(&mut self, _p: NodeId, _c: NodeId) {}
        fn move_(&mut self, _p: NodeId, _c: NodeId, _i: u32) {}
        fn attach(&mut self, _n: NodeId, _e: EventKind, _h: HandlerId, _d: bool) {}
        fn detach(&mut self, _n: NodeId, _e: EventKind, _h: HandlerId) {}
        fn on_mount(&mut self, _n: NodeId, _h: HookId) {}
        fn on_unmount(&mut self, _n: NodeId, _h: HookId) {}
    }

    /// One reaction: the text bind, hole 0 renders the count.
    const BIND: u8 = 0;

    static SCHEDULE: Schedule = Schedule {
        order: &[BIND],
        writes: &[0],
        writes_hi: &[],
    };

    /// The hand-written counter unit: the surface generated code emits.
    struct Mini {
        count: i64,
        frag: Fragment,
    }

    impl Mini {
        fn build(h: &mut dyn Host) -> Self {
            let mut s = Self {
                count: 0,
                frag: Fragment::new(h, TPL, HOLE_ANCHORS, BRANCH_ANCHORS),
            };
            s.frag.mark_mask(1 << BIND);
            crate::schedule::flush(&mut s, h);
            s
        }
    }

    impl Reactions for Mini {
        fn schedule(&self) -> &'static Schedule {
            &SCHEDULE
        }

        fn dirty(&mut self) -> &mut DirtySet {
            &mut self.frag.dirty
        }

        fn run(&mut self, host: &mut dyn Host, r: u8) {
            if r == BIND {
                let mut buf = [0u8; 20];
                let text = crate::fmt::format_i64(&mut buf, self.count);
                self.frag.set_text(host, 0, text);
            }
        }
    }

    impl ComponentUnit for Mini {
        fn fragment(&mut self) -> &mut Fragment {
            &mut self.frag
        }
    }

    /// The ping-pong unit: the typed-cycle test's instrument (the mount
    /// seam's `Cyclic` shape — two reactions that re-mark each other).
    struct Cyclic {
        frag: Fragment,
    }

    static CYCLE_SCHEDULE: Schedule = Schedule {
        order: &[0, 1],
        writes: &[1 << 1, 1 << 0],
        writes_hi: &[],
    };

    impl Reactions for Cyclic {
        fn schedule(&self) -> &'static Schedule {
            &CYCLE_SCHEDULE
        }

        fn dirty(&mut self) -> &mut DirtySet {
            &mut self.frag.dirty
        }

        fn run(&mut self, _h: &mut dyn Host, _r: u8) {}
    }

    impl ComponentUnit for Cyclic {
        fn fragment(&mut self) -> &mut Fragment {
            &mut self.frag
        }
    }

    #[test]
    fn mount_stores_components_behind_the_trait_and_returns_fresh_ids() {
        // The mount seam's report names this container as its embedding
        // follow-up: "A `dyn` component registry (host stores mounted
        // components behind `&mut dyn ComponentUnit`) has the trait ready
        // but no shipped container". The acceptance: mounted units live in
        // the registry behind the trait object, and each mount hands back
        // a fresh id that drives the lifecycle through `&mut dyn
        // ComponentUnit`.
        let mut h = RecHost::default();
        let mut reg = ComponentRegistry::new();
        let a = reg.mount(&mut h, Mini::build);
        let b = reg.mount(&mut h, Mini::build);
        assert_ne!(a, b, "every mount gets a fresh id");
        assert_eq!(reg.len(), 2, "both components mounted");
        // The store is behind the trait: the tick runs through
        // `&mut dyn ComponentUnit`, not through any concrete type.
        let first: &mut dyn ComponentUnit = reg.get(a).expect("mounted");
        let ok = crate::schedule::flush_checked(first, &mut h);
        ok.expect("the counter converges through the trait object");
        assert!(h.destroyed().is_empty(), "mounting destroys nothing");
    }

    #[test]
    fn update_marks_through_the_trait_then_flushes_the_checked_front() {
        let mut h = RecHost::default();
        let mut reg = ComponentRegistry::new();
        let id = reg.mount(&mut h, Mini::build);
        // One call through the registry: the mark batch on the trait
        // object (the fire_N glue shape: capture, mark, checked flush),
        // then the checked flush that drains it. Setters are inherent on
        // the concrete state (the `Mounted` front); what the trait object
        // drives is the reaction mark + the checked tick.
        let before = h.text_commits();
        let ok: Result<(), crate::schedule::CycleError> = reg
            .update(id, &mut h, |s: &mut dyn ComponentUnit| {
                s.fragment().mark_mask(1 << BIND);
            })
            .expect("the id is mounted");
        ok.expect("the counter converges");
        assert_eq!(
            h.text_commits(),
            before + 1,
            "the bind re-ran through the checked front"
        );
        assert!(
            h.last_text().is_some(),
            "the bind rendered through the tick"
        );
    }

    #[test]
    fn flush_after_convergence_is_a_no_op() {
        let mut h = RecHost::default();
        let mut reg = ComponentRegistry::new();
        let id = reg.mount(&mut h, Mini::build);
        reg.update(id, &mut h, |s: &mut dyn ComponentUnit| {
            s.fragment().mark_mask(1 << BIND);
        })
        .expect("mounted")
        .expect("converges");
        let settled = h.commits.len();
        reg.flush(id, &mut h).expect("mounted").expect("converges");
        reg.flush(id, &mut h).expect("mounted").expect("converges");
        assert_eq!(
            h.commits.len(),
            settled,
            "a converged schedule emits no commits"
        );
    }

    #[test]
    fn unknown_ids_report_none_and_mutate_nothing() {
        let mut h = RecHost::default();
        let mut reg = ComponentRegistry::new();
        let id = reg.mount(&mut h, Mini::build);
        let stale = CompId(id.get() + 77);
        assert!(reg.get(stale).is_none(), "unknown id: None");
        assert!(
            reg.update(stale, &mut h, |_| {}).is_none(),
            "unknown id: the tick never ran"
        );
        assert!(!reg.dispose(stale, &mut h), "unknown id: nothing disposed");
        assert_eq!(reg.len(), 1, "the mounted component is untouched");
        assert!(h.destroyed().is_empty(), "no teardown for a stale id");
    }

    #[test]
    fn flush_all_drains_every_mounted_component_in_id_order() {
        let mut h = RecHost::default();
        let mut reg = ComponentRegistry::new();
        let a = reg.mount(&mut h, Mini::build);
        let b = reg.mount(&mut h, Mini::build);
        // Mark both via the registry updates (each drains itself), then the
        // scene tick: every mounted component's schedule drains in one call
        // (one re-render per marked unit).
        let before = h.text_commits();
        for id in [a, b] {
            reg.update(id, &mut h, |s: &mut dyn ComponentUnit| {
                s.fragment().mark_mask(1 << BIND);
            })
            .expect("mounted")
            .expect("converges");
        }
        assert_eq!(
            h.text_commits(),
            before + 2,
            "both registry updates drained their marks"
        );
        reg.flush_all(&mut h).expect("both counters converge");
        assert_eq!(
            h.text_commits(),
            before + 2,
            "a converged scene tick emits no commits"
        );
        // A fresh mark on each: flush_all re-renders both in one call.
        reg.get(a).expect("mounted").fragment().mark_mask(1 << BIND);
        reg.get(b).expect("mounted").fragment().mark_mask(1 << BIND);
        reg.flush_all(&mut h).expect("both counters converge");
        assert_eq!(
            h.text_commits(),
            before + 4,
            "the scene tick drained both components"
        );
    }

    #[test]
    fn flush_all_surfaces_the_first_cycle_as_a_typed_error() {
        let mut h = RecHost::default();
        let mut reg = ComponentRegistry::new();
        let cyclic = reg.mount(&mut h, |host| Cyclic {
            frag: Fragment::new(host, TPL, HOLE_ANCHORS, BRANCH_ANCHORS),
        });
        let counter = reg.mount(&mut h, Mini::build);
        reg.get(cyclic).expect("mounted").fragment().mark_mask(1);
        // Fail closed: the ping-pong unit's schedule surfaces the typed
        // LIG-E401 error from the scene tick — never a panic.
        let err: crate::schedule::CycleError = reg
            .flush_all(&mut h)
            .expect_err("the ping-pong schedule never converges");
        assert!(
            alloc::format!("{err}").contains("LIG-E401"),
            "the catalog code rides the error: {err}"
        );
        // The registry stays usable: the healthy component still ticks.
        reg.update(counter, &mut h, |s: &mut dyn ComponentUnit| {
            s.fragment().mark_mask(1 << BIND);
        })
        .expect("mounted")
        .expect("the counter converges");
        assert!(h.last_text().is_some(), "the healthy unit rendered");
    }

    #[test]
    fn dispose_tears_down_the_named_component_exactly_once() {
        let mut h = RecHost::default();
        let mut reg = ComponentRegistry::new();
        let a = reg.mount(&mut h, Mini::build);
        let a_created = h.created().len();
        let b = reg.mount(&mut h, Mini::build);
        assert!(reg.dispose(a, &mut h), "the mounted id disposes");
        let destroyed = h.destroyed();
        assert_eq!(
            destroyed.len(),
            a_created,
            "total disposal: every node the first build created destroyed"
        );
        let mut seen = Vec::new();
        for n in &destroyed {
            assert!(!seen.contains(n), "destroyed exactly once: {n}");
            seen.push(*n);
        }
        assert_eq!(reg.len(), 1, "only the second component remains");
        // The survivor still drives; the disposed id is stale.
        reg.update(b, &mut h, |s: &mut dyn ComponentUnit| {
            s.fragment().mark_mask(1 << BIND);
        })
        .expect("mounted")
        .expect("converges");
        assert!(h.last_text().is_some(), "the survivor rendered");
    }

    #[test]
    fn ids_are_never_reused_after_disposal() {
        let mut h = RecHost::default();
        let mut reg = ComponentRegistry::new();
        let a = reg.mount(&mut h, Mini::build);
        assert!(reg.dispose(a, &mut h));
        let again = reg.mount(&mut h, Mini::build);
        assert_ne!(a, again, "a disposed id is never handed out again");
    }

    #[test]
    fn dispose_all_totals_every_mounted_component() {
        let mut h = RecHost::default();
        let mut reg = ComponentRegistry::new();
        reg.mount(&mut h, Mini::build);
        reg.mount(&mut h, Mini::build);
        let created = h.created().len();
        reg.dispose_all(&mut h);
        assert!(reg.is_empty(), "the registry empties");
        let destroyed = h.destroyed();
        assert_eq!(
            destroyed.len(),
            created,
            "scene teardown: every created node destroyed exactly once"
        );
    }
}
