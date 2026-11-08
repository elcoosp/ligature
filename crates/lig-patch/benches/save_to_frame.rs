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

criterion_group!(benches, bench_apply, bench_dispatch);
criterion_main!(benches);
