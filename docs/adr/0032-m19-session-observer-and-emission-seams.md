# ADR-0032: M19 — the session-observer and session-emission seams

- Status: accepted (M19)
- Deciders: compiler team
- Date: 2026-11-12

## Context

M16 (ADR-0029) made the consumer surface real: runnable examples, named
`tracing` spans across the session stages, and typed `EmitError`/`M18
CodegenFailure` diagnostics. Two seams stayed half-open, both hit by the
same class of consumer — a team embedding `lig-incremental` in a host with
no filesystem and no log subscriber:

- **The pass is observable only through a log subscriber.** The M16 spans
  name the stages (`session_compile` wrapping `lower` → `admit` →
  `codegen` per unit), but the *data* — per-stage wall clocks, which stage
  failed which unit, memo hits versus recomputes — lives in span records a
  subscriber must reconstruct. A program that wants numbers (an IDE status
  bar, a CI watcher, an embedded profiler) gets nothing it can hold. The
  `CompileReport` carries pass-level `recompiled`/`reused`/`elapsed` only.
- **Emission is a two-step, consumer-assembled call.** The emission-target
  seam (M16's `emit_units_to`) takes a borrowed unit slice and a bare
  profile string (`"taffy"` / `"web"`), so a consumer re-implements the
  CLI's assembly: collect `session.units()`, map the `Profile` enum onto
  the string (the CLI's own `if profile == Profile::Web` match — a magic
  string duplicated at every consumer), and remember to skip emission on a
  denied pass. The BR-1 contract ("a denied pass never touches dist/") is
  the caller's discipline, not the seam's — a consumer that forgets it
  silently writes a denied unit's neighbors.

## Decision

### 1. The session-observer seam: stages as events to a caller-owned object

`lig-incremental` gains `observe` (`SessionObserver`, `Stage`,
`StageOutcome`, `StageEvent`, `TracingObserver`):

- `Session::compile_with(profile, &mut dyn SessionObserver)` runs the exact
  digest-gated pass `compile` runs, driving one `StageEvent` per stage per
  unit — `Computed` with the stage's own wall clock, `Failed` on a lowering
  error, an admission denial (or cycle), or the codegen backstop, `Reused`
  at zero elapsed when the D-9 memo still validated — plus one `on_pass`
  with the same `CompileReport` `compile` returns.
- The stage names and order are 1:1 with the M16 spans (`lower`, `admit`,
  `codegen`), so a trace and an event stream interleave into one story.
  A stage that never runs (codegen after a denial, admit after a lowering
  failure) reports nothing — the seam never invents stages.
- The seam shape is the codebase's own: `&mut dyn`, object-safe,
  synchronously driven, caller-owned state (readable the moment the pass
  returns) — the same shape as `&mut dyn Host` and `&mut dyn
  ArtifactSink`. `Session::compile` is unchanged and builds no events (an
  `Option<&mut dyn>` seat that stays `None`).
- `TracingObserver` ships in-tree: it forwards each event as a flat
  `tracing::info!` (`session_stage`) with the stage name, unit, outcome,
  and µs elapsed as fields — the durations the spans alone do not record.
- Trade-off accepted: the observer rides the compiling thread and its
  events are borrow-only views; a panicking or blocking observer is a
  consumer bug documented in the trait contract, not defended against.

### 2. The session-emission seam: one fail-closed call into any sink

`Session::emit_to(&self, sink: &mut dyn ArtifactSink, profile: Profile,
prune: bool)` is the session front of `emit_units_to`:

- The BR-1 gate rides the seam: a session holding a denied unit (lowering
  failure or admission denial) or a codegen-backstop failure refuses the
  WHOLE emission with a typed `EmitError` (`ligature::emit`) naming the
  blocking stem and the fix; a session with no compiled units refuses the
  same way. Nothing writes on a refusal — the contract the CLI holds, now
  enforced where the consumer cannot skip it.
- The profile rides its ledger name through the new
  `lig_admission::Profile::name()` (`"taffy"` / `"web"`) — the one mapping,
  in the library, replacing the CLI's private match; the strings are
  load-bearing (the digest gate keys build state on them) and are pinned by
  a unit test in `lig-admission`.
- `emit_units_to` stays the primitive for callers that already hold a unit
  slice; `emit_to` reuses its body unchanged, so the artifact bytes are
  identical through either front.

### 3. The seams are budgeted, benched, and exampled like every other seam

- `session/observe` and `session/emit_to` criterion workloads + ledger rows
  (24 rows total) + the in-process measurement registry: the observer event
  stream measured ≈0.2% over the unobserved pass (679 µs vs 678 µs
  release-median on the counter workload), the one-call emission ≈2% over
  the free fn (867 µs vs 850 µs) — the seams are free enough to leave on.
- Two consumer walkthroughs: `observed_pass` (a timing observer across a
  cold pass and an edit — recompute versus `Reused` per stage) and
  `custom_sink` (a consumer-implemented content-addressed `ArtifactCache`
  — dedupe, digest-gate reads, and the BR-1 refusal writing nothing).

### 4. Micro-debt cleared alongside (additive-adjacent, not substitutes)

- `lig-expr`'s dead `is_static_attr_value` / `is_logical_not` removed
  (zero callers anywhere, including tests).
- `lig-cli`'s unused `lig-sourcemap` dependency dropped.
- `lig errors-gen` regenerated after the stage parser keeps the status
  line's punctuation (`deny (M1; narrowed by …)` → stage `M1`, not `M1;`)
  — the embedded `lig explain` catalog stops serving the stale LIG-E118
  body.
- `docs/m2/REPORT.md`'s `tests/cli.rs` path citation fixed to
  `crates/lig-cli/tests/cli.rs`.

## Consequences

- The session surface is now sync → compile (observed or not) → emit
  (fail-closed, any sink), each step a one-call seam with typed errors and
  measured overhead.
- The `Profile::name` strings become a library contract; renaming a
  profile's ledger name now breaks a pinned test instead of a silent
  build-state mismatch.
- What stays open: the `Host` integration story (a consumer still
  implements the 15-method trait and drives `build`/`flush` themselves —
  no session-level wiring, no walkthrough example), and the unwired
  `lig-taffy-profile` crate (zero dependents; its const-fn emitter never
  reaches codegen). Both are named in the M19 report's remaining gaps.
