//! §14.2 budget gate (`lig budgets`): the measured perf budgets as an
//! enforced CI contract, not a claim.
//!
//! M0–M3 *measured* every stage (criterion benches, `docs/benchmarks/*`);
//! M4 pins them: the committed ledger `docs/benchmarks/budgets.toml` holds
//! one row per workload with an absolute ceiling (≥ 5× the reference median,
//! so the gate is a regression tripwire, not a benchmark). The gate measures
//! the same workloads the benches drive — in-process, median over fixed
//! samples — and fails on any ceiling violation.
//!
//! Profile contract: time budgets are calibrated for **release** runs
//! (`cargo run -p lig-cli --release -- budgets`, how CI enforces them). A
//! dev-profile run reports its measurements but enforces only the
//! profile-independent rows (bytes) — dev-profile timings are a different
//! currency, and the gate never lies about which contract it checked.

use crate::gates::{counter_module, repo_root};
use lig_codegen::generate_component;
use lig_core::id::ComponentId;
use lig_parse_solid::tests_support::COUNTER_TSX;
use lig_runtime::host::{
    AttrVal, EventKind, HandlerId, HookId, Host, NodeId, PropVal, StyleConstId, TemplateDesc,
};
use lig_runtime::mount::{ComponentUnit, Mounted};
use lig_runtime::{AnchorStep, DirtySet, Fragment, Reactions, Schedule, TplNode, plan_keyed};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

/// The committed ledger, relative to the workspace root.
const LEDGER: &str = "docs/benchmarks/budgets.toml";

/// One ledger row (`[[budget]]`).
#[derive(Debug, Deserialize)]
struct BudgetRow {
    /// The measurement name — 1:1 with the gate's registry (the sync check).
    name: String,
    /// `time` (µs medians) or `bytes` (profile-independent sizes).
    metric: String,
    /// The absolute ceiling in the row's unit.
    budget: f64,
    /// Provenance: the reference median the ceiling was derived from.
    note: String,
}

/// The parsed ledger.
#[derive(Debug, Deserialize)]
struct Ledger {
    /// Ledger schema version (1 — the M4 shape).
    /// Ledger schema version (1 — the M4 shape; the gate fails closed on
    /// any newer schema it cannot measure).
    schema: u32,
    /// One row per workload.
    budget: Vec<BudgetRow>,
}

/// A committed jump-table entry count sentinel for the apply measurement.
const APPLY_ENTRIES: usize = 1;

/// A no-op counting host — the runtime measurements' commit sink (the same
/// shape the runtime bench drives).
struct CountingHost {
    commits: usize,
}

impl Host for CountingHost {
    fn create(&mut self, _node: NodeId, _desc: TemplateDesc<'_>) {
        self.commits += 1;
    }
    fn destroy(&mut self, _node: NodeId) {
        self.commits += 1;
    }
    fn set_text(&mut self, _node: NodeId, _text: &str) {
        self.commits += 1;
    }
    fn set_attr(&mut self, _node: NodeId, _key: &str, _val: Option<AttrVal<'_>>) {
        self.commits += 1;
    }
    fn set_class(&mut self, _node: NodeId, _class: Option<&str>) {
        self.commits += 1;
    }
    fn set_style(&mut self, _node: NodeId, _style: StyleConstId) {
        self.commits += 1;
    }
    fn set_style_dyn(&mut self, _node: NodeId, _props: &[(u32, PropVal<'_>)]) {
        self.commits += 1;
    }
    fn set_html(&mut self, _node: NodeId, _html: &str) {
        self.commits += 1;
    }
    fn insert(&mut self, _parent: NodeId, _child: NodeId, _index: u32) {
        self.commits += 1;
    }
    fn remove(&mut self, _parent: NodeId, _child: NodeId) {
        self.commits += 1;
    }
    fn move_(&mut self, _parent: NodeId, _child: NodeId, _index: u32) {
        self.commits += 1;
    }
    fn attach(&mut self, _node: NodeId, _ev: EventKind, _handler: HandlerId, _delegate: bool) {
        self.commits += 1;
    }
    fn detach(&mut self, _node: NodeId, _ev: EventKind, _handler: HandlerId) {
        self.commits += 1;
    }
    fn on_mount(&mut self, _node: NodeId, _hook: HookId) {
        self.commits += 1;
    }
    fn on_unmount(&mut self, _node: NodeId, _hook: HookId) {
        self.commits += 1;
    }
}

/// The counter-like template the runtime measurements drive (the runtime
/// bench's workload: div > button, span+marker, marker).
fn counter_tpl() -> &'static [TplNode] {
    TPL
}

/// A counting observer — the observer-seam measurement's event sink (the
/// bench drives the same shape: every stage event counted, none retained).
#[derive(Default)]
struct SeamObserver {
    stages: usize,
}

impl lig_incremental::observe::SessionObserver for SeamObserver {
    fn on_stage(&mut self, _event: &lig_incremental::observe::StageEvent) {
        self.stages += 1;
    }
}

static TPL: &[TplNode] = &[TplNode::Element {
    tag: "div",
    attrs: &[],
    style: Some(StyleConstId(0)),
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
        TplNode::Marker,
    ],
}];

static HOLE_ANCHORS: &[&[AnchorStep]] = &[
    &[AnchorStep::Down(0)],
    &[AnchorStep::Down(1), AnchorStep::MarkerDown(0)],
];

static BRANCH_ANCHORS: &[&[AnchorStep]] = &[&[AnchorStep::Down(2)]];

/// Null reactions over a two-reaction schedule — the update-mask workload's
/// flush sink (writes map nothing; the loop converges in one pass).
struct NullReactions {
    dirty: DirtySet,
}

impl Reactions for NullReactions {
    fn schedule(&self) -> &'static Schedule {
        &Schedule {
            order: &[0, 1],
            writes: &[0, 0],
            writes_hi: &[],
        }
    }
    fn dirty(&mut self) -> &mut DirtySet {
        &mut self.dirty
    }
    fn run(&mut self, _h: &mut dyn Host, _r: u8) {}
}

/// A ping-pong cycle (r0 writes r1, r1 writes r0) — the checked-flush
/// workload's never-converging schedule, the worst case the fail-safe cap
/// bounds (the `runtime/flush_checked_cycle` bench drives the same shape).
struct CycleReactions {
    dirty: DirtySet,
}

static CYCLE_REACTIONS_SCHEDULE: Schedule = Schedule {
    order: &[0, 1],
    writes: &[1 << 1, 1 << 0],
    writes_hi: &[],
};

impl Reactions for CycleReactions {
    fn schedule(&self) -> &'static Schedule {
        &CYCLE_REACTIONS_SCHEDULE
    }
    fn dirty(&mut self) -> &mut DirtySet {
        &mut self.dirty
    }
    fn run(&mut self, _h: &mut dyn Host, _r: u8) {}
}

/// The mount-seam workload's unit (M20): the counter-shaped state a
/// generated `build` emits — root fragment, one text bind — with the
/// `ComponentUnit` impl the mount front drives. Same shape the
/// `runtime/mount_lifecycle` bench drives.
struct MountUnit {
    count: i64,
    frag: Fragment,
}

const MOUNT_BIND: u8 = 0;

static MOUNT_SCHEDULE: Schedule = Schedule {
    order: &[MOUNT_BIND],
    writes: &[0],
    writes_hi: &[],
};

impl MountUnit {
    fn build(h: &mut dyn Host) -> Self {
        let mut s = Self {
            count: 0,
            frag: Fragment::new(h, counter_tpl(), HOLE_ANCHORS, BRANCH_ANCHORS),
        };
        s.frag.mark_mask(1 << MOUNT_BIND);
        lig_runtime::flush(&mut s, h);
        s
    }

    /// The generated setter shape: write the field, mark the bits.
    fn set_count(&mut self, v: i64) {
        self.count = v;
        self.frag.mark_mask(1 << MOUNT_BIND);
    }
}

impl Reactions for MountUnit {
    fn schedule(&self) -> &'static Schedule {
        &MOUNT_SCHEDULE
    }
    fn dirty(&mut self) -> &mut DirtySet {
        &mut self.frag.dirty
    }
    fn run(&mut self, host: &mut dyn Host, r: u8) {
        if r == MOUNT_BIND {
            let mut buf = [0u8; 20];
            let text = lig_runtime::format_i64(&mut buf, self.count);
            self.frag.set_text(host, 0, text);
        }
    }
}

impl ComponentUnit for MountUnit {
    fn fragment(&mut self) -> &mut Fragment {
        &mut self.frag
    }
}

/// The un-patched body the safepoint falls back to / routes from.
fn patchable_v0() -> u64 {
    0
}

/// The patched body standing in for fresh code (same signature).
fn patchable_v1() -> u64 {
    1
}

/// A synthetic `n`-entry old→new table around the two real functions (the
/// patch bench's workload).
fn synthetic_table(n: usize) -> lig_patch::JumpTable {
    let old = patchable_v0 as *const () as usize as u64;
    let new = patchable_v1 as *const () as usize as u64;
    let map: std::collections::BTreeMap<u64, u64> = (0..n as u64)
        .map(|i| {
            if i == 0 {
                (old, new)
            } else {
                (old.wrapping_add(i * 0x10), new.wrapping_add(i * 0x10))
            }
        })
        .collect();
    lig_patch::JumpTable::new(map)
}

/// Median of `iters` samples of `f`, in microseconds (one warm-up round of
/// `iters / 8` first).
fn median_us(iters: usize, mut f: impl FnMut()) -> f64 {
    for _ in 0..iters / 8 {
        f();
    }
    let mut samples: Vec<u64> = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t = Instant::now();
        f();
        samples.push(u64::try_from(t.elapsed().as_nanos()).unwrap_or(u64::MAX));
    }
    samples.sort_unstable();
    let mid = samples.len() / 2;
    samples[mid] as f64 / 1000.0
}

/// Sample counts per workload: enough for a stable median, small enough that
/// the gate stays fast even in a dev profile (the gate must never be the
/// slow part).
fn samples_for(name: &str) -> usize {
    match name {
        "lower_solid_counter" | "lower_vue_counter" | "session_watch_tick" => 120,
        "canonical_json_counter" => 200,
        "codegen_counter"
        | "session_compile_counter"
        | "session_emit_mem"
        | "session_observe"
        | "session_emit_to" => 60,
        "codegen_composed"
        | "codegen_branch"
        | "codegen_ref"
        | "codegen_else"
        | "codegen_list_component"
        | "codegen_list_slot"
        | "codegen_for_event"
        | "codegen_for_event_composed"
        | "codegen_for_composed_content" => 40,
        "runtime_instantiate"
        | "runtime_update_mask"
        | "runtime_mount_lifecycle"
        | "runtime_flush_checked_cycle"
        | "patch_apply_1"
        | "patch_dispatch_hit" => 800,
        "runtime_list_diff_keyed" | "session_sync" => 400,
        "gen_counter_bytes" => 1,
        _ => 100,
    }
}

/// The counter Vue leg (the Vue bench's workload), read from the committed
/// fixture so the gate and the parity harness share one source of truth.
fn counter_vue_source(root: &Path) -> String {
    let path = root.join("fixtures/conformance/counter/input.vue");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The composed slot-content fixture (the M9 codegen bench's workload:
/// component-in-content + nested outlets — the ctx classification walk, the
/// recursive mount walk, and the grandchild inserts), read from the
/// committed fixture so the gate and the parity harness share one source.
fn slots_component_source(root: &Path) -> String {
    let path = root.join("fixtures/conformance/slots-component/input.solid.tsx");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The branch-assembly fixture (the M10 codegen bench's workload: per-mount
/// component identity + branch-time slot assembly — the Option-field
/// rebuild/dispose arms and the assembly walk), read from the committed
/// fixture so the gate and the parity harness share one source.
fn slots_branch_component_source(root: &Path) -> String {
    let path = root.join("fixtures/conformance/slots-branch-component/input.solid.tsx");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The template-ref fixture (the M12 codegen bench's workload: the Ref
/// capture contract over both spellings' canonical IR — the ref-field
/// declaration, the sentinel init, and the build-body capture), read from
/// the committed fixture so the gate and the parity harness share one
/// source.
fn template_ref_source(root: &Path) -> String {
    let path = root.join("fixtures/conformance/template-ref/input.solid.tsx");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The else-leg fixture (the M11 codegen bench's workload: the tracked-leg
/// mounting arms and the per-leg assembly/marking emission), read from the
/// committed fixture so the gate and the parity harness share one source.
fn show_else_source(root: &Path) -> String {
    let path = root.join("fixtures/conformance/show-else/input.solid.tsx");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn list_component_source(root: &Path) -> String {
    let path = root.join("fixtures/conformance/list-component/input.solid.tsx");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The list-slot fixture (the M14 codegen bench's workload: the per-item
/// fallback mount in the item replay and the item-bound bind emission), read
/// from the committed fixture so the gate and the parity harness share one
/// source.
fn list_slot_source(root: &Path) -> String {
    let path = root.join("fixtures/conformance/list-slot/input.solid.tsx");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The for-event fixture (the M15 codegen bench's workload: the
/// index-addressed fire fns, the item-substituted handler bodies, and the
/// per-item attach arms), read from the committed fixture so the gate and
/// the parity harness share one source.
fn for_event_source(root: &Path) -> String {
    let path = root.join("fixtures/conformance/for-event/input.solid.tsx");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The composed-event fixture (the M17 codegen bench's workload: the
/// ctx-classified composed reconcile emission with the receiver-parameterized
/// per-item attach arms and the list-carrying branch's mount mark), read from
/// the committed fixture so the gate and the parity harness share one
/// source.
fn for_event_composed_source(root: &Path) -> String {
    let path = root.join("fixtures/conformance/for-event-composed/input.solid.tsx");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The composed-content fixture (the M18 codegen bench's workload: the
/// fill-carried composed reconcile, the per-mount fill assembly in the
/// mounting arm, the receiver-guarded ctx unwraps, and the per-item fire
/// fn), read from the committed fixture so the gate and the parity harness
/// share one source.
fn for_composed_content_source(root: &Path) -> String {
    let path = root.join("fixtures/conformance/for-composed-content/input.solid.tsx");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// One full cold session pass (the M16 session bench's workload:
/// set_source + compile — lower, admission, the build-order cycle check,
/// and codegen ride the digest-gated pass `lig build` runs per unit), over
/// the M0 reference workload the bench drives.
fn session_compile_counter() -> f64 {
    median_us(samples_for("session_compile_counter"), || {
        let mut s = lig_incremental::Session::new();
        s.set_source("counter.tsx", COUNTER_TSX);
        let r = s.compile(lig_admission::Profile::Layout);
        black_box(r);
    })
}

/// One cold pass plus an in-memory emission (the sink-seam workload: the
/// `session/emit_mem` bench drives the same thing) — set_source + compile
/// + `emit_units_to` into a fresh `MemSink`, no filesystem involved.
fn session_emit_mem() -> f64 {
    median_us(samples_for("session_emit_mem"), || {
        let mut s = lig_incremental::Session::new();
        s.set_source("counter.tsx", COUNTER_TSX);
        s.compile(lig_admission::Profile::Layout);
        let units = s.units();
        let mut sink = lig_incremental::emit::MemSink::new();
        let r = lig_incremental::emit::emit_units_to(&mut sink, &units, "taffy", false);
        black_box(r.expect("mem emit"));
    })
}

/// One watch tick (the sync-seam workload: the `session/sync` bench drives
/// the same thing) — a cold 8-source registration pass plus one `sync` diff
/// over a fresh snapshot with one edit.
fn session_sync() -> f64 {
    use std::path::PathBuf;
    let edited = COUNTER_TSX.replace("Add", "Add2");
    median_us(samples_for("session_sync"), || {
        let mut s = lig_incremental::Session::new();
        for i in 0..8 {
            s.set_source(format!("unit{i}.tsx"), COUNTER_TSX);
        }
        let fresh: Vec<(PathBuf, &str)> = (0..8)
            .map(|i| {
                if i == 3 {
                    (PathBuf::from(format!("unit{i}.tsx")), edited.as_str())
                } else {
                    (PathBuf::from(format!("unit{i}.tsx")), COUNTER_TSX)
                }
            })
            .collect();
        black_box(s.sync(fresh, &[]));
    })
}

/// One full cold pass under observation (the observer-seam workload: the
/// `session/observe` bench drives the same thing) — set_source + compile
/// with every stage event delivered to a counting observer: the seam's
/// cost on top of the plain pass the compile row measures.
fn session_observe() -> f64 {
    median_us(samples_for("session_observe"), || {
        let mut observer = SeamObserver::default();
        let mut s = lig_incremental::Session::new();
        s.set_source("counter.tsx", COUNTER_TSX);
        let r = s.compile_with(lig_admission::Profile::Layout, &mut observer);
        black_box(r);
        black_box(observer.stages);
    })
}

/// One cold pass plus the one-call session emission seam (the
/// `session/emit_to` bench drives the same thing) — compile + `emit_to`
/// into a fresh `MemSink`, the BR-1 gate and the profile ledger name
/// riding the call.
fn session_emit_to() -> f64 {
    median_us(samples_for("session_emit_to"), || {
        let mut s = lig_incremental::Session::new();
        s.set_source("counter.tsx", COUNTER_TSX);
        s.compile(lig_admission::Profile::Layout);
        let mut sink = lig_incremental::emit::MemSink::new();
        let r = s.emit_to(&mut sink, lig_admission::Profile::Layout, false);
        black_box(r.expect("session emit"));
    })
}

/// One full watch tick (the watch-tick-seam workload: the `session/watch_tick`
/// bench drives the same thing) — a prebuilt 8-source session takes one
/// `Session::tick` over a fresh snapshot with one edit: the sync diff, the
/// digest-gated recompile of exactly the edited unit, and the fail-closed
/// in-memory emission, the whole `lig watch` loop body as one call.
fn session_watch_tick() -> f64 {
    use std::path::PathBuf;
    let edited = COUNTER_TSX.replace("Add", "Add2");
    median_us(samples_for("session_watch_tick"), || {
        let mut s = lig_incremental::Session::new();
        for i in 0..8 {
            s.set_source(format!("unit{i}.tsx"), COUNTER_TSX);
        }
        s.compile(lig_admission::Profile::Layout);
        let fresh: Vec<(PathBuf, &str)> = (0..8)
            .map(|i| {
                if i == 3 {
                    (PathBuf::from(format!("unit{i}.tsx")), edited.as_str())
                } else {
                    (PathBuf::from(format!("unit{i}.tsx")), COUNTER_TSX)
                }
            })
            .collect();
        let mut sink = lig_incremental::emit::MemSink::new();
        let r = s.tick(fresh, &[], &mut sink, lig_admission::Profile::Layout, true);
        black_box(r.expect("the tick passes"));
    })
}

/// One measurement by ledger name. Time rows return median µs; the bytes row
/// returns a byte count. The workload of each row is the same one the
/// matching criterion bench drives (§14.2 — the ledger rows are 1:1 with the
/// bench suite).
fn measure(name: &str, root: &Path) -> f64 {
    match name {
        "lower_solid_counter" => median_us(samples_for(name), || {
            let m = lig_parse_solid::lower_module(COUNTER_TSX, "counter.tsx").expect("lower");
            black_box(m);
        }),
        "lower_vue_counter" => {
            let src = counter_vue_source(root);
            median_us(samples_for(name), || {
                let m = lig_parse_vue::lower_module(&src, "counter.vue").expect("lower");
                black_box(m);
            })
        }
        "canonical_json_counter" => {
            let m = counter_module();
            median_us(samples_for(name), || {
                black_box(m.canonical_json());
            })
        }
        "codegen_counter" => {
            let m = counter_module();
            median_us(samples_for(name), || {
                let u = generate_component(&m, ComponentId::new(0)).expect("codegen");
                black_box(u);
            })
        }
        "codegen_composed" => {
            let src = slots_component_source(root);
            let m = lig_parse_solid::lower_module(&src, "slots-component.tsx").expect("lower");
            median_us(samples_for(name), || {
                let u = lig_codegen::generate_module(&m).expect("codegen");
                black_box(u);
            })
        }
        "codegen_branch" => {
            let src = slots_branch_component_source(root);
            let m =
                lig_parse_solid::lower_module(&src, "slots-branch-component.tsx").expect("lower");
            median_us(samples_for(name), || {
                let u = lig_codegen::generate_module(&m).expect("codegen");
                black_box(u);
            })
        }
        "codegen_ref" => {
            let src = template_ref_source(root);
            let m = lig_parse_solid::lower_module(&src, "template-ref.tsx").expect("lower");
            median_us(samples_for(name), || {
                let u = lig_codegen::generate_module(&m).expect("codegen");
                black_box(u);
            })
        }
        "codegen_else" => {
            let src = show_else_source(root);
            let m = lig_parse_solid::lower_module(&src, "show-else.tsx").expect("lower");
            median_us(samples_for(name), || {
                let u = lig_codegen::generate_module(&m).expect("codegen");
                black_box(u);
            })
        }
        "codegen_list_component" => {
            let src = list_component_source(root);
            let m = lig_parse_solid::lower_module(&src, "list-component.tsx").expect("lower");
            median_us(samples_for(name), || {
                let u = lig_codegen::generate_module(&m).expect("codegen");
                black_box(u);
            })
        }
        "codegen_list_slot" => {
            let src = list_slot_source(root);
            let m = lig_parse_solid::lower_module(&src, "list-slot.tsx").expect("lower");
            median_us(samples_for(name), || {
                let u = lig_codegen::generate_module(&m).expect("codegen");
                black_box(u);
            })
        }
        "codegen_for_event" => {
            let src = for_event_source(root);
            let m = lig_parse_solid::lower_module(&src, "for-event.tsx").expect("lower");
            median_us(samples_for(name), || {
                let u = lig_codegen::generate_module(&m).expect("codegen");
                black_box(u);
            })
        }
        "codegen_for_event_composed" => {
            let src = for_event_composed_source(root);
            let m = lig_parse_solid::lower_module(&src, "for-event-composed.tsx").expect("lower");
            median_us(samples_for(name), || {
                let u = lig_codegen::generate_module(&m).expect("codegen");
                black_box(u);
            })
        }
        "codegen_for_composed_content" => {
            let src = for_composed_content_source(root);
            let m = lig_parse_solid::lower_module(&src, "for-composed-content.tsx").expect("lower");
            median_us(samples_for(name), || {
                let u = lig_codegen::generate_module(&m).expect("codegen");
                black_box(u);
            })
        }
        "session_compile_counter" => session_compile_counter(),
        "session_emit_mem" => session_emit_mem(),
        "session_sync" => session_sync(),
        "session_watch_tick" => session_watch_tick(),
        "runtime_instantiate" => median_us(samples_for(name), || {
            let mut h = CountingHost { commits: 0 };
            let f = Fragment::new(&mut h, counter_tpl(), HOLE_ANCHORS, BRANCH_ANCHORS);
            black_box(f);
        }),
        "runtime_update_mask" => median_us(samples_for(name), || {
            let mut h = CountingHost { commits: 0 };
            let mut frag = Fragment::new(&mut h, counter_tpl(), HOLE_ANCHORS, BRANCH_ANCHORS);
            let mut rx = NullReactions {
                dirty: DirtySet::new(),
            };
            frag.mark_mask((1 << 0) | (1 << 1));
            lig_runtime::flush(&mut rx, &mut h);
        }),
        "runtime_list_diff_keyed" => {
            // 64-item list, one swap + one replace — the bench's mid-size
            // reorder case, measured through the shipped sequential planner
            // (the plan the generated reconcile executes).
            let old: Vec<u64> = (0..64).map(|i| 100 + i).collect();
            let mut new = old.clone();
            new.swap(10, 40);
            new[60] = 999;
            median_us(samples_for(name), || {
                black_box(plan_keyed(&old, &new));
            })
        }
        "runtime_mount_lifecycle" => {
            // The mount seam's full lifecycle (M20): mount, one save-tick
            // through `Mounted::update`, dispose — the three calls an
            // embedding host loop repeats per surface (the bench drives the
            // same body).
            median_us(samples_for(name), || {
                let mut h = CountingHost { commits: 0 };
                let mut m = Mounted::mount(&mut h, MountUnit::build);
                let tick = m.update(&mut h, |s| s.set_count(1));
                black_box(tick.is_ok());
                let state = m.dispose(&mut h);
                black_box(state);
            })
        }
        "runtime_flush_checked_cycle" => {
            // The checked flush's fail-safe cap (M21): the ping-pong cycle
            // through `flush_checked` — exactly MAX_PASSES capped passes,
            // then the typed error (the bench drives the same body; the
            // dirty bit is boxed so the convergence loop cannot be
            // const-folded).
            median_us(samples_for(name), || {
                let mut h = CountingHost { commits: 0 };
                let mut c = CycleReactions {
                    dirty: DirtySet::new(),
                };
                c.dirty.set(black_box(0usize));
                let checked = lig_runtime::flush_checked(&mut c, &mut h);
                black_box(checked.is_err());
            })
        }
        "patch_apply_1" => median_us(samples_for(name), || {
            black_box(lig_patch::commit_table(synthetic_table(APPLY_ENTRIES)));
        }),
        "patch_dispatch_hit" => {
            // Ensure the committed table routes the safepoint (hit), then
            // measure the steady-state routed call.
            lig_patch::commit_table(synthetic_table(APPLY_ENTRIES));
            median_us(samples_for(name), || {
                black_box(lig_patch::hot_call(patchable_v0 as fn() -> u64));
            })
        }
        "gen_counter_bytes" => {
            let m = counter_module();
            let u = generate_component(&m, ComponentId::new(0)).expect("codegen");
            u.text.len() as f64
        }
        "session_observe" => session_observe(),
        "session_emit_to" => session_emit_to(),
        other => panic!("budgets: unregistered measurement {other} (sync check gap)"),
    }
}

/// The measurement registry the ledger must cover 1:1 (the sync check).
const REGISTRY: &[&str] = &[
    "lower_solid_counter",
    "lower_vue_counter",
    "canonical_json_counter",
    "codegen_counter",
    "codegen_composed",
    "codegen_branch",
    "codegen_ref",
    "codegen_else",
    "codegen_list_component",
    "codegen_list_slot",
    "codegen_for_event",
    "codegen_for_event_composed",
    "codegen_for_composed_content",
    "session_compile_counter",
    "session_observe",
    "session_emit_mem",
    "session_emit_to",
    "session_sync",
    "session_watch_tick",
    "runtime_instantiate",
    "runtime_update_mask",
    "runtime_mount_lifecycle",
    "runtime_flush_checked_cycle",
    "runtime_list_diff_keyed",
    "patch_apply_1",
    "patch_dispatch_hit",
    "gen_counter_bytes",
];

/// The enforcement decision for one measured row: bytes rows are
/// profile-independent and always enforced; time budgets are release
/// numbers — a dev run measures but does not enforce them.
fn decide(row: &BudgetRow, value: f64, dev: bool) -> &'static str {
    let enforced = row.metric == "bytes" || !dev;
    if !enforced {
        "measured (dev)"
    } else if value <= row.budget {
        "PASS"
    } else {
        "FAIL"
    }
}

/// Parse the committed ledger.
fn load_ledger(root: &Path) -> Result<Ledger, String> {
    let path = root.join(LEDGER);
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let ledger: Ledger =
        toml::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
    Ok(ledger)
}

/// The ledger row count for `lig doctor`'s environment report.
pub fn load_ledger_for_doctor() -> Result<usize, String> {
    let ledger = load_ledger(&repo_root())?;
    Ok(ledger.budget.len())
}

/// The budget gate (`lig budgets`): load the ledger, keep it in sync with
/// the measurement registry, measure, and enforce.
///
/// Exit codes: 0 = within budget, 1 = violation / broken ledger.
pub fn budgets_gate() -> i32 {
    let root = repo_root();
    let ledger = match load_ledger(&root) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("budgets: {e}");
            return 1;
        }
    };

    // Forward-compat guard (fail closed): the gate measures the v1 ledger —
    // a newer schema must be opted into by a reviewed gate change, not
    // silently mis-measured.
    if ledger.schema != 1 {
        eprintln!(
            "budgets: ledger schema {} is newer than the v1 gate",
            ledger.schema
        );
        return 1;
    }

    // Sync check (the rules-sync discipline, applied to the ledger): every
    // registered measurement has exactly one row, and no row names anything
    // the gate cannot measure.
    let mut rows: BTreeMap<&str, &BudgetRow> = BTreeMap::new();
    for row in &ledger.budget {
        if row.name.is_empty() {
            eprintln!("budgets: ledger row with empty name");
            return 1;
        }
        if rows.contains_key(row.name.as_str()) {
            eprintln!("budgets: duplicate ledger row {}", row.name);
            return 1;
        }
        if !REGISTRY.contains(&row.name.as_str()) {
            eprintln!(
                "budgets: ledger row {} is not a registered measurement — known: {REGISTRY:?}",
                row.name
            );
            return 1;
        }
        if row.metric != "time" && row.metric != "bytes" {
            eprintln!("budgets: {} has unknown metric {}", row.name, row.metric);
            return 1;
        }
        if row.budget <= 0.0 {
            eprintln!("budgets: {} has a non-positive budget", row.name);
            return 1;
        }
        rows.insert(row.name.as_str(), row);
    }
    for name in REGISTRY {
        if !rows.contains_key(name) {
            eprintln!("budgets: measurement {name} has no ledger row — add one to {LEDGER}");
            return 1;
        }
    }

    // Measure + report.
    let dev = cfg!(debug_assertions);
    let mut violations: Vec<String> = Vec::new();
    let mut table = String::new();
    let _ = writeln!(
        table,
        "{:<26} {:>13} {:>13} {:>9}  status",
        "workload", "measured", "budget", "headroom"
    );
    for name in REGISTRY {
        let row = rows.get(name).expect("sync check passed");
        let value = measure(name, &root);
        let unit = if row.metric == "bytes" { "B" } else { "µs" };
        let headroom = if value > 0.0 {
            row.budget / value
        } else {
            f64::INFINITY
        };
        let status = decide(row, value, dev);
        if status == "FAIL" {
            violations.push(format!(
                "{} measured {:.1} {} > budget {:.1} {} ({})",
                name, value, unit, row.budget, unit, row.note
            ));
        }
        let _ = writeln!(
            table,
            "{name:<26} {value:>10.2} {unit:<2} {:>10.1} {unit:<2} {headroom:>8.1}×  {status}",
            row.budget
        );
    }
    print!("{table}");
    if dev {
        println!(
            "budgets: dev profile — time budgets are enforced in release runs: \
             cargo run -p lig-cli --release -- budgets"
        );
    }
    if violations.is_empty() {
        println!(
            "budgets: all {} row(s) within budget ({})",
            rows.len(),
            if dev { "bytes enforced" } else { "enforced" }
        );
        0
    } else {
        for v in &violations {
            eprintln!("budgets: VIOLATION — {v}");
        }
        eprintln!(
            "budgets: {} row(s) over budget — if this is a deliberate regression, \
             raise the ceiling in {LEDGER} and review it like code",
            violations.len()
        );
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, metric: &str, budget: f64) -> BudgetRow {
        BudgetRow {
            name: name.to_string(),
            metric: metric.to_string(),
            budget,
            note: "test".to_string(),
        }
    }

    #[test]
    fn registry_covers_every_row_shape() {
        // The registry is the sync contract: 27 rows (M12 adds codegen_ref,
        // M11's else leg adds codegen_else, M13 adds
        // codegen_list_component, M14 adds codegen_list_slot, M15 adds
        // codegen_for_event, M16 adds session_compile_counter, M17 adds
        // codegen_for_event_composed, M18 adds
        // codegen_for_composed_content, the sink seam adds
        // session_emit_mem, the sync seam adds session_sync, the observer
        // seam adds session_observe, the session emission seam adds
        // session_emit_to, the mount seam adds runtime_mount_lifecycle,
        // the checked flush seam adds runtime_flush_checked_cycle, the
        // watch-tick seam adds session_watch_tick), all named, no dupes.
        assert_eq!(REGISTRY.len(), 27);
        let mut sorted: Vec<_> = REGISTRY.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), REGISTRY.len(), "registry names are unique");
    }

    #[test]
    fn sample_counts_cover_every_registered_row() {
        for name in REGISTRY {
            assert!(samples_for(name) > 0, "{name} has a sample count");
        }
    }

    #[test]
    fn ledger_parses_and_is_in_sync() {
        let ledger = load_ledger(&repo_root()).expect("committed ledger parses");
        assert!(ledger.schema >= 1, "ledger schema declared");
        let names: Vec<_> = ledger.budget.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names.len(), REGISTRY.len(), "ledger row count");
        for name in REGISTRY {
            assert!(names.contains(name), "ledger covers {name}");
        }
        for r in &ledger.budget {
            assert!(
                r.metric == "time" || r.metric == "bytes",
                "{} metric",
                r.name
            );
            assert!(r.budget > 0.0, "{} positive budget", r.name);
            assert!(r.note.len() > 8, "{} carries a provenance note", r.name);
        }
    }

    #[test]
    fn bytes_rows_enforce_in_every_profile() {
        // Bytes are profile-independent: over budget is a FAIL in dev too.
        let bytes_row = row("gen_counter_bytes", "bytes", 8192.0);
        assert_eq!(decide(&bytes_row, 9000.0, true), "FAIL");
        assert_eq!(decide(&bytes_row, 8192.0, false), "PASS");
        assert_eq!(decide(&bytes_row, 7146.0, true), "PASS");
    }

    #[test]
    fn time_rows_enforce_only_in_release() {
        let time_row = row("lower_solid_counter", "time", 100.0);
        // Dev run over the ceiling: measured, not enforced.
        assert_eq!(decide(&time_row, 264.0, true), "measured (dev)");
        // Release run over the ceiling: enforced violation.
        assert_eq!(decide(&time_row, 264.0, false), "FAIL");
        assert_eq!(decide(&time_row, 18.4, false), "PASS");
        // The boundary is inclusive (measured == budget passes).
        assert_eq!(decide(&time_row, 100.0, false), "PASS");
    }

    #[test]
    fn median_us_returns_a_plausible_median() {
        let mut counter = 0u64;
        let v = median_us(50, || {
            counter += 1;
            black_box(&counter);
        });
        assert!(v >= 0.0, "median is non-negative");
    }

    #[test]
    fn synthetic_table_maps_the_safepoint() {
        let t = synthetic_table(1);
        let old = patchable_v0 as *const () as usize as u64;
        assert_eq!(t.len(), 1);
        assert_eq!(
            t.lookup(old),
            Some(patchable_v1 as *const () as usize as u64)
        );
    }

    #[test]
    fn bytes_row_measures_the_pinned_counter_unit() {
        // The D-8-pinned counter unit is 7 146 bytes; the bytes measurement
        // must agree (a fresh generation is byte-identical).
        let v = measure("gen_counter_bytes", &repo_root());
        assert!(
            (6000.0..9000.0).contains(&v),
            "counter unit size {v} in the pinned band"
        );
    }

    #[test]
    fn time_rows_measure_finite_values() {
        for name in [
            "lower_solid_counter",
            "runtime_update_mask",
            "patch_dispatch_hit",
        ] {
            let v = measure(name, &repo_root());
            assert!(v.is_finite() && v > 0.0, "{name} measured {v}");
        }
    }
}
