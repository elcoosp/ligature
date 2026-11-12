//! §14.2 workload `sourcemap/link_table_cycle` (M22): the full link-table
//! consumer cycle on the M0 reference workload — the emitter records the
//! §9.4 table post-unparse, then the consumer builds `src→IR`, `IR→Rust`
//! (from the recorded offsets, no scanning), chains them, and verifies.
//! This is the per-unit map pipeline `emit_units_to` runs before every
//! artifact write. The in-process gate (`lig budgets`) re-measures the same
//! cycle inside its `session_emit_to` / `session_emit_mem` rows, so the
//! ledger carries the end-to-end number; this bench isolates the map cycle.
//! The position resolver (M24) adds the `resolve_merged` workload: one
//! `resolve` over the merged map per iteration — the query `lig resolve`
//! and embedded stack-trace mappers run per position.

use criterion::{Criterion, criterion_group, criterion_main};
use lig_codegen::generate_component;
use lig_core::id::ComponentId;
use lig_parse_solid::tests_support::COUNTER_TSX;
use std::hint::black_box;

/// Lower the counter fixture once, then time the full link-table cycle:
/// codegen (recording the table) → the three maps → the NFR-6 verify.
fn bench_link_table_cycle(c: &mut Criterion) {
    let module = lig_parse_solid::lower_module(COUNTER_TSX, "counter.tsx").expect("counter lowers");
    let src = COUNTER_TSX;
    c.bench_function("sourcemap/link_table_cycle", |b| {
        b.iter(|| {
            let unit = generate_component(black_box(&module), ComponentId::new(0))
                .expect("counter codegens");
            let src_map = lig_sourcemap::src_to_ir(black_box(&module), black_box(src));
            let ir_map = lig_sourcemap::ir_to_rust(black_box(&module), &unit.text, &unit.links)
                .expect("the recorded table builds the map");
            let merged = lig_sourcemap::chain(&src_map, &ir_map);
            let stats =
                lig_sourcemap::verify(black_box(&module), black_box(src), &unit.text, &unit.links)
                    .expect("the cycle verifies");
            black_box((merged.get_tokens().len(), stats.reactions_linked));
        });
    });
}

/// Time one position resolution (the resolve seam, M24): the merged map is
/// built once, then `resolve` runs per iteration — the lookup-table build
/// and the binary search the `lig resolve` command pays per query.
fn bench_resolve_merged(c: &mut Criterion) {
    let module = lig_parse_solid::lower_module(COUNTER_TSX, "counter.tsx").expect("counter lowers");
    let src = COUNTER_TSX;
    let unit = generate_component(&module, ComponentId::new(0)).expect("counter codegens");
    let src_map = lig_sourcemap::src_to_ir(&module, src);
    let ir_map = lig_sourcemap::ir_to_rust(&module, &unit.text, &unit.links)
        .expect("the recorded table builds the map");
    let merged = lig_sourcemap::chain(&src_map, &ir_map);
    let token = merged
        .get_tokens()
        .next()
        .expect("the merged map carries tokens");
    let (line, col) = (token.get_dst_line(), token.get_dst_col());
    c.bench_function("sourcemap/resolve_merged", |b| {
        b.iter(|| {
            black_box(lig_sourcemap::resolve(black_box(&merged), line, col).expect("resolves"))
        });
    });
}

criterion_group!(benches, bench_link_table_cycle, bench_resolve_merged);
criterion_main!(benches);
