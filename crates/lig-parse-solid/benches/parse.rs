//! Parse + lower benchmarks (§14.2: `parse`, `view_ir`) on the M0 reference
//! workload. The SWC comparison lives in `swc_compare` (feature `bench-swc`),
//! run once at M0 and recorded in docs/benchmarks.

use criterion::{Criterion, criterion_group, criterion_main};
use lig_parse_solid::tests_support::COUNTER_TSX;

fn bench_parse(c: &mut Criterion) {
    c.bench_function("parse/counter_tsx", |b| {
        b.iter(|| lig_parse_solid::lower_module(COUNTER_TSX, "counter.tsx").expect("lower"));
    });
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
