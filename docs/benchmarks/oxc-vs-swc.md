# Oxc vs SWC re-benchmark — M0 (§2.5, §14.2)

The architecture keeps **Oxc** for its arena allocation, semantic layer,
sourcemap and diagnostics support — not because of a third-party benchmark
claim. As required by §2.5, the claim "Oxc ≥ 3× faster than SWC at parsing"
was **re-measured on our own workload** rather than cited.

## Method

- Workload: the M0 reference fixture `COUNTER_TSX` (counter component, the
  same source every crate gates on).
- Harness: criterion benches in `crates/lig-parse-solid/benches/swc_compare.rs`
  (feature `bench-swc`; SWC is never linked into compiler crates, §15).
- Oxc: `oxc_parser 0.157.0`, arena parse into `oxc_allocator` (fresh allocator
  per iteration, as the AST borrows it).
- SWC: `swc_ecma_parser 47.0.0` + `swc_common 27.0.0`, TSX syntax, fresh
  parser per iteration (SWC's AST owns its input).
- Both parse **TSX** on identical byte input; both include per-iteration
  input setup; neither includes lowering.

## Result (2026-11-05, Linux x86_64, criterion, warm-up 0.3 s / measure 1 s)

| Bench | Median |
| --- | --- |
| `swc_compare/oxc_parse` | **1.86 µs** |
| `swc_compare/swc_parse` | **6.11 µs** |
| ratio | Oxc ≈ **3.3× faster** |

The §2.5 claim is confirmed on our workload. Decision: **keep Oxc** — the
ratio is on the promised side of the gate, and the structural reasons
(arena + semantic + sourcemap + diagnostics) apply regardless.

## Notes

- Absolute numbers are for this runner only; the ratio is the gate.
- Re-run locally: `cargo bench -p lig-parse-solid --features bench-swc --bench swc_compare`.
- CI runs this bench in the `bench-swc` job (`.github/workflows/ci.yml`).
