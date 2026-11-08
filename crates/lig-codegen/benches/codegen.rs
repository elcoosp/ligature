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

/// Lower the branch-assembly fixture once (M10: per-mount component identity
/// plus branch-time slot assembly — the Option-field rebuild/dispose arms,
/// the assembly walk emission, and the widened mount masks ride the module
/// pipeline), then time repeated codegen of the same IR.
fn bench_codegen_branch(c: &mut Criterion) {
    let src = include_str!("../../../fixtures/conformance/slots-branch-component/input.solid.tsx");
    let module =
        lig_parse_solid::lower_module(src, "slots-branch-component.tsx").expect("fixture lowers");
    c.bench_function("codegen/branch", |b| {
        b.iter(|| {
            black_box(lig_codegen::generate_module(black_box(&module))).expect("branch codegens");
        });
    });
}

/// Lower the template-ref fixture once (M12: the Ref capture contract —
/// the ref-field declaration, the sentinel init, and the build-body capture
/// ride the module pipeline), then time repeated codegen of the same IR.
fn bench_codegen_ref(c: &mut Criterion) {
    let src = include_str!("../../../fixtures/conformance/template-ref/input.solid.tsx");
    let module = lig_parse_solid::lower_module(src, "template-ref.tsx").expect("fixture lowers");
    c.bench_function("codegen/ref", |b| {
        b.iter(|| {
            black_box(lig_codegen::generate_module(black_box(&module))).expect("ref codegens");
        });
    });
}

/// Lower the else-leg fixture once (M11: the else leg — the tracked-leg
/// mounting arms, the per-leg assembly/marking emission, and the leg-guarded
/// captures ride the module pipeline), then time repeated codegen of the
/// same IR.
fn bench_codegen_else(c: &mut Criterion) {
    let src = include_str!("../../../fixtures/conformance/show-else/input.solid.tsx");
    let module = lig_parse_solid::lower_module(src, "show-else.tsx").expect("fixture lowers");
    c.bench_function("codegen/else", |b| {
        b.iter(|| {
            black_box(lig_codegen::generate_module(black_box(&module))).expect("else codegens");
        });
    });
}

criterion_group!(
    benches,
    bench_codegen,
    bench_codegen_composed,
    bench_codegen_branch,
    bench_codegen_ref,
    bench_codegen_else
);
criterion_main!(benches);
