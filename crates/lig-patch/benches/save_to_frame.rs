//! S2 spike benchmark: the in-process cost of the hot-patch protocol (§11.3).
//!
//! What is measured here is the *runtime* half of save→frame:
//!
//! 1. `patch-apply/commit` — building and atomically committing a jump table
//!    of N entries (the apply step's runtime cost, minus dylib loading).
//! 2. `patch-dispatch/miss` — a safepoint call before any patch is committed
//!    (the fat-build baseline: one atomic load + one BTreeMap miss).
//! 3. `patch-dispatch/hit` — a safepoint call routed through the table into a
//!    different (same-signature) function — the steady-state patched call.
//! 4. `patch-surface/dispatch_hit` — the same route at the mounted dispatch
//!    shape (`hot_dispatch`: the surface bridge's safepoint, args and the
//!    checked `Result` riding along). Isolation workload, unregistered by
//!    design (the M22 precedent): the ledger's `patch_dispatch_hit` row
//!    bounds the raw route cost and `runtime_dispatch_checked` bounds the
//!    mounted tick, so the surface adds no end-to-end row of its own.
//!
//! The *end-to-end* save→frame budget (tip-crate recompile + relink + apply +
//! first routed frame) is measured by the `lig patch-spike` gate, which
//! enforces the < 1 s acceptance criterion (§17 M0, S2) on Linux and macOS.
//!
//! Note on leaks: `commit_table` intentionally leaks each committed table
//! (never-dlclose / never-free discipline, §2.4). Benchmark iterations leak
//! one small boxed table each — bounded by criterion's iteration count and
//! irrelevant outside `cargo bench`.

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use lig_patch::{JumpTable, commit_table, hot_call};
use std::collections::BTreeMap;
use std::hint::black_box;

/// The un-patched implementation the safepoint falls back to on a miss.
fn patchable_v0() -> u64 {
    0
}

/// A distinct same-signature body standing in for freshly patched code.
/// Reachable only through the routed (transmuted) call — the reachability
/// analysis sees the route, so no dead-code suppression is needed.
fn patchable_v1() -> u64 {
    1
}

/// Build a synthetic old→new table of `n` entries around two real functions.
fn synthetic_table(n: usize) -> JumpTable {
    let old = patchable_v0 as *const () as usize as u64;
    let new = patchable_v1 as *const () as usize as u64;
    let map: BTreeMap<u64, u64> = (0..n as u64)
        .map(|i| {
            if i == 0 {
                (old, new)
            } else {
                (old.wrapping_add(i * 0x10), new.wrapping_add(i * 0x10))
            }
        })
        .collect();
    JumpTable::new(map)
}

fn bench_apply(c: &mut Criterion) {
    let mut group = c.benchmark_group("patch-apply");
    for n in [1, 16, 256] {
        group.throughput(criterion::Throughput::Elements(n as u64));
        group.bench_with_input(criterion::BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || synthetic_table(n),
                |table| {
                    // The apply step's runtime half: one allocation + one
                    // atomic pointer swap (AcqRel), per committed table.
                    black_box(lig_patch::commit_table(black_box(table)));
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_dispatch(c: &mut Criterion) {
    let mut group = c.benchmark_group("patch-dispatch");

    // Baseline: nothing committed — every safepoint call is a miss.
    group.bench_function("miss", |b| {
        b.iter(|| black_box(hot_call(black_box(patchable_v0) as fn() -> u64)));
    });

    // Steady state: the table routes the safepoint into the patched body.
    commit_table(synthetic_table(1));
    assert_eq!(lig_patch::current_table().map_or(0, JumpTable::len), 1);
    assert_eq!(
        hot_call(patchable_v0 as fn() -> u64),
        1,
        "routed call must hit v1"
    );
    group.bench_function("hit", |b| {
        b.iter(|| black_box(hot_call(black_box(patchable_v0) as fn() -> u64)));
    });

    group.finish();
}

fn bench_surface(c: &mut Criterion) {
    let mut group = c.benchmark_group("patch-surface");

    // Steady state: the mounted dispatch rides the table route (the bridge
    // safepoint), then the checked body runs — the cost a hot-patched
    // mounted surface pays per event after the route lands. No ledger row:
    // `patch_dispatch_hit` bounds the route and `runtime_dispatch_checked`
    // bounds the mounted tick (the isolation-bench precedent).
    commit_table(synthetic_table(1));
    group.bench_function("dispatch_hit", |b| {
        let ev = lig_runtime::EvData::new();
        let mut unit = SurfaceUnit {
            dirty: lig_runtime::DirtySet::new(),
        };
        b.iter(|| {
            black_box(lig_patch::hot_dispatch(
                surface_v0 as lig_patch::DispatchFn<SurfaceUnit, lig_runtime::EvData>,
                black_box(&mut unit),
                &mut NoopHost,
                &ev,
            ))
        });
    });
    group.finish();
}

/// The mounted dispatch's measured unit: an empty schedule, so the checked
/// body converges in one pass — the cost after the route lands.
struct SurfaceUnit {
    dirty: lig_runtime::DirtySet,
}

static SURFACE_SCHEDULE: lig_runtime::Schedule = lig_runtime::Schedule {
    order: &[],
    writes: &[],
    writes_hi: &[],
};

impl lig_runtime::Reactions for SurfaceUnit {
    fn schedule(&self) -> &'static lig_runtime::Schedule {
        &SURFACE_SCHEDULE
    }
    fn dirty(&mut self) -> &mut lig_runtime::DirtySet {
        &mut self.dirty
    }
    fn run(&mut self, _h: &mut dyn lig_runtime::Host, _r: u8) {}
}

/// The un-patched dispatch body the safepoint falls back to on a miss (and
/// the symbol the synthetic table routes): the mounted glue's shape —
/// state, host, event — returning the checked front's `Result` (the body
/// the generated `fire_N` glue emits after its event capture).
fn surface_v0(
    state: &mut SurfaceUnit,
    host: &mut dyn lig_runtime::Host,
    _ev: &lig_runtime::EvData,
) -> Result<(), lig_runtime::CycleError> {
    lig_runtime::flush_checked(state, host)
}

/// A no-op host: the surface dispatch carries the commit-stream parameter
/// even where the measured body touches no node.
struct NoopHost;

impl lig_runtime::Host for NoopHost {
    fn create(&mut self, _node: lig_runtime::NodeId, _desc: lig_runtime::TemplateDesc<'_>) {}
    fn destroy(&mut self, _node: lig_runtime::NodeId) {}
    fn set_text(&mut self, _node: lig_runtime::NodeId, _text: &str) {}
    fn set_attr(
        &mut self,
        _node: lig_runtime::NodeId,
        _key: &str,
        _val: Option<lig_runtime::AttrVal<'_>>,
    ) {
    }
    fn set_class(&mut self, _node: lig_runtime::NodeId, _class: Option<&str>) {}
    fn set_style(&mut self, _node: lig_runtime::NodeId, _s: lig_runtime::StyleConstId) {}
    fn set_style_dyn(
        &mut self,
        _node: lig_runtime::NodeId,
        _props: &[(u32, lig_runtime::PropVal<'_>)],
    ) {
    }
    fn set_html(&mut self, _node: lig_runtime::NodeId, _h: &str) {}
    fn insert(&mut self, _p: lig_runtime::NodeId, _c: lig_runtime::NodeId, _i: u32) {}
    fn remove(&mut self, _p: lig_runtime::NodeId, _c: lig_runtime::NodeId) {}
    fn move_(&mut self, _p: lig_runtime::NodeId, _c: lig_runtime::NodeId, _i: u32) {}
    fn attach(
        &mut self,
        _n: lig_runtime::NodeId,
        _e: lig_runtime::EventKind,
        _h: lig_runtime::HandlerId,
        _d: bool,
    ) {
    }
    fn detach(
        &mut self,
        _n: lig_runtime::NodeId,
        _e: lig_runtime::EventKind,
        _h: lig_runtime::HandlerId,
    ) {
    }
    fn on_mount(&mut self, _n: lig_runtime::NodeId, _h: lig_runtime::HookId) {}
    fn on_unmount(&mut self, _n: lig_runtime::NodeId, _h: lig_runtime::HookId) {}
}

criterion_group!(benches, bench_apply, bench_dispatch, bench_surface);
criterion_main!(benches);
