# M24 milestone report — the library position resolver

Date: 2026-11-12 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the chained map becomes a queryable surface)

M22 gave the map pipeline its producer-side link table and typed
failures, but the consumer's last composition stayed hand-rolled:
resolving ONE generated position back to source existed only as the
CLI's `run_resolve` (lookup table + `lookup_token` + 1-based conversion
+ name extraction, inline in `lig-cli`), and the M22 walkthrough
re-implemented the walk to print its positions. An embedded consumer —
a stack-trace mapper, a debugger panel, a test-report annotator — had to
copy the recipe and invent its own error for the miss. M24 makes the
resolution library API:

- **`lig_sourcemap::resolve(map, line, col)`**: the exact `lookup_token`
  contract, 0-based in and out (VLQ-native), returning
  `ResolvedPosition { source, line, col, name }` — the original source
  file, the position inside it, and the IR label the map carried
  through (`e7`/`r2`/`t3`). Works on any map the crate produces or a
  consumer loads: the merged map, one of the halves, or a map read
  back from JSON through `round_trip`.
- **The miss is typed**: `SourceMapError::Unmapped { line, col }` joins
  the failure set (the CycleError pattern: typed data, module-path code
  `ligature::sourcemap`, `core::error::Error`) — no nearest-hit policy
  is invented in the library; nearest-hit is a consumer policy and the
  map is theirs to implement it on.
- **The same-position contract, written down**: per-arm attribution
  (M3) stacks several IR nodes on one generated position; the lookup
  deterministically selects one of the tokens AT that position —
  pinned by a set-membership test across every merged-map position and
  an exactness test on unique positions.
- **The CLI is the thinnest wrapper**: `run_resolve` keeps its file I/O
  and the 1-based boundary conversion, delegates everything else to
  `resolve`, and renders the typed result — the printed line, the
  exit-1 no-mapping path, the internal-error path. `lig-cli` speaks
  `lig-sourcemap` directly now (the dep the emission path already used
  transitively).

## Design decisions (ADR-0037)

- `Unmapped` is a new variant on the crate's failure surface: a miss
  that carries the position as data is what the variant list was
  missing; the verifier's `Resolve` variant stays (different failure,
  different data).
- `ResolvedPosition.source` is an owned `String` (`"?"` when the token
  carries no source): a result outliving the map borrow is the common
  embedded shape.
- Bench, no new ledger row: `sourcemap/resolve_merged` joins the
  isolation bench (the M22 precedent — the gate's
  `session_emit_to`/`session_emit_mem` rows already re-measure the map
  cycle end-to-end; a per-position query has no end-to-end row to
  guard).

## Evidence

- Tests (`cargo test -p lig-sourcemap --lib`): 5 new resolve tests —
  set-membership across every merged position, exactness on unique
  positions, the typed `Unmapped` miss (data + `Display` + code), a
  JSON round-trip resolve (the disk-read surface), and the error
  object-safety check. 15 tests in the crate, all green.
- Bench: `sourcemap/resolve_merged` — one `resolve` per iteration over
  a prebuilt merged map (the lookup-table build plus binary search
  `lig resolve` pays per query); isolation bench, unregistered by
  design.
- Example: `crates/lig-sourcemap/examples/resolve_position.rs` (`cargo
  run -p lig-sourcemap --example resolve_position`) — the merged map,
  five generated positions resolved back to source, the first recorded
  link resolved through the M22 table, the typed miss printed, and a
  resolve over the JSON-loaded map.
- CLI: `lig resolve` drives the library resolver; the integration test
  pins the mapped-position line and the exit-1 no-mapping path.

## Remaining gaps / risks

- Nearest-hit lookup ("resolve this position or the closest token
  before it") is deliberately not the library's policy; a consumer that
  needs it builds the lookup table itself (`generate_lookup_table` is
  public) or asks for the policy to be promoted in a follow-up.
- `resolve` re-derives the lookup table per call — the per-query cost
  the CLI pays; a hot consumer hoists `generate_lookup_table` and uses
  `lookup_token` directly.
