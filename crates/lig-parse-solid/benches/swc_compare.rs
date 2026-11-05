//! Oxc vs SWC re-benchmark on our own workload (§2.5, §14.2).
//!
//! The architecture doc keeps Oxc for its arena allocation, semantic layer,
//! sourcemaps and diagnostics — not for a third-party benchmark claim. This
//! bench re-measures the parse substrate once at M0 on the reference
//! workload, as a gate rather than a citation; the result is recorded in
//! `docs/benchmarks/oxc-vs-swc.md`.
//!
//! Both parsers run TSX on the same source. Oxc parses into its arena;
//! SWC into its owning AST. Feature-gated (`bench-swc`) so SWC is never
//! linked into compiler crates (§15).

use criterion::{Criterion, criterion_group, criterion_main};
use lig_parse_solid::tests_support::COUNTER_TSX;
use std::hint::black_box;

/// Oxc: arena parse of the reference workload.
fn bench_oxc_parse(c: &mut Criterion) {
    c.bench_function("swc_compare/oxc_parse", |b| {
        b.iter(|| {
            let allocator = oxc_allocator::Allocator::default();
            black_box(
                oxc_parser::Parser::new(&allocator, COUNTER_TSX, oxc_span::SourceType::tsx())
                    .parse(),
            );
        });
    });
}

/// SWC: owning parse of the same workload (fresh parser per iteration, as
/// SWC's parser borrows its input for the AST's lifetime).
fn bench_swc_parse(c: &mut Criterion) {
    use swc_ecma_parser::{Parser, StringInput, Syntax, TsSyntax};

    let syntax = Syntax::Typescript(TsSyntax {
        tsx: true,
        ..TsSyntax::default()
    });
    c.bench_function("swc_compare/swc_parse", |b| {
        b.iter(|| {
            let input = StringInput::new(
                COUNTER_TSX,
                swc_common::BytePos(0),
                swc_common::BytePos(COUNTER_TSX.len() as u32),
            );
            let mut parser = Parser::new(syntax, input, None);
            let _ = black_box(parser.parse_module());
        });
    });
}

criterion_group!(benches, bench_oxc_parse, bench_swc_parse);
criterion_main!(benches);
