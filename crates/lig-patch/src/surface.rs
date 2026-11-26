//! The patch engine's host bridge: the hot-patch protocol driving a
//! mounted surface — the seam the component-mount report named as its
//! last untouched gap. A mounted component's event front is the
//! generated `fire_N` dispatch glue (capture the payload, mark the
//! handler reaction, the checked flush); under hot-patching, that glue
//! is exactly what a patch generation replaces — the new dylib carries a
//! new body for the same symbol, and the jump table routes the old
//! address to it. This module is the safepoint pair for that shape:
//!
//! - [`hot_dispatch`] — the mounted dispatch as a safepoint call: a
//!   table hit transmutes to the same-signature patched body, a miss
//!   calls the fat build's body (the [`crate::hot_call`] pattern, at the
//!   dispatch shape the runtime's event front uses);
//! - [`captured_dispatch_address`] / [`call_captured_dispatch`] — the
//!   never-dlclose proof on the surface: capture a generation's
//!   committed address before the next apply, call it after any number
//!   of later generations — it stays executable (§2.4);
//! - [`HotSurface`] — the mounted front whose every dispatch rides the
//!   safepoint: mount once, dispatch across patch generations, dispose
//!   through the mount seam's total teardown. Ticks and setters stay the
//!   mount seam's ([`Mounted::update`](lig_runtime::Mounted::update) via
//!   [`HotSurface::surface`]) — the bridge adds only the route.
//!
//! Real generations arrive through [`crate::PatchSession::apply`] (a
//! loaded patch dylib's symbols resolve against the fat build's
//! addresses); the route mechanics here are identical, and the
//! `lig patch-spike` gate proves the dylib path end to end. The
//! dispatch shape is the generated glue's: `fn(&mut C, &mut dyn Host,
//! &E) -> Result<(), CycleError>` — the checked event front, so a
//! non-converging schedule reports as the typed `CycleError`
//! (LIG-E401) through a patched body exactly as through the fat build.

use crate::table::current_table;
use lig_runtime::host::Host;
use lig_runtime::mount::{ComponentUnit, Mounted};
use lig_runtime::schedule::CycleError;

/// The mounted dispatch shape: one event pump step on a component state
/// against a host — the signature the generated `fire_N` glue exposes as
/// a free function (capture, mark, checked flush) and the signature a
/// patch dylib re-exports under the same symbol name.
pub type DispatchFn<C, E> = fn(&mut C, &mut dyn Host, &E) -> Result<(), CycleError>;

/// Safepoint dispatch (the surface twin of [`crate::hot_call`]): route
/// the mounted event pump through the jump table — a hit calls the
/// patched body at the same signature, a miss calls `f` itself. The
/// checked front is preserved either way: the dispatched body's
/// `Result` (a non-converging schedule is the typed
/// [`CycleError`](lig_runtime::CycleError), LIG-E401) rides out.
///
/// # Errors
/// Whatever the dispatched body (fat or patched) reports — the typed
/// [`CycleError`](lig_runtime::CycleError) for a non-converging
/// schedule.
pub fn hot_dispatch<C, E>(
    f: DispatchFn<C, E>,
    state: &mut C,
    host: &mut dyn Host,
    ev: &E,
) -> Result<(), CycleError> {
    match current_table().and_then(|t| t.lookup(f as usize as u64)) {
        Some(new_addr) if new_addr != f as usize as u64 => {
            // SAFETY: the target is a function in a loaded (never-unmapped)
            // dylib — or, for an in-process table, a live function of the
            // same signature — matched by symbol name (the protocol's
            // same-signature invariant, §2.4).
            let patched: DispatchFn<C, E> = unsafe { core::mem::transmute(new_addr) };
            patched(state, host, ev)
        }
        _ => f(state, host, ev),
    }
}

/// The committed (current-generation) address a mounted dispatch fn
/// routes to, if any — the address a stale frame inside a superseded
/// generation would return into. Capture it **before** the next apply to
/// prove the never-dlclose discipline afterwards ([`call_captured_dispatch`]).
#[must_use]
pub fn captured_dispatch_address<C, E>(f: DispatchFn<C, E>) -> Option<u64> {
    let table = current_table()?;
    table.lookup(f as usize as u64)
}

/// Call a captured (previous-generation) dispatch address directly, at
/// the mounted dispatch shape.
///
/// # Safety contract
/// The address must come from [`captured_dispatch_address`] — a
/// leaked-table entry the protocol committed and deliberately never freed
/// (never-dlclose, §2.4), with the same signature as the original symbol.
///
/// # Errors
/// Whatever the captured body reports (the typed
/// [`CycleError`](lig_runtime::CycleError) for a non-converging schedule).
pub fn call_captured_dispatch<C, E>(
    addr: u64,
    state: &mut C,
    host: &mut dyn Host,
    ev: &E,
) -> Result<(), CycleError> {
    // SAFETY: `captured_dispatch_address` returns a leaked-table entry
    // whose target lives in a dylib that is never unmapped, and the
    // protocol's same-signature invariant holds for symbol-matched remaps.
    let f: DispatchFn<C, E> = unsafe { core::mem::transmute(addr) };
    f(state, host, ev)
}

/// A mounted component whose every event dispatch is a safepoint: the
/// mount seam's lifecycle front with the patch protocol's route in front
/// of the event pump — mount once, dispatch across patch generations
/// (the newest committed table routes every call), dispose through the
/// same total teardown.
///
/// The state stays caller-owned inside the [`Mounted`] handle (reached
/// through [`HotSurface::surface`]); the bridge adds only the route —
/// ticks and setter batches ride the mount seam's checked front.
pub struct HotSurface<C: ComponentUnit> {
    mounted: Mounted<C>,
}

impl<C: ComponentUnit> HotSurface<C> {
    /// Mount a component under the hot surface: run `build` against the
    /// host (the build's own init flush included) and wrap the handle.
    pub fn mount(host: &mut dyn Host, build: impl FnOnce(&mut dyn Host) -> C) -> Self {
        Self {
            mounted: Mounted::mount(host, build),
        }
    }

    /// The mount seam's handle — setters, the save-tick
    /// ([`Mounted::update`](lig_runtime::Mounted::update)), the bare
    /// checked flush; everything but the event route.
    pub fn surface(&mut self) -> &mut Mounted<C> {
        &mut self.mounted
    }

    /// One event dispatch through the safepoint: the newest committed
    /// table routes `f` (a hit calls the patched body at the same
    /// signature), and the dispatched body's checked result rides out —
    /// a non-converging schedule is the typed
    /// [`CycleError`](lig_runtime::CycleError) (LIG-E401), patched body
    /// or not.
    ///
    /// # Errors
    /// Whatever the dispatched body (fat or patched) reports.
    pub fn dispatch<E>(
        &mut self,
        f: DispatchFn<C, E>,
        host: &mut dyn Host,
        ev: &E,
    ) -> Result<(), CycleError> {
        hot_dispatch(f, self.mounted.state(), host, ev)
    }

    /// Total disposal — the mount seam's recursive LIFO teardown, one
    /// `destroy` commit per node the build created, whatever generations
    /// dispatched in between.
    pub fn dispose(self, host: &mut dyn Host) -> C {
        self.mounted.dispose(host)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DispatchFn, HotSurface, call_captured_dispatch, captured_dispatch_address, hot_dispatch,
    };
    use crate::{JumpTable, commit_table};
    use lig_runtime::host::{EventKind, HandlerId, HookId, Host, NodeId, TemplateDesc};
    use lig_runtime::schedule::{CycleError, Reactions, Schedule};
    use lig_runtime::{ComponentUnit, DirtySet, EvData, Fragment};
    use std::collections::BTreeMap;

    // ------------------------------------------------------------------
    // The mounted-surface test instrument: a hand-written counter unit of
    // the generated shape (state + root fragment + build ending in the
    // init flush) and a recording host — the commit families asserted on.
    // ------------------------------------------------------------------

    /// The counter-shaped template: div > (button, span+marker) with one
    /// text hole (the mount seam's test template).
    static TPL: &[lig_runtime::TplNode] = &[lig_runtime::TplNode::Element {
        tag: "div",
        attrs: &[],
        style: None,
        children: &[
            lig_runtime::TplNode::Element {
                tag: "button",
                attrs: &[],
                style: None,
                children: &[lig_runtime::TplNode::Text { value: "Add" }],
            },
            lig_runtime::TplNode::Element {
                tag: "span",
                attrs: &[],
                style: None,
                children: &[lig_runtime::TplNode::Marker],
            },
        ],
    }];

    static HOLE_ANCHORS: &[&[lig_runtime::AnchorStep]] = &[
        &[lig_runtime::AnchorStep::Down(0)],
        &[
            lig_runtime::AnchorStep::Down(1),
            lig_runtime::AnchorStep::MarkerDown(0),
        ],
    ];

    static BRANCH_ANCHORS: &[&[lig_runtime::AnchorStep]] = &[];

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
            lig_runtime::flush(&mut s, h);
            s
        }

        /// The generated setter shape: write the field, mark the bits.
        fn set_count(&mut self, v: i64) {
            self.count = v;
            self.frag.mark_mask(1 << BIND);
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
                let text = lig_runtime::format_i64(&mut buf, self.count);
                self.frag.set_text(host, 0, text);
            }
        }
    }

    impl lig_runtime::ComponentUnit for Mini {
        fn fragment(&mut self) -> &mut Fragment {
            &mut self.frag
        }
    }

    /// The commit stream a test asserts on.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Commit {
        Create(NodeId),
        Text(NodeId, String),
        Destroy(NodeId),
    }

    /// The recording host: the surface tests' instrument.
    #[derive(Debug, Default)]
    struct RecHost {
        commits: Vec<Commit>,
    }

    impl RecHost {
        fn created(&self) -> usize {
            self.commits
                .iter()
                .filter(|c| matches!(c, Commit::Create(..)))
                .count()
        }

        fn destroyed(&self) -> usize {
            self.commits
                .iter()
                .filter(|c| matches!(c, Commit::Destroy(..)))
                .count()
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

        fn set_attr(&mut self, _n: NodeId, _k: &str, _v: Option<lig_runtime::AttrVal<'_>>) {}
        fn set_class(&mut self, _n: NodeId, _c: Option<&str>) {}
        fn set_style(&mut self, _n: NodeId, _s: lig_runtime::StyleConstId) {}
        fn set_style_dyn(&mut self, _n: NodeId, _p: &[(u32, lig_runtime::PropVal<'_>)]) {}
        fn set_html(&mut self, _n: NodeId, _h: &str) {}
        fn insert(&mut self, _p: NodeId, _c: NodeId, _i: u32) {}
        fn remove(&mut self, _p: NodeId, _c: NodeId) {}
        fn move_(&mut self, _p: NodeId, _c: NodeId, _i: u32) {}
        fn attach(&mut self, _n: NodeId, _e: EventKind, _h: HandlerId, _d: bool) {}
        fn detach(&mut self, _n: NodeId, _e: EventKind, _h: HandlerId) {}
        fn on_mount(&mut self, _n: NodeId, _h: HookId) {}
        fn on_unmount(&mut self, _n: NodeId, _h: HookId) {}
    }

    // ------------------------------------------------------------------
    // The mounted dispatch shape (the generated `fire_N` glue as a free
    // fn): capture, mark, checked flush. v0 is the fat build's body; v1/v2
    // are the "patched" bodies — each generation's patch carries its own
    // behavior (here: a bigger increment), observable in the rendered
    // text. These are the only fns the route test maps; every other test
    // dispatches `plain_dispatch`, which no table ever maps, so a parallel
    // test's table commit cannot reroute it.
    // ------------------------------------------------------------------

    /// The fat build's dispatch body (v0): mark the bind, checked flush.
    fn dispatch_v0(state: &mut Mini, host: &mut dyn Host, _ev: &EvData) -> Result<(), CycleError> {
        state.set_count(state.count + 1);
        lig_runtime::flush_checked(state, host)
    }

    /// The generation-1 patch body: the increment doubles.
    fn dispatch_v1(state: &mut Mini, host: &mut dyn Host, _ev: &EvData) -> Result<(), CycleError> {
        state.set_count(state.count + 2);
        lig_runtime::flush_checked(state, host)
    }

    /// The generation-2 patch body: the increment triples.
    fn dispatch_v2(state: &mut Mini, host: &mut dyn Host, _ev: &EvData) -> Result<(), CycleError> {
        state.set_count(state.count + 3);
        lig_runtime::flush_checked(state, host)
    }

    /// A dispatch fn NO table ever maps — the parallel-safe miss-path probe.
    fn plain_dispatch(
        state: &mut Mini,
        host: &mut dyn Host,
        _ev: &EvData,
    ) -> Result<(), CycleError> {
        state.set_count(state.count + 7);
        lig_runtime::flush_checked(state, host)
    }

    /// Table commits swap a process-global; only this test commits, and it
    /// owns the whole miss → gen1 → gen2 → stale sequence in one thread.
    #[test]
    fn the_bridge_routes_dispatch_through_the_jump_table() {
        let mut host = RecHost::default();
        let ev = EvData::new();

        // 1. Miss path (no table yet): the safepoint calls the fat body.
        let mut unit = Mini::build(&mut host);
        hot_dispatch(dispatch_v0, &mut unit, &mut host, &ev).expect("converges");
        assert_eq!(unit.count, 1, "the fat body ran: +1");
        assert_eq!(host.last_text(), Some("1"), "the bind re-rendered");

        // 2. Generation 1: commit old→new around the real fn addresses.
        let old = dispatch_v0 as DispatchFn<Mini, EvData> as usize as u64;
        let new = dispatch_v1 as DispatchFn<Mini, EvData> as usize as u64;
        let mut map = BTreeMap::new();
        map.insert(old, new);
        commit_table(JumpTable::new(map));
        hot_dispatch(dispatch_v0, &mut unit, &mut host, &ev).expect("converges");
        assert_eq!(unit.count, 3, "the routed body ran: +2 (generation 1)");
        assert_eq!(host.last_text(), Some("3"));

        // 3. Capture the generation-1 committed address — what a stale
        //    frame inside generation 1 would return into.
        let gen1_addr = captured_dispatch_address(dispatch_v0).expect("generation 1 table entry");
        assert_ne!(
            gen1_addr, old,
            "the capture is the patch, not the fat build"
        );

        // 4. Generation 2: the newest table routes to the v2 body.
        let new2 = dispatch_v2 as DispatchFn<Mini, EvData> as usize as u64;
        let mut map2 = BTreeMap::new();
        map2.insert(old, new2);
        commit_table(JumpTable::new(map2));
        hot_dispatch(dispatch_v0, &mut unit, &mut host, &ev).expect("converges");
        assert_eq!(unit.count, 6, "the routed body ran: +3 (generation 2)");

        // 5. Never-dlclose on the surface: the generation-1 address is
        //    still mapped and still runs the generation-1 body.
        call_captured_dispatch(gen1_addr, &mut unit, &mut host, &ev).expect("converges");
        assert_eq!(
            unit.count, 8,
            "the stale address still ran: +2 (generation 1)"
        );

        // 6. The same safepoint through the mounted front: HotSurface drives
        //    the identical route, and the teardown totals afterwards — both
        //    the routed unit and the surface's unit leave the host balanced.
        let mut surface = HotSurface::mount(&mut host, Mini::build);
        surface
            .dispatch(dispatch_v0, &mut host, &ev)
            .expect("converges");
        let created_total = host.created();
        unit.fragment().teardown(&mut host);
        let surface_state = surface.dispose(&mut host);
        assert_eq!(
            host.destroyed(),
            created_total,
            "dispose totals after patched dispatches"
        );
        assert_eq!(
            surface_state.count, 3,
            "the surface front routed through the newest table (+3)"
        );
        drop(ev);
    }

    /// Steady state on a foreign (or no) table: a fn the table does not map
    /// hits the fat body through the mounted front.
    #[test]
    fn unmapped_dispatch_hits_the_fat_body_through_the_surface() {
        let mut host = RecHost::default();
        let mut surface = HotSurface::mount(&mut host, Mini::build);
        surface
            .dispatch(plain_dispatch, &mut host, &EvData::new())
            .expect("converges");
        assert_eq!(surface.surface().state().count, 7, "the fat body ran: +7");
        surface
            .dispatch(plain_dispatch, &mut host, &EvData::new())
            .expect("converges");
        assert_eq!(
            surface.surface().state().count,
            14,
            "+7 again: still the fat body"
        );
        let created = host.created();
        surface.dispose(&mut host);
        assert_eq!(host.destroyed(), created, "dispose totals");
    }
}
