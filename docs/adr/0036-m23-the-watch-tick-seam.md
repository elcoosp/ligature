# ADR-0036: M23 — the watch-tick session seam

- Status: accepted (M23)
- Deciders: compiler team
- Date: 2026-11-12

## Context

The session seams closed one composition at a time: M18 made the watch
tick's snapshot diff one call (`Session::sync`), M19 made the emission one
call (`Session::emit_to`) and the pass observable (`Session::compile_with`)
— but the tick itself, the loop body `lig watch` runs per save, was still
caller-assembled from three of them plus a hand-rolled BR-1 check. The CLI
watch loop carried that assembly inline: sync, compile, walk the units for
denials, emit, print — and every embedded consumer that wanted watch-like
live reload (a dev server, an IDE plugin, a playground) had to re-assemble
the same four steps and get the fail-closed ORDER right on their own: a
denied pass must never let the sink see a byte, and nothing in the API
shape enforced it.

Three costs followed:

- **The BR-1 ordering was a convention, not a contract.** `Session::sync`
  and `Session::compile` each returned their own report; nothing stopped a
  consumer from emitting a partially-denied session — `emit_to` would
  refuse it (the M19 gate), but only if the consumer actually routed the
  emission through it AFTER checking, and the check-and-continue dance the
  CLI ran (collect denials, print, skip emission) lived outside the
  library.
- **The tick was unobservable.** `compile_with` exposes stage events for
  the pass, but the composition — what the snapshot changed, what the pass
  recomputed, what the emission wrote — had no one-report surface, so an
  embedded tick either scraped three reports or instrumented nothing.
- **The emptied-session edge was ambiguous.** A snapshot that removes
  every source: `emit_to` refuses an empty session ("forgot to compile"),
  but a watch tick that emptied itself is legitimate — the dist must be
  pruned. The caller-assembled loop handled it by accident of ordering.

## Decision

### 1. The tick is one session call

`Session::tick(fresh, keep, sink, profile, prune)` composes the three
seams in the fail-closed order: `sync` applies first (D-9 no-op
discipline, keep-list semantics unchanged), the digest-gated pass runs,
then the shared BR-1 gate refuses a session holding a denied unit or a
codegen-backstop failure BEFORE `emit_units_to` sees the sink. The
refusal is the same typed `EmitError` (`ligature::emit`, help-text) M19
established — the gate itself is extracted (`refuse_blocked_pass`) and
shared with `emit_to`, so the two fronts cannot drift.

### 2. One report, three stages

`TickReport { sync, pass, emission }` carries the three reports the loop
prints — the snapshot diff, the compile report, the emission report — as
plain public fields; no new error type, no wrapping: the tick's failure
IS the emission seam's failure.

### 3. The observability convention rides along

`Session::tick_with(..., observer)` drives the same pass through a
`SessionObserver` — the M19 seat discipline (`compile_impl`'s
`Option<&mut dyn SessionObserver>`), stage events 1:1 with the M16 tracing
spans, `on_pass` before the report returns. An unobserved tick builds no
events.

### 4. The emptied session is a legitimate tick

The one deliberate divergence from `emit_to`, pinned by test: a session
the snapshot emptied emits the empty manifest, records the empty build
state, and prunes the leftovers. `emit_to` keeps its empty-refusal (there
the empty set means "forgot to compile"); in a tick the compile just ran,
so empty means emptied. The gate is shared, the empty-policy is not.

### 5. The CLI loop body is the seam

`lig watch`'s loop body reads the fresh snapshot (file I/O stays in the
CLI — the seam takes paths' texts, not the filesystem) and hands the rest
to `Session::tick`; the denial walk, the dead `if once` branch inside the
loop, and the hand-rolled emit-and-report print move into the tick's Ok/
Err arms. The initial pass stays build-shaped (`compile` +
`emit_and_report`).

### 6. Bench

`session/watch_tick` (the `session/watch_tick` ledger row, 27 rows now):
a prebuilt 8-source session takes one tick over a fresh snapshot with one
edit — sync diff, recompile of exactly the edited unit, fail-closed
in-memory emission. The whole loop body, once.

## Trade-offs accepted

- No new trait: the tick composes existing seams the way `emit_to` does.
  A `TickSink`/`TickObserver` abstraction would add surface without a
  second implementation.
- On the BR-1 refusal the sync/compile reports are NOT delivered (the
  error is the `EmitError`): the sync and compile have already applied to
  the session — the consumer inspects units for diagnostics, the same
  contract `emit_to` refusals follow.
- A no-op tick (identical snapshot) still runs the memo-validation pass
  and rewrites `manifest.toml`/`build-state.json` (identical bytes, the
  "manifest rewritten every pass" contract); the CLI keeps its
  `sync.is_empty()` early-continue so no-op ticks print nothing. The
  digest-gated artifacts never rewrite.

## Consequences

- The watch story is one call for every consumer; the CLI's loop is the
  seam's thinnest wrapper, and the BR-1 ordering is enforced where a
  consumer cannot skip it.
- Embedded live-reload hosts get the tick's observability for free
  (`tick_with`), the same events the compile-with seam delivers.
- The emptied-session policy is now written down and pinned, not an
  accident of loop ordering.
