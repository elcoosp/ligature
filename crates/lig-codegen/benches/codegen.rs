//! Codegen benchmark (§14.2 `codegen`): `quote!` → `syn::File` →
//! `prettyplease` on the M0 reference workload. Byte-reproducibility (D-8) is
//! gated separately by `lig reproducibility`; this measures throughput.

use criterion::{Criterion, criterion_group, criterion_main};
use lig_codegen::generate_component;
use lig_core::id::ComponentId;
use lig_parse_solid::tests_support::COUNTER_TSX;
use std::hint::black_box;

/// Lower the counter fixture once, then time repeated codegen of the same IR.
fn bench_codegen(c: &mut Criterion) {
    let module = lig_parse_solid::lower_module(COUNTER_TSX, "counter.tsx").expect("counter lowers");
    c.bench_function("codegen/counter", |b| {
        b.iter(|| {
            black_box(generate_component(black_box(&module), ComponentId::new(0)))
                .expect("counter codegens");
        });
    });
}

/// Lower the composed slot-content fixture once (M9: component-in-content +
/// nested outlets — the ctx classification walk, the recursive mount walk,
/// and the grandchild inserts ride the module pipeline), then time repeated
/// codegen of the same IR.
fn bench_codegen_composed(c: &mut Criterion) {
    let src = include_str!("../../../fixtures/conformance/slots-component/input.solid.tsx");
    let module = lig_parse_solid::lower_module(src, "slots-component.tsx").expect("fixture lowers");
    c.bench_function("codegen/composed", |b| {
        b.iter(|| {
            black_box(lig_codegen::generate_module(black_box(&module))).expect("composed codegens");
        });
    });
}

criterion_group!(benches, bench_codegen, bench_codegen_composed);
criterion_main!(benches);
