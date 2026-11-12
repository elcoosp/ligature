# ADR-0035: M22 — the recorded link table

- Status: accepted (M22)
- Deciders: compiler team
- Date: 2026-11-12

## Context

The §9.4 source-map contract said "codegen emits the markers itself, no
text scanning in consumers" — but only the marker NAMES were true API.
`GenLinks` carried `(id, name)` pairs while every byte offset was found by
text-scanning in `lig-sourcemap::ir_to_rust` (four `generated.find(...)`
passes per map build), and the designed `IrRustLink` shape sat
defined-but-unpopulated — M19's and M21's reports both name populating it
as a codegen-internals milestone. Three costs followed from the dead seam:

- **The consumer contract was false.** `ir_to_rust` re-derived positions
  the producer already knew, from first-occurrence needles — a marker name
  echoed anywhere before its declaration would mis-attribute silently.
- **Failure attribution landed late.** A marker missing from the generated
  text surfaced only when a map was built (emission time), not when the
  emitter produced the text.
- **The verifier under-checked.** `verify` confirmed markers EXISTED in
  the text; it could not confirm the map pointed at the RIGHT offsets,
  because the offsets were never recorded anywhere.

## Decision

### 1. The emitter records the table — post-unparse, fail-closed

`lig-codegen` gains `IrLink { ir_ordinal, generated_offset }` (the
designed §9.4 entry shape) and `GenLinks.links: Vec<IrLink>`. After
`prettyplease::unparse` — the one point where exact offsets exist —
`record_link_offsets` locates every reaction/template marker the emitter
just wrote and records it, sorted by offset. A marker that did not reach
the final text fails codegen as the new `CodegenError::LinkTable` (the
session's `CodegenFailure` renders it with help-text) — the failure moves
to emit time, where the text was born. The combined ordinal space is
owned by the producer: `DYNAMIC_BASE` moves to `lig-codegen`
(`lig-sourcemap` re-exports it; the public name survives).

### 2. The consumer reads the table — one anchor check, no scans

`ir_to_rust` builds every reaction and template token from the recorded
offsets. The one text touch left is a `starts_with` AT a recorded offset —
the fail-closed anchor check (drift/tampering detection), not a scan.
Failures are typed (`SourceMapError::LinkMissing { ordinal }`,
`Anchor { ordinal, offset }`): a table-less `GenLinks` (e.g. the `Default`
of a pre-M22 producer) fails on a module-anchored staleness guard instead
of silently producing an empty map, and `verify` re-checks every recorded
offset — reaction markers AND template statics — against the generated
text.

### 3. Typed failures on the seam (the CycleError pattern)

`SourceMapError` (`LinkMissing`/`Anchor`/`RoundTrip`/`Resolve`) replaces
the bare `Result<_, String>` across `ir_to_rust`/`verify`/`round_trip`:
typed data, a module-path code constant (`ligature::sourcemap`,
`EmitError` renders it as the miette code upstream), `core::error::Error`,
no `miette` — the crate keeps its dependency floor. `emit.rs` maps the
`Display` into `EmitError::contract` unchanged.

### 4. Bench

`sourcemap/link_table_cycle` (`crates/lig-sourcemap/benches/link_table.rs`)
pins the per-unit map pipeline — codegen (recording) → `src→IR` →
`IR→Rust` → `chain` → `verify` — release median ≈666 µs on the counter.
No new ledger row: the gate's `session_emit_to`/`session_emit_mem` rows
already re-measure this cycle in-process end-to-end (both PASS, ≈0.87 ms
against 4.5–4.7 ms ceilings), and `view_ir`'s unregistered benches set the
precedent for isolation benches.

## Trade-offs accepted

- One `find` pass over the text happens in the producer (post-unparse —
  offsets do not exist before it); the consumer contract — no scanning —
  is what the design point protected, and it holds now.
- `GenLinks` grows by one `Vec` (≈12 B per marker; the counter records 7):
  negligible against the text itself, and it retires four scans per map
  build on the emission path.
- `IrRustLink` becomes a consumer-facing alias of the producer type rather
  than a separate struct: two definitions of the §9.4 shape would drift.

## Consequences

- The designed seam is populated: `GenLinks.links` is the single source of
  IR→Rust positions, fail-closed at both ends (missing marker = codegen
  error; drifted offset = typed consumer error caught by `ir_to_rust` AND
  `verify`).
- Generated code is byte-identical (D-8): `lig reproducibility` and the
  36-fixture parity gate pass unchanged; the link table rides alongside
  the text.
- `lig-sourcemap`'s public failures are typed and carry the ordinal/offset
  as data — the crate fails like the runtime (M21) and the session (M18)
  do.
