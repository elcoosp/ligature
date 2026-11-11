# M19 milestone report — the session-observer and session-emission seams

Date: 2026-11-12 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the session surface, completed)

After M18 the session's seams were each one call away from complete but
stopped short of it. A compile pass was observable only through a `tracing`
subscriber (the M16 spans name the stages; the numbers live in span
records), and emission was a consumer-assembled two-step — borrow the
unit slice, map the `Profile` enum onto a bare `"taffy"`/`"web"` string
(the CLI's private match, duplicated at every consumer), remember the BR-1
discipline yourself. Both halves close here:

- **The session-observer seam** (`lig-incremental::observe`): one
  `StageEvent` per stage per unit — `Computed` with the stage's own wall
  clock, `Failed` on a lowering error / admission denial (or cycle,
  LIG-E020) / codegen backstop, `Reused` at zero elapsed when the D-9 memo
  still validated — plus `on_pass` with the same `CompileReport`
  `compile` returns, through `Session::compile_with(profile, &mut dyn
  SessionObserver)`. Stage names and order are 1:1 with the M16 spans; a
  stage that never runs reports nothing. `TracingObserver` ships in-tree,
  forwarding each event as a flat filterable `tracing::info!` with the
  elapsed µs the spans alone do not record. `Session::compile` is
  unchanged and builds no events.
- **The session-emission seam** (`Session::emit_to`): one call from the
  last `compile` pass to the full §9.5 artifact set in any `ArtifactSink`,
  with the BR-1 gate INSIDE the seam — a session holding a denied unit
  (lowering failure, admission denial) or a codegen-backstop failure
  refuses the whole emission with a typed `EmitError` (`ligature::emit`)
  naming the blocking stem; nothing writes on a refusal. The profile rides
  its ledger name through the new `lig_admission::Profile::name()`
  (`"taffy"` / `"web"`, pinned by a unit test — the strings are
  load-bearing: the digest gate keys build state on them), replacing the
  CLI's private `if profile == Profile::Web` match.
- **Micro-debt cleared alongside** (additive-adjacent, per the milestone
  discipline — not substitutes for the seams): `lig-expr`'s dead
  `is_static_attr_value` / `is_logical_not` removed (zero callers,
  including tests); `lig-cli`'s unused `lig-sourcemap` dependency dropped;
  the `errors-gen` stage parser keeps the status line's punctuation
  (`deny (M1; narrowed by M15, …)` → stage `M1`, not `M1;`) and the
  catalog regenerated — the embedded `lig explain` catalog stops serving
  the stale LIG-E118 body; `docs/m2/REPORT.md`'s `tests/cli.rs` path
  citation fixed.

## Design decisions (ADR-0032)

- The observer is `&mut dyn` with caller-owned state — the codebase's own
  seam shape (`&mut dyn Host`, `&mut dyn ArtifactSink`) — synchronously
  driven on the compiling thread; the trait contract documents the
  must-not-panic / must-not-block discipline instead of defending against
  it.
- `Session::compile` delegates to the same pass body through an
  `Option<&mut dyn>` seat that stays `None`: the unobserved path builds no
  event at all, and the unobserved API is untouched.
- `emit_to` refuses rather than skips: `emit_units_to` stays the raw
  primitive (it skips denied units by BR-1 upstream contract), while the
  session front enforces the full contract — the same behavior the CLI
  holds, now where a consumer cannot bypass it.

## Evidence

- **TDD**: one red→green cycle per seam. Red: the `observe.rs` tests failed
  to compile (`no method named compile_with`) and the `emit.rs` tests
  failed (`no method named emit_to`) against the unwired API; green: 10
  observer tests + 5 emission-seam tests + 1 `Profile::name` test, full
  `cargo test --workspace` green.
- **Bench + budget**: `session/observe` and `session/emit_to` criterion
  workloads, ledger rows at ≥5× headroom (24 rows total):
  `session_observe` median 679 µs vs the unobserved pass's 678 µs (the
  event stream costs ≈0.2%), `session_emit_to` median 867 µs vs the free
  fn's 850 µs (the one-call front adds the BR-1 gate ≈2%) — measured by
  `lig budgets` in release, re-measured across calibration, all 24 rows
  within budget.
- **Examples**: `observed_pass` (a timing observer across a cold pass and
  an edit — 3 computed + 1 reused per stage on the warm pass, the stage
  table printed) and `custom_sink` (a consumer-implemented content-
  addressed `DigestCache` — dedupe by digest, the per-pass manifest/state
  rewrite distinction, and the BR-1 refusal writing nothing), both run
  with `cargo run -p lig-incremental --example …`.
- **Gates**: `cargo fmt --all -- --check`; `cargo clippy --workspace
  --all-targets --all-features -- -D warnings -W pedantic` (zero warnings);
  the zero-suppression grep over `src/ crates/ tests/ benches/ examples/`
  (zero `#[allow]`/`#[expect]` attributes in the touched code — the two
  pre-existing `#![expect(unsafe_code)]` of the single-unsafe-module
  policy are untouched, lig-patch and tests/dummy-host); `cargo test
  --workspace --all-features`; `cargo test --workspace --benches`.

## Known gaps (deferred, deliberate)

- **The `Host` integration story stays open.** A consumer still implements
  the 15-method `Host` trait and drives `build`/`flush` themselves; there
  is no session-level wiring and no walkthrough example for the runtime
  half. The observer/emission seams were closed first because they are
  compile-side; the host side is a runtime-embedding milestone of its own.
- **`lig-taffy-profile` stays unwired** (zero dependents; the const-fn
  emitter never reaches codegen). Wiring or parking it is an
  architecture-level decision (ADR-worthy) larger than this seam milestone.
- **`lig-sourcemap`'s `IrRustLink` stays defined-but-unpopulated.** It is
  the designed §9.4 link-table shape; removing it would delete a designed
  seam, populating it is a codegen-internals milestone.
- **`cargo vet`** remains deferred (carried from M18's gap list).
