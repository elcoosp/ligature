//! Session benchmark (§14.2, M16): one full digest-gated compile pass over
//! the M0 reference workload — set_source + `compile` walks lower → admit →
//! the build-order cycle check → codegen, the exact pass `lig build` drives
//! per unit. The memo-reuse half of the session is pinned by the D-9 gate
//! (`lig incremental`), not by a throughput number. The emission-target seam
//! (the `ArtifactSink` trait) adds the in-memory emission workload: the same
//! cold pass plus `emit_units_to` into a fresh `MemSink` — what an embedded
//! consumer pays per pass with no filesystem involved. The watch-sync seam
//! adds the tick workload: one `sync` diff over an 8-source snapshot with
//! one edit — the call the `lig watch` loop runs per tick. The observer seam
//! adds the observe workload: the same cold pass with the stage events
//! delivered to a counting observer — what an embedding consumer pays for
//! the programmatic event stream on top of the plain pass. The watch-tick
//! seam (M23) adds the full-tick workload: sync + compile + fail-closed
//! emission over the same 8-source snapshot — the whole loop body, once.

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use lig_admission::Profile;
use lig_incremental::Session;
use lig_incremental::emit::{MemSink, emit_units_to};
use lig_incremental::observe::{SessionObserver, StageEvent};
use lig_parse_solid::tests_support::COUNTER_TSX;
use std::hint::black_box;

/// A counting observer — the observe workload's event sink: every stage
/// event counted, none retained (the cheapest useful observer).
#[derive(Default)]
struct CountingObserver {
    stages: usize,
}

impl SessionObserver for CountingObserver {
    fn on_stage(&mut self, _event: &StageEvent) {
        self.stages += 1;
    }
}

/// Time a cold pass: a fresh session, one source, one compile — every stage
/// recomputes (the digest gate has nothing to reuse yet).
fn bench_session_compile(c: &mut Criterion) {
    c.bench_function("session/compile_counter", |b| {
        b.iter(|| {
            let mut s = Session::new();
            s.set_source("counter.tsx", COUNTER_TSX);
            black_box(s.compile(Profile::Layout));
        });
    });
}

/// Time a cold pass plus an in-memory emission (the sink seam): set_source +
/// compile + `emit_units_to` into a fresh `MemSink` — sourcemap verify, IR
/// JSON, the merged map, the manifest, and the state file all ride the pass.
fn bench_session_emit_mem(c: &mut Criterion) {
    c.bench_function("session/emit_mem", |b| {
        b.iter(|| {
            let mut s = Session::new();
            s.set_source("counter.tsx", COUNTER_TSX);
            s.compile(Profile::Layout);
            let units = s.units();
            let mut sink = MemSink::new();
            black_box(emit_units_to(&mut sink, &units, "taffy", false).expect("mem emit"));
        });
    });
}

/// Time a cold pass under observation (the observer seam): a fresh session
/// with a counting observer installed, one source, one `compile_with` —
/// every stage recomputes AND reports its event, the programmatic view of
/// the same pass `session/compile_counter` drives unobserved.
fn bench_session_observe(c: &mut Criterion) {
    c.bench_function("session/observe", |b| {
        b.iter(|| {
            let mut observer = CountingObserver::default();
            let mut s = Session::new();
            s.set_source("counter.tsx", COUNTER_TSX);
            black_box(s.compile_with(Profile::Layout, &mut observer));
            black_box(observer.stages);
        });
    });
}

/// Time one watch tick (the sync seam): a prebuilt 8-source session diffs a
/// fresh snapshot with one edit — 8 digest compares, one registration, and
/// the added/edited/removed report the tick acts on. The session is rebuilt
/// per iteration so every tick sees the same pre-tick state.
fn bench_session_sync(c: &mut Criterion) {
    let edited = COUNTER_TSX.replace("Add", "Add2");
    c.bench_function("session/sync", |b| {
        b.iter_batched(
            || {
                let mut s = Session::new();
                for i in 0..8 {
                    s.set_source(format!("unit{i}.tsx"), COUNTER_TSX);
                }
                s
            },
            |mut s| {
                let fresh: Vec<(std::path::PathBuf, &str)> = (0..8)
                    .map(|i| {
                        if i == 3 {
                            (
                                std::path::PathBuf::from(format!("unit{i}.tsx")),
                                edited.as_str(),
                            )
                        } else {
                            (
                                std::path::PathBuf::from(format!("unit{i}.tsx")),
                                COUNTER_TSX,
                            )
                        }
                    })
                    .collect();
                black_box(s.sync(fresh, &[]))
            },
            BatchSize::SmallInput,
        );
    });
}

/// Time the session emission seam (M19): a fresh session, one source, one
/// compile, one `Session::emit_to` into a fresh `MemSink` — the one-call
/// front of the sink seam, with the BR-1 admission gate and the profile
/// ledger name riding the call.
fn bench_session_emit_to(c: &mut Criterion) {
    c.bench_function("session/emit_to", |b| {
        b.iter(|| {
            let mut s = Session::new();
            s.set_source("counter.tsx", COUNTER_TSX);
            s.compile(Profile::Layout);
            let mut sink = MemSink::new();
            black_box(
                s.emit_to(&mut sink, Profile::Layout, false)
                    .expect("session emit"),
            );
        });
    });
}

/// Time one full watch tick (the watch-tick seam, M23): a prebuilt 8-source
/// session takes one `Session::tick` over a fresh snapshot with one edit —
/// the sync diff, the digest-gated recompile of exactly the edited unit, and
/// the fail-closed in-memory emission, the whole `lig watch` loop body as
/// one call. The session is rebuilt per iteration so every tick sees the
/// same pre-tick state.
fn bench_session_watch_tick(c: &mut Criterion) {
    let edited = COUNTER_TSX.replace("Add", "Add2");
    c.bench_function("session/watch_tick", |b| {
        b.iter_batched(
            || {
                let mut s = Session::new();
                for i in 0..8 {
                    s.set_source(format!("unit{i}.tsx"), COUNTER_TSX);
                }
                s.compile(Profile::Layout);
                s
            },
            |mut s| {
                let fresh: Vec<(std::path::PathBuf, &str)> = (0..8)
                    .map(|i| {
                        if i == 3 {
                            (
                                std::path::PathBuf::from(format!("unit{i}.tsx")),
                                edited.as_str(),
                            )
                        } else {
                            (
                                std::path::PathBuf::from(format!("unit{i}.tsx")),
                                COUNTER_TSX,
                            )
                        }
                    })
                    .collect();
                let mut sink = MemSink::new();
                black_box(
                    s.tick(fresh, &[], &mut sink, Profile::Layout, true)
                        .expect("the tick passes"),
                )
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_session_compile,
    bench_session_observe,
    bench_session_emit_mem,
    bench_session_emit_to,
    bench_session_sync,
    bench_session_watch_tick
);
criterion_main!(benches);
