//! The component-mount seam (M20): the runtime half's one-call front — the
//! mirror of the compile half's session seams (`Session::compile_with`,
//! `Session::emit_to`). After the compiler emits a unit, the consumer drives
//! the generated state against a [`Host`](crate::host::Host); before this
//! seam that meant assembling the lifecycle by hand — call the generated
//! `build`, reach for the free [`flush`](crate::schedule::flush) with the
//! right argument order after every setter batch, and discover the total
//! teardown path (`Fragment::teardown`) on your own. This module makes the
//! lifecycle one surface:
//!
//! - implement [`ComponentUnit`] once per component — the `Reactions`
//!   supertrait is what the flush loop already speaks, plus one accessor
//!   naming the root [`Fragment`] (the teardown surface);
//! - [`Mounted::mount`] pairs the generated `build` with the handle;
//! - [`Mounted::update`] is the save-tick — a setter batch, then the
//!   const-schedule flush, in one call;
//! - [`Mounted::dispose`] is the total LIFO teardown — every node the build
//!   created reaches the host as a `destroy` commit, branches and list items
//!   recursively — and the state comes back for reuse.
//!
//! Shape: [`ComponentUnit`] is object-safe (`&mut dyn ComponentUnit` works
//! for host registries that store mounted components behind the trait), the
//! state stays caller-owned inside [`Mounted`], and every call is
//! synchronous on the driving thread — the same seam discipline as
//! `&mut dyn Host` (§10.1) and `&mut dyn ArtifactSink` (§9.5). The handle
//! never touches the host outside the commits the pipeline itself makes.

use crate::fragment::Fragment;
use crate::host::Host;
use crate::schedule::{CycleError, Reactions, flush_checked};

/// The component-unit seam: the one trait a consumer implements to make a
/// generated unit (or a hand-written unit of the same shape) drivable
/// through the [`Mounted`] lifecycle front.
///
/// The [`Reactions`] supertrait is the contract generated code already
/// implements (§7.2: the const schedule, the dirty bits, the monomorphic
/// `run` dispatch) — this trait adds only the teardown surface, so the
/// implementation is one accessor:
///
/// ```ignore
/// impl ComponentUnit for Counter {
///     fn fragment(&mut self) -> &mut Fragment {
///         &mut self.frag
///     }
/// }
/// ```
///
/// Contract: `fragment` must return the component's root fragment — the one
/// the build created — every call; it is the disposal root. Like `Reactions`
/// (and every seam in this workspace), the trait is object-safe and its
/// methods must not panic and must not block: they ride the driving
/// thread's critical path, mirroring `Host` and `SessionObserver`.
pub trait ComponentUnit: Reactions {
    /// The component's root fragment — the teardown surface `dispose`
    /// walks (the generated state's `frag` field).
    fn fragment(&mut self) -> &mut Fragment;
}

/// A mounted component: the state one `build` produced, behind the
/// lifecycle front — mount once, drive through [`Mounted::update`] /
/// [`Mounted::state`], and [`Mounted::dispose`] when the surface goes away.
///
/// The type parameter is the component state itself, so setters and the
/// generated `fire_N` event dispatch stay directly reachable through
/// [`Mounted::state`] — the handle adds the lifecycle, never a second way
/// into the state.
pub struct Mounted<C: ComponentUnit> {
    comp: C,
}

impl<C: ComponentUnit> Mounted<C> {
    /// Mount a component: run `build` against the host and wrap the state.
    ///
    /// The generated `build` performs its own initial flush (§9.2's build
    /// phases end with the init flush — memos and binds initialize before
    /// `build` returns), so mounting is exactly one call; a post-mount
    /// flush on a converged dirty set would be a no-op.
    pub fn mount(host: &mut dyn Host, build: impl FnOnce(&mut dyn Host) -> C) -> Self {
        Self { comp: build(host) }
    }

    /// The component state — setters and the generated `fire_N` dispatch.
    ///
    /// A setter marks the schedule's dirty bits; the flush that drains them
    /// is [`Mounted::update`] (batch + flush in one call) or
    /// [`Mounted::flush`].
    pub fn state(&mut self) -> &mut C {
        &mut self.comp
    }

    /// Flush pending reactions through the const schedule (§7.2) — the
    /// bare tick, for callers that mark dirty bits outside a setter batch
    /// (the generated `fire_N` paths flush themselves). The checked front:
    /// a non-converging schedule surfaces as the typed
    /// [`CycleError`](crate::schedule::CycleError) (LIG-E401), never a
    /// panic.
    ///
    /// # Errors
    /// [`CycleError`](crate::schedule::CycleError) when the schedule does
    /// not converge within [`MAX_PASSES`](crate::schedule::MAX_PASSES)
    /// passes.
    pub fn flush(&mut self, host: &mut dyn Host) -> Result<(), CycleError> {
        flush_checked(&mut self.comp, host)
    }

    /// One save-tick: apply the setter batch, then flush — the discipline
    /// every host loop repeats (mutate state, drain the schedule), as one
    /// call. The checked front: the tick's flush reports a non-converging
    /// schedule as the typed [`CycleError`](crate::schedule::CycleError)
    /// (LIG-E401), never a panic.
    ///
    /// # Errors
    /// [`CycleError`](crate::schedule::CycleError) when the schedule does
    /// not converge within [`MAX_PASSES`](crate::schedule::MAX_PASSES)
    /// passes — the setter batch has been applied (fields hold their new
    /// values); only the re-render is missing.
    pub fn update(
        &mut self,
        host: &mut dyn Host,
        f: impl FnOnce(&mut C),
    ) -> Result<(), CycleError> {
        f(&mut self.comp);
        flush_checked(&mut self.comp, host)
    }

    /// Total disposal (§10.1): walk the root fragment's teardown — mounted
    /// branches, list items, and slot content recursively, then every own
    /// slab record in LIFO order — committing a `destroy` to the host per
    /// node, and return the component state (fields intact, ready for a
    /// remount or inspection; the slab handles inside are spent).
    ///
    /// Disposal is total by the fragment contract: every node the build
    /// created is destroyed exactly once, even when branches, lists, and
    /// slot fills were mounted after it.
    pub fn dispose(self, host: &mut dyn Host) -> C {
        let mut comp = self.comp;
        comp.fragment().teardown(host);
        comp
    }
}

#[cfg(test)]
mod tests {
    use super::{ComponentUnit, Mounted};
    use crate::dirty::DirtySet;
    use crate::fragment::Fragment;
    use crate::host::{Host, NodeId, TemplateDesc};
    use crate::schedule::{Reactions, Schedule};
    use crate::template::{AnchorStep, TplNode};
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;

    /// The counter-shaped template the runtime bench drives: div > (button,
    /// span+marker) with one text hole.
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

    /// The recording host: the mount seam's test instrument.
    #[derive(Debug, Default)]
    struct RecHost {
        commits: Vec<Commit>,
    }

    impl RecHost {
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
        fn attach(
            &mut self,
            _n: NodeId,
            _e: crate::host::EventKind,
            _h: crate::host::HandlerId,
            _d: bool,
        ) {
        }
        fn detach(&mut self, _n: NodeId, _e: crate::host::EventKind, _h: crate::host::HandlerId) {}
        fn on_mount(&mut self, _n: NodeId, _h: crate::host::HookId) {}
        fn on_unmount(&mut self, _n: NodeId, _h: crate::host::HookId) {}
    }

    /// The hand-written counter unit: the same surface generated code
    /// emits — state fields, the root fragment, a `build` ending in the
    /// init flush, the `Reactions` impl, and now the `ComponentUnit` impl.
    struct Mini {
        count: i64,
        frag: Fragment,
    }

    /// One reaction: the text bind, hole 0 renders the count.
    const BIND: u8 = 0;

    static SCHEDULE: Schedule = Schedule {
        order: &[BIND],
        writes: &[0],
        writes_hi: &[],
    };

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

    #[test]
    fn mount_runs_the_build_once_and_holds_the_state() {
        let mut h = RecHost::default();
        let m = Mounted::mount(&mut h, Mini::build);
        let created = h.created();
        assert!(!created.is_empty(), "the build created the template nodes");
        assert!(h.destroyed().is_empty(), "mount destroys nothing");
        // The state is reachable through the handle, fields intact.
        assert_eq!(m.comp.count, 0);
    }

    #[test]
    fn update_applies_the_batch_then_flushes() {
        let mut h = RecHost::default();
        let mut m = Mounted::mount(&mut h, Mini::build);
        // One call: the setter batch, then the flush that drains it.
        m.update(&mut h, |s| s.set_count(3))
            .expect("the counter converges");
        assert_eq!(h.last_text(), Some("3"), "the bind drained in the tick");
        assert_eq!(m.comp.count, 3);
    }

    #[test]
    fn flush_after_convergence_is_a_no_op() {
        let mut h = RecHost::default();
        let mut m = Mounted::mount(&mut h, Mini::build);
        m.update(&mut h, |s| s.set_count(1))
            .expect("the counter converges");
        let settled = h.commits.len();
        m.flush(&mut h).expect("converged");
        m.flush(&mut h).expect("converged");
        assert_eq!(
            h.commits.len(),
            settled,
            "a converged schedule emits no commits"
        );
    }

    #[test]
    fn state_gives_the_setter_surface_without_flushing() {
        let mut h = RecHost::default();
        let mut m = Mounted::mount(&mut h, Mini::build);
        m.state().set_count(7);
        assert_eq!(h.last_text(), Some("0"), "no flush yet: the text is stale");
        m.flush(&mut h).expect("the counter converges");
        assert_eq!(h.last_text(), Some("7"), "the flush drained the batch");
    }

    #[test]
    fn dispose_tears_down_every_node_exactly_once_lifo() {
        let mut h = RecHost::default();
        let m = Mounted::mount(&mut h, Mini::build);
        let created = h.created();
        let state = m.dispose(&mut h);
        assert_eq!(state.count, 0, "the state returns for reuse");
        let destroyed = h.destroyed();
        assert_eq!(
            destroyed.len(),
            created.len(),
            "total disposal: every created node destroyed"
        );
        let mut seen = Vec::new();
        for n in &destroyed {
            assert!(!seen.contains(n), "destroyed exactly once: {n}");
            seen.push(*n);
        }
        assert_eq!(
            destroyed.last().copied(),
            created.first().copied(),
            "LIFO: the root record is destroyed last"
        );
    }

    #[test]
    fn dispose_tears_down_mounted_branches_recursively() {
        // A branch mounted after the build carries its own sub-slab; the
        // dispose must reach it through the recursive teardown.
        static BRANCH_TPL: &[TplNode] = &[TplNode::Element {
            tag: "p",
            attrs: &[],
            style: None,
            children: &[TplNode::Text { value: "big" }],
        }];
        let mut h = RecHost::default();
        let mut m = Mounted::mount(&mut h, Mini::build);
        let before = h.created().len();
        m.state()
            .fragment()
            .mount_branch(&mut h, 0, BRANCH_TPL, &[], &[]);
        let branch_nodes = h.created().len() - before;
        assert!(branch_nodes > 0, "the branch mounted its own nodes");
        m.dispose(&mut h);
        assert_eq!(
            h.destroyed().len(),
            h.created().len(),
            "branch nodes disposed with the owner"
        );
    }

    #[test]
    fn the_seam_is_object_safe() {
        // A host registry stores mounted components behind the trait; the
        // trait object drives flush and teardown like any concrete impl.
        fn flush_dyn(u: &mut dyn ComponentUnit, host: &mut dyn Host) {
            crate::schedule::flush(u, host);
        }
        fn dispose_dyn(u: &mut dyn ComponentUnit, host: &mut dyn Host) {
            u.fragment().teardown(host);
        }
        let mut h = RecHost::default();
        let mut unit = Mini::build(&mut h);
        unit.set_count(5);
        flush_dyn(&mut unit, &mut h);
        assert_eq!(h.last_text(), Some("5"));
        let created = h.created().len();
        dispose_dyn(&mut unit, &mut h);
        assert_eq!(h.destroyed().len(), created);
    }

    #[test]
    fn mounted_flush_surfaces_the_cycle_as_a_typed_error() {
        // The checked mount front: a non-converging schedule is the typed
        // LIG-E401 error on the handle — never a panic — and the dispose
        // still totals the surface afterwards.
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
        let mut h = RecHost::default();
        let mut m = Mounted::mount(&mut h, |host| Cyclic {
            frag: Fragment::new(host, TPL, HOLE_ANCHORS, BRANCH_ANCHORS),
        });
        m.state().frag.mark_mask(1);
        let err: crate::schedule::CycleError = m
            .flush(&mut h)
            .expect_err("the ping-pong schedule never converges");
        assert!(alloc::format!("{err}").contains("LIG-E401"));
        let created = h.created().len();
        m.dispose(&mut h);
        assert_eq!(h.destroyed().len(), created, "dispose still totals");
    }
}
