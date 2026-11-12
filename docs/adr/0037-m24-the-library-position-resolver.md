# ADR-0037: M24 — the library position resolver

- Status: accepted (M24)
- Deciders: compiler team
- Date: 2026-11-12

## Context

M22 gave the map pipeline its producer-side table and typed failures, but
the consumer's last composition stayed hand-rolled: resolving ONE
generated position back to source. The only implementation was the CLI's
`run_resolve` — `generate_lookup_table` + `lookup_token` + 1-based
conversion + name extraction, inline in `lig-cli/src/main.rs` — and the
M22 `link_table` walkthrough re-implemented the walk a third time to print
its positions. An embedded consumer (a stack-trace mapper, a debugger
panel, a test-report annotator) had to copy the recipe, with no typed
failure: `lookup_token` returns `None` and the caller invents the error.

## Decision

### 1. `resolve` is library API

`lig_sourcemap::resolve(map, line, col) -> Result<ResolvedPosition,
SourceMapError>` — the exact `lookup_token` contract (0-based in and out,
VLQ-native), the source path and the IR label (`e7`/`r2`/`t3`) surfaced as
data in `ResolvedPosition`. It works on any map the crate produces or a
consumer loads: the merged map, one of the halves, or a map read back from
JSON through `round_trip` — the disk-read surface the CLI uses.

### 2. The miss is typed, the hit is exact

A position with no token fails closed as the new
`SourceMapError::Unmapped { line, col }` (the CycleError pattern: typed
data, module-path code, `core::error::Error`) — no nearest-hit policy
invented in the library; a consumer that wants nearest-hit semantics has
the map and the table to implement its own policy. The same-position
contract is written down: per-arm attribution stacks several IR nodes on
one generated position, and the lookup deterministically selects one of
the tokens AT that position — pinned by a set-membership test plus an
exactness test on unique positions.

### 3. The CLI is the thinnest wrapper

`run_resolve` keeps its file I/O and 1-based boundary conversion, and
delegates everything else to `lig_sourcemap::resolve` — the printed line,
the exit-1 no-mapping path, and the internal-error path all render from
the typed result. `lig-cli` gains the `lig-sourcemap` dependency (the
emission path already used it transitively; the CLI now speaks the map
crate directly).

### 4. Bench, no new ledger row

`sourcemap/resolve_merged` joins the isolation bench (the M22 precedent):
one `resolve` per iteration over a prebuilt merged map — the lookup-table
build plus binary search `lig resolve` pays per query. No ledger row: the
gate's `session_emit_to`/`session_emit_mem` rows already re-measure the
map cycle end-to-end, and a per-position query has no end-to-end row to
guard.

## Trade-offs accepted

- `SourceMapError` grows a variant: the enum is the crate's failure
  surface, and a miss that carries the position as data is exactly what
  the variant list was missing (the `Resolve` variant stays the
  verifier's — different failure, different data).
- `ResolvedPosition.source` is an owned `String` (`"?"` when the token
  carries no source): a resolver result outliving the map borrow is the
  common embedded shape, and the map's sources are cheap to copy.

## Consequences

- The chained-map story closes end to end in the library: build (M22) →
  verify → resolve (M24) — the CLI prints, the library speaks.
- Consumer walkthrough: `crates/lig-sourcemap/examples/resolve_position.rs`
  — positions resolved, the recorded link read, the typed miss shown.
- The M22 walkthrough's inline position walk retires in favor of the
  resolver in future examples; the existing example stays as recorded.
