# M22 milestone report — the recorded link table

Date: 2026-11-12 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the designed §9.4 seam, populated)

M19 and M21 both deferred the same seam: `lig-sourcemap`'s `IrRustLink`
sat defined-but-unpopulated, and the `IR→Rust` map positions were
re-derived by text-scanning in the consumer (four `generated.find(...)`
passes per map build) — the documented contract ("codegen emits the
markers itself, no text scanning in consumers") held only on paper. M22
makes the emitter record what it emits:

- **`IrLink { ir_ordinal, generated_offset }` + `GenLinks.links`**
  (`lig_codegen`): after `prettyplease::unparse` — the one point where
  exact offsets exist — the producer locates every reaction/template
  marker it just wrote and records the table, sorted by offset, one entry
  per marker. `DYNAMIC_BASE` (the combined ordinal space) moves to the
  producer; `lig_sourcemap` re-exports it and `IrRustLink` is the stable
  consumer-facing alias.
- **The emitter fails on its own table**: `record_link_offsets` is
  fail-closed — a marker that did not reach the final text fails codegen
  (`CodegenError::LinkTable`, rendered by the session's `CodegenFailure`
  with internal-bug help-text). Failure attribution moves from emission
  time to the emitter.
- **The consumer never scans**: `ir_to_rust` builds every reaction and
  template token from the recorded offsets; the one text touch left is a
  `starts_with` AT a recorded offset — the fail-closed anchor check.
  A table-less `GenLinks` fails on the module-anchored staleness guard
  instead of silently producing an empty map.
- **The seam fails typed**: `SourceMapError`
  (`LinkMissing { ordinal }` / `Anchor { ordinal, offset }` /
  `RoundTrip` / `Resolve { node, detail }`) replaces the bare
  `Result<_, String>` across `ir_to_rust`/`verify`/`round_trip` — the
  CycleError pattern (typed data, module-path code constant
  `ligature::sourcemap`, `core::error::Error`, no `miette`; the crate
  keeps its dependency floor). `emit.rs` maps the `Display` into
  `EmitError::contract` unchanged.
- **The verifier anchors, not just finds**: `verify` re-checks every
  recorded offset — reaction markers AND template statics (new; the old
  check only confirmed marker presence) — against the generated text.
- **Bench**: `sourcemap/link_table_cycle` (the per-unit map pipeline:
  codegen + record → three maps → verify) — release median ≈666 µs on the
  counter; the gate's `session_emit_to`/`session_emit_mem` rows already
  re-measure the cycle end-to-end (both PASS at ≈0.87 ms against
  4.5–4.7 ms ceilings).

## Design decisions (ADR-0035)

- One `find` pass survives — in the producer, post-unparse (offsets do
  not exist before prettyplease reflows the tokens). The design point is
  the consumer contract, and it holds.
- `IrRustLink` becomes an alias of the producer-side `IrLink` rather than
  a second struct: two definitions of the §9.4 shape would drift.
- No new ledger row: the in-process gate already covers the cycle; the
  new bench isolates it (the `view_ir` isolation benches set the
  precedent).

## Evidence

- **TDD**: two red→green cycles. Red 1: the link-table tests failed to
  compile (`no field links on type GenLinks`, `cannot find value
  DYNAMIC_BASE`), then the three consumer fail-closed tests failed against
  the scanning impl (a table-less `GenLinks` produced `Ok` — 3 red tests).
  Green: the producer records, the consumer reads, the staleness guard
  rejects, all 34 codegen + 9 sourcemap + 37 incremental tests pass.
  Red 2 (M2): the typed-failure test failed to compile
  (`SourceMapError` absent); green with the variant/offset context
  asserted.
- **D-8 untouched**: `lig reproducibility` — byte-identical, 7,385 bytes;
  `lig fixtures verify` — all 36 fixtures match the committed canonical
  IR on both legs. The link table rides alongside the text; no generated
  byte changed.
- **Gates**: fmt clean; clippy zero warnings (`--workspace --all-targets
  --all-features -- -D warnings`, pedantic from the workspace ladder); the
  zero-suppression grep returns the two pre-existing policy
  `#![expect(unsafe_code)]` lines only (§15's single-unsafe-module policy,
  lig-patch and tests/dummy-host) — none added; tests green
  `--all-features` and `--benches`; the budget ledger green in release
  (26 rows, including the map-cycle-carrying emission rows).
- **Example**: `crates/lig-sourcemap/examples/link_table.rs` — the crate's
  first consumer walkthrough: the recorded table printed, the three maps
  built, generated positions resolved back to source through the chain,
  the typed `Anchor` failure shown as data, and the NFR-6 verifier stats.

## Known gaps (deferred, deliberate)

- `lig-taffy-profile` stays unwired and `cargo vet` stays deferred
  (carried from M19's gap list — an ADR-worthy architecture decision).
- Generated `fire_N` bodies still call the panicking `flush` (carried from
  M21: a D-8-coupled codegen milestone).
- Consumer examples for a custom `Host` implementation and the `lig-patch`
  protocol remain unwritten (the mock-host adapter and the dummy-host
  spike cover them in-tree; walkthroughs are additive work).
