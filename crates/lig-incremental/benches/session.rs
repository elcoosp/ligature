//! Session benchmark (§14.2, M16): one full digest-gated compile pass over
//! the M0 reference workload — set_source + `compile` walks lower → admit →
//! the build-order cycle check → codegen, the exact pass `lig build` drives
//! per unit. The memo-reuse half of the session is pinned by the D-9 gate
//! (`lig incremental`), not by a throughput number.

use criterion::{Criterion, criterion_group, criterion_main};
use lig_admission::Profile;
use lig_incremental::Session;
use lig_parse_solid::tests_support::COUNTER_TSX;
use std::hint::black_box;

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

criterion_group!(benches, bench_session_compile);
criterion_main!(benches);
