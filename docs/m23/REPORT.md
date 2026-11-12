# M23 milestone report — the watch-tick session seam

Date: 2026-11-12 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the watch loop body, as one call)

The session seams closed one composition at a time — M18 the snapshot
diff (`Session::sync`), M19 the emission (`Session::emit_to`) and the
observed pass (`Session::compile_with`) — but the tick itself, the loop
body `lig watch` runs per save, stayed caller-assembled: sync, compile,
a hand-rolled denial walk, emit, print, with the fail-closed BR-1
ordering living in the caller's discipline. M23 makes the tick one
surface:

- **`Session::tick(fresh, keep, sink, profile, prune)`**
  (`lig_incremental::tick`): the sync applies first (D-9 no-op
  discipline, keep-list semantics unchanged), the digest-gated pass
  runs, and the shared BR-1 gate refuses a session holding a denied
  unit or a codegen-backstop failure BEFORE the sink sees a byte. The
  gate is extracted (`refuse_blocked_pass`) and shared with `emit_to` —
  the two fail-closed fronts cannot drift.
- **`Session::tick_with(..., observer)`**: the same pass through the M19
  observer seat — stage events 1:1 with the M16 tracing spans, `on_pass`
  before the report returns; an unobserved tick builds no events.
- **`TickReport { sync, pass, emission }`**: the three reports the loop
  prints as one value. The failure IS the emission seam's failure — the
  typed `EmitError` (`ligature::emit`, help-text), no new error type.
- **The emptied-session policy, pinned**: a snapshot that removes every
  source is a legitimate watch state — the tick writes the empty
  manifest, records the empty build state, and prunes the leftovers
  (`emit_to` keeps its empty-refusal; there the empty set means "forgot
  to compile", in a tick the compile just ran).
- **The CLI loop body is the seam**: `lig watch` reads the fresh
  snapshot (file I/O stays in the CLI) and hands the rest to
  `Session::tick`; the inline denial walk, the dead `if once` branch
  inside the loop, and the duplicated print code retire. The initial
  pass stays build-shaped.

## Design decisions (ADR-0036)

- No new trait — the tick composes the seams the session already speaks,
  the way `emit_to` composes the emission; the sink stays `&mut dyn
  ArtifactSink`, the observer stays caller-owned.
- On the BR-1 refusal the sync/compile reports are not delivered: the
  sync and compile have already applied to the session (inspect units
  for diagnostics), the same contract `emit_to` refusals follow.
- A no-op tick still rewrites `manifest.toml`/`build-state.json`
  (identical bytes — the "manifest rewritten every pass" contract); the
  CLI's `sync.is_empty()` early-continue keeps no-op ticks silent, and
  the digest-gated artifacts never rewrite.

## Evidence

- Tests (`cargo test -p lig-incremental --lib`): 10 new tick tests —
  composition, no-op reuse, exact-edit re-emission, the two BR-1
  refusals (denied + codegen backstop, sink untouched), prune,
  emptied-session manifest, observer events, keep-list pass-through,
  and byte-for-byte parity with the pre-seam hand-assembled loop.
- Bench: `session/watch_tick` — a prebuilt 8-source session takes one
  tick over a fresh snapshot with one edit, into a fresh in-memory sink
  (the `emit_mem` convention): release median ≈7.47 ms; the ledger row
  (`session_watch_tick`, 27 rows now) bounds the cold tick end-to-end
  at 40 ms (≈5.3× headroom). The gate re-measures in-process: PASS.
- Example: `crates/lig-incremental/examples/watch_tick.rs` (`cargo run
  -p lig-incremental --example watch_tick`) — the cold tick, the no-op
  tick, the one-edit tick, the BR-1 refusal with the sink untouched,
  and the prune tick, all against a `MemSink`.
- CLI: the `lig watch` loop drives the seam; `lig build`/`lig watch`
  output shapes unchanged (the shared `print_unit_line` extraction).

## Remaining gaps / risks

- The tick takes the snapshot as data; filesystem watching stays the
  consumer's job (the CLI's interval scan) — a notify-backed watcher is
  a consumer concern, not a seam gap.
- The BR-1 refusal hides the sync report; a consumer that needs the diff
  on the failure path can read it from the session state.
