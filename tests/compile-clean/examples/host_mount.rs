//! The runtime half, walked end to end (M20): the committed `counter` unit —
//! the same artifact `lig reproducibility` byte-pins — mounted on the mock
//! host through the component-mount seam, driven through one save-tick and
//! one event dispatch, and disposed with a total LIFO teardown.
//!
//! This is the consumer walkthrough the compile half never needed: on the
//! compile side a session wires everything (`Session::compile_with`,
//! `Session::emit_to`); on the runtime side the mount front is the same
//! one-call story — `Mounted::mount`, `Mounted::update`, `Mounted::dispose`
//! (the checked front: ticks report a non-converging schedule as the typed
//! `CycleError`, LIG-E401, never a panic). The one consumer impl the seam
//! asks for lives next to the `include!`d
//! unit it names (here: `compile_clean`'s `ComponentUnit for Counter`; in a
//! real embedding crate, beside your own units).
//!
//! Run without touching `dist/`:
//!
//! ```sh
//! cargo run -p compile-clean --example host_mount
//! ```

use compile_clean::counter::{self, Counter};
use lig_runtime::mount::{ComponentUnit, Mounted};
use lig_runtime::{
    AnchorStep, CycleError, DirtySet, EvData, Fragment, Reactions, Schedule, TplNode,
};
use mock_host::{Commit, MockHost};

fn main() {
    // 1. The style table: the generated unit interned one static style
    //    (`Counter_STYLE_CSS`); the host binds the table once by convention
    //    and resolves every `set_style(StyleConstId(0))` commit from it.
    let mut host = MockHost::new();
    host.bind_style_table(counter::generated::Counter_STYLE_CSS);

    // 2. Mount — the generated `build` against the host, wrapped in the
    //    handle. The build runs its own init flush, so the mounted unit is
    //    live the moment this returns (the memo and both binds initialized,
    //    the click handler attached).
    let mut mounted = Mounted::mount(&mut host, |h| {
        Counter::build(h, counter::generated::CounterProps {})
    });

    // 3. One save-tick: the setter batch, then the const-schedule flush.
    //    The bind re-renders the count into the text hole; the memo keeps
    //    `doubled` in step. The checked front: a non-converging schedule
    //    would come back as the typed `CycleError` (LIG-E401), never a
    //    panic — the counter converges, so the tick is an `Ok`.
    mounted
        .update(&mut host, |s| s.set_count(3))
        .expect("the counter converges");
    let s = mounted.state();
    println!("count = {} (doubled = {})", s.count, s.doubled);

    // 4. Event dispatch: the generated `fire_1` captures the payload, marks
    //    the handler reaction, and flushes through the checked front — the
    //    counter's effect increments `count`, and the tick the fire drives
    //    re-renders. The dispatch is the checked front now: a non-converging
    //    schedule comes back as the typed `CycleError` (LIG-E401), never a
    //    panic inside the event.
    mounted
        .state()
        .fire_1(&mut host, &EvData::new().current_target_value("3"))
        .expect("the counter's dispatch converges");
    let s = mounted.state();
    println!(
        "after the click: count = {} (doubled = {})",
        s.count, s.doubled
    );

    // 5. Dispose: the total LIFO teardown — branches, list items, and slot
    //    content recursively, then every own slab record — one `destroy`
    //    commit per node the build created, and the state comes back.
    let state = mounted.dispose(&mut host);
    let creates = host.count_kind("Create");
    let destroys = host.count_kind("Destroy");
    println!(
        "disposed: count = {}, {creates} create commits, {destroys} destroy commits — balanced: {}",
        state.count,
        creates == destroys
    );

    // The commit stream ends balanced: every node the build created was
    // destroyed exactly once (assert it, so the walkthrough is also a gate).
    assert_eq!(creates, destroys, "total disposal balances the stream");
    assert_eq!(state.count, 4, "the click effect incremented 3 → 4");
    assert!(
        host.commits
            .iter()
            .any(|c| matches!(c, Commit::SetStyle { style: 0, .. })),
        "the static style commit resolved through the bound table"
    );

    // 6. The checked front's failure path (M21 + the checked event glue):
    //    a hand-written unit whose reactions ping-pong forever (r0 writes
    //    r1, r1 writes r0) — the shape the compile-time cycle check denies,
    //    had it slipped through. Its `fire` is the exact glue shape every
    //    generated `fire_N` now emits: mark the handler reaction, then the
    //    checked flush. The dispatch surfaces the cycle as the typed
    //    `CycleError`, bounded by `MAX_PASSES`, and the teardown still
    //    totals afterwards.
    let mut probe = MockHost::new();
    let mut broken = Mounted::mount(&mut probe, Broken::build);
    let err: CycleError = broken
        .state()
        .fire(&mut probe)
        .expect_err("a ping-pong never converges");
    println!("checked dispatch: {err}");
    let _state = broken.dispose(&mut probe);
    assert_eq!(err.passes, lig_runtime::MAX_PASSES);
}

/// The deliberately broken unit the failure path drives: two reactions that
/// re-dirty each other every pass — the schedule never converges, and the
/// checked dispatch reports it instead of panicking (LIG-E401). Its `fire`
/// is the glue shape the generator emits for every event handler.
struct Broken {
    frag: Fragment,
}

static BROKEN_TPL: &[TplNode] = &[TplNode::Element {
    tag: "div",
    attrs: &[],
    style: None,
    children: &[],
}];

static BROKEN_HOLES: &[&[AnchorStep]] = &[];

static BROKEN_SCHEDULE: Schedule = Schedule {
    order: &[0, 1],
    writes: &[1 << 1, 1 << 0],
    writes_hi: &[],
};

impl Broken {
    fn build(h: &mut dyn lig_runtime::Host) -> Self {
        Self {
            frag: Fragment::new(h, BROKEN_TPL, BROKEN_HOLES, &[]),
        }
    }

    /// The event-glue shape the generator emits (`fire_N`): mark the
    /// handler reaction, then the checked flush — the cycle reports as
    /// data.
    fn fire(&mut self, host: &mut dyn lig_runtime::Host) -> Result<(), CycleError> {
        self.frag.mark_mask(1); // the ping-pong starts dirty
        lig_runtime::flush_checked(self, host)
    }
}

impl Reactions for Broken {
    fn schedule(&self) -> &'static Schedule {
        &BROKEN_SCHEDULE
    }

    fn dirty(&mut self) -> &mut DirtySet {
        &mut self.frag.dirty
    }

    fn run(&mut self, _host: &mut dyn lig_runtime::Host, _r: u8) {}
}

impl ComponentUnit for Broken {
    fn fragment(&mut self) -> &mut Fragment {
        &mut self.frag
    }
}
