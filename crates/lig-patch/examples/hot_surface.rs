//! The hot-patch protocol's consumer walkthrough: a mounted surface driven
//! through the patch engine's host bridge — the mount seam's report named
//! the pair as its last untouched gap ("the patch engine's host bridge,
//! `lig-patch` driving a mounted surface"), and the M22 report named the
//! missing walkthrough. This example is both: a custom [`Host`]
//! implementation spelled out in full (the 15 methods a consumer writes —
//! the runtime half's embedding work), a hand-written counter unit of the
//! generated shape, and [`HotSurface`] routing every event dispatch
//! through the jump table across three generations.
//!
//! The dispatch bodies stand in for patch dylib symbols: the fat build's
//! `dispatch_v0`, then two "patch generations" whose bodies carry their
//! own behavior — committed in-process via
//! [`commit_table`](lig_patch::commit_table), the same mechanics
//! [`PatchSession::apply`](lig_patch::PatchSession::apply) drives against
//! real dylibs (that path — cdylib rebuilds, symbol resolution, the
//! chained manifests, the save→frame budget — is the `lig patch-spike`
//! gate's proof; the route mechanics proven here are identical, minus the
//! loader). The never-dlclose discipline is proven on the surface: the
//! generation-1 committed address, captured before generation 2, still
//! runs the generation-1 body after any number of later generations.
//!
//! Run:
//!
//! ```sh
//! cargo run -p lig-patch --example hot_surface
//! ```

use lig_patch::{
    DispatchFn, HotSurface, call_captured_dispatch, captured_dispatch_address, commit_table,
    hot_dispatch,
};
use lig_runtime::host::{EventKind, HandlerId, HookId, Host, NodeId, TemplateDesc};
use lig_runtime::schedule::{CycleError, Reactions, Schedule};
use lig_runtime::{AttrVal, DirtySet, EvData, Fragment, PropVal, StyleConstId, TplNode};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// The custom Host implementation — the 15-method commit stream a consumer
// writes to embed the runtime (§10.1): a recording host this walkthrough
// asserts its story on. The node table is the host's own representation,
// side-tabled by `NodeId` (the runtime slab tracks structure only).
// ---------------------------------------------------------------------------

/// One commit in the stream — the lifecycle families the walkthrough
/// asserts on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Commit {
    /// `create` — the node materialized.
    Create(NodeId),
    /// `set_text` — a text bind re-rendered.
    Text(NodeId, String),
    /// `destroy` — a node torn down.
    Destroy(NodeId),
}

/// The walkthrough's custom host: records the commit stream, owns nothing
/// about components — the registry of mounted units lives host-side (here:
/// one [`HotSurface`], the embedding's own store).
#[derive(Debug, Default)]
struct Recorder {
    commits: Vec<Commit>,
}

impl Recorder {
    /// How many `create` commits the stream carries.
    fn created(&self) -> usize {
        self.commits
            .iter()
            .filter(|c| matches!(c, Commit::Create(..)))
            .count()
    }

    /// How many `destroy` commits the stream carries.
    fn destroyed(&self) -> usize {
        self.commits
            .iter()
            .filter(|c| matches!(c, Commit::Destroy(..)))
            .count()
    }

    /// The most recent text a bind wrote.
    fn last_text(&self) -> Option<&str> {
        self.commits.iter().rev().find_map(|c| match c {
            Commit::Text(_, t) => Some(t.as_str()),
            _ => None,
        })
    }
}

impl Host for Recorder {
    fn create(&mut self, node: NodeId, _desc: TemplateDesc<'_>) {
        self.commits.push(Commit::Create(node));
    }

    fn destroy(&mut self, node: NodeId) {
        self.commits.push(Commit::Destroy(node));
    }

    fn set_text(&mut self, node: NodeId, text: &str) {
        self.commits.push(Commit::Text(node, text.to_string()));
    }

    fn set_attr(&mut self, _node: NodeId, _key: &str, _val: Option<AttrVal<'_>>) {}
    fn set_class(&mut self, _node: NodeId, _class: Option<&str>) {}
    fn set_style(&mut self, _node: NodeId, _style: StyleConstId) {}
    fn set_style_dyn(&mut self, _node: NodeId, _props: &[(u32, PropVal<'_>)]) {}
    fn set_html(&mut self, _node: NodeId, _html: &str) {}
    fn insert(&mut self, _parent: NodeId, _child: NodeId, _index: u32) {}
    fn remove(&mut self, _parent: NodeId, _child: NodeId) {}
    fn move_(&mut self, _parent: NodeId, _child: NodeId, _index: u32) {}
    fn attach(&mut self, _node: NodeId, _ev: EventKind, _handler: HandlerId, _delegate: bool) {}
    fn detach(&mut self, _node: NodeId, _ev: EventKind, _handler: HandlerId) {}
    fn on_mount(&mut self, _node: NodeId, _hook: HookId) {}
    fn on_unmount(&mut self, _node: NodeId, _hook: HookId) {}
}

// ---------------------------------------------------------------------------
// The counter unit — the surface generated code emits (state fields, the
// root fragment, a build ending in the init flush, the `Reactions` impl,
// the `ComponentUnit` impl): the same shape the compile half emits for the
// committed counter fixture.
// ---------------------------------------------------------------------------

/// The counter-shaped template: div > (button, span+marker) with one text
/// hole — the mount seam's test template.
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

/// The hand-written counter unit: the mounted surface's component.
struct Counter {
    count: i64,
    frag: Fragment,
}

impl Counter {
    /// The generated build shape: create the fragment, mark the initial
    /// bits, run the init flush — the unit is live when `build` returns.
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

impl Reactions for Counter {
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

impl lig_runtime::ComponentUnit for Counter {
    fn fragment(&mut self) -> &mut Fragment {
        &mut self.frag
    }
}

// ---------------------------------------------------------------------------
// The patch generations: the fat build's dispatch body (v0) and two
// "patched" bodies — each generation's dylib would re-export the same
// symbol with its own behavior; here the behavior delta is the increment,
// visible in the rendered text.
// ---------------------------------------------------------------------------

/// The fat build's dispatch body (v0): the generated `fire_N` glue shape —
/// mutate, mark the bind, the checked flush.
fn dispatch_v0(state: &mut Counter, host: &mut dyn Host, _ev: &EvData) -> Result<(), CycleError> {
    state.set_count(state.count + 1);
    lig_runtime::flush_checked(state, host)
}

/// The generation-1 body: the patch's new behavior (a bigger increment).
fn dispatch_v1(state: &mut Counter, host: &mut dyn Host, _ev: &EvData) -> Result<(), CycleError> {
    state.set_count(state.count + 2);
    lig_runtime::flush_checked(state, host)
}

/// The generation-2 body: the newest patch's behavior (biggest increment).
fn dispatch_v2(state: &mut Counter, host: &mut dyn Host, _ev: &EvData) -> Result<(), CycleError> {
    state.set_count(state.count + 3);
    lig_runtime::flush_checked(state, host)
}

/// Commit one in-process generation: old (the fat build's body) → new (the
/// patched body), the route `PatchSession::apply` commits against real
/// dylib symbols.
fn apply_generation(new: DispatchFn<Counter, EvData>) {
    let old = dispatch_v0 as DispatchFn<Counter, EvData> as usize as u64;
    let new_addr = new as usize as u64;
    let mut map = BTreeMap::new();
    map.insert(old, new_addr);
    commit_table(lig_patch::JumpTable::new(map));
}

fn main() {
    let mut host = Recorder::default();

    // 1. Mount the counter under the hot surface: the build runs its init
    //    flush, the unit is live, and every dispatch from here on is a
    //    safepoint.
    let mut surface = HotSurface::mount(&mut host, Counter::build);
    println!("mounted — registry of one, dispatches routed through the jump table");

    // 2. Miss path: no table committed yet — the safepoint calls the fat
    //    build's body. The counter ticks to 1; the bind re-rendered.
    let ev = EvData::new().current_target_value("1");
    surface
        .dispatch(dispatch_v0, &mut host, &ev)
        .expect("converges");
    println!(
        "v0 (miss): count = {}, text = {:?}",
        surface.surface().state().count,
        host.last_text()
    );
    assert_eq!(surface.surface().state().count, 1);
    assert_eq!(host.last_text(), Some("1"));

    // 3. Generation 1 applies (in-process stand-in for a real patch dylib):
    //    the mounted front picks the patched body up on the NEXT dispatch —
    //    no remount, no state loss.
    apply_generation(dispatch_v1);
    surface
        .dispatch(dispatch_v0, &mut host, &ev)
        .expect("converges");
    println!(
        "v1 (routed): count = {}, text = {:?}",
        surface.surface().state().count,
        host.last_text()
    );
    assert_eq!(surface.surface().state().count, 3, "generation 1: +2");

    // 4. Capture the generation-1 committed address — what a stale frame
    //    inside generation 1 would return into.
    let gen1_addr =
        captured_dispatch_address(dispatch_v0).expect("generation 1 committed the route");
    assert_ne!(
        gen1_addr,
        dispatch_v0 as DispatchFn<Counter, EvData> as usize as u64
    );

    // 5. Generation 2: the newest table routes the same safepoint to the
    //    newest body.
    apply_generation(dispatch_v2);
    surface
        .dispatch(dispatch_v0, &mut host, &ev)
        .expect("converges");
    println!(
        "v2 (routed): count = {}, text = {:?}",
        surface.surface().state().count,
        host.last_text()
    );
    assert_eq!(surface.surface().state().count, 6, "generation 2: +3");

    // 6. Never-dlclose, proven on the surface: the generation-1 address is
    //    still mapped and still runs the generation-1 body.
    let mut stale_host_surface = surface;
    call_captured_dispatch(
        gen1_addr,
        stale_host_surface.surface().state(),
        &mut host,
        &ev,
    )
    .expect("the superseded generation stays executable");
    assert_eq!(
        stale_host_surface.surface().state().count,
        8,
        "the stale address ran generation 1's body: +2"
    );
    println!("stale frame: gen-1 address still runs (+2) — never-dlclose holds");

    // 7. The steady state, one more dispatch through the newest table, and
    //    the hot_dispatch safepoint used directly (the free-fn shape hosts
    //    that drive `Mounted` themselves use):
    hot_dispatch(
        dispatch_v0,
        stale_host_surface.surface().state(),
        &mut host,
        &ev,
    )
    .expect("converges");
    assert_eq!(
        stale_host_surface.surface().state().count,
        11,
        "generation 2 routes the steady-state dispatch: +3"
    );
    println!(
        "steady state: count = {}, text = {:?}",
        stale_host_surface.surface().state().count,
        host.last_text()
    );

    // 8. Dispose: the mount seam's total teardown — every node the build
    //    created reaches the host exactly once, whatever generations
    //    dispatched in between.
    let created = host.created();
    let state = stale_host_surface.dispose(&mut host);
    println!(
        "disposed: count = {}, {created} create commits, {} destroy commits — balanced: {}",
        state.count,
        host.destroyed(),
        created == host.destroyed()
    );
    assert_eq!(
        host.destroyed(),
        created,
        "total disposal balances the stream"
    );
}
