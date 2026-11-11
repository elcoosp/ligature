# M21 milestone report — the checked flush seam

Date: 2026-11-12 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the fail-safe as data)

The §7.2 flush loop's cycle fail-safe was a bare panic:
`LIG-E401 reactive cycle at runtime` after `MAX_PASSES` non-converging
passes. On the runtime's stated deployment surface — `#![no_std]` bare
metal with `panic = "abort"` (§10) — that is a hard reset, and the mount
front inherited it mid-tick, after the setter batch had applied. M21
gives the runtime the compile half's typed-failure discipline:

- **`flush_checked`** (`lig_runtime::schedule`): byte-for-byte the §7.2
  convergence semantics `flush` has always run, reporting a
  non-converging schedule as `Err` instead of panicking. `flush` stays
  the panicking front for generated code (`fire_N` bodies, the build's
  init flush — all unchanged, byte-identical under D-8) and now
  delegates to the shared body, panicking with the error's `Display` —
  the catalog code plus the pass cap, strictly more context than the
  bare `assert!` had.
- **`CycleError { passes: u8 }`**: the typed LIG-E401 — `Display` names
  the catalog code and the enforced cap; `core::error::Error`. No
  `miette`: the runtime keeps zero dependencies; the code string is the
  cross-cutting contract (`docs/errors/LIG-E401.md`, `lig explain`, and
  the panic message all carry it).
- **The mount front goes checked**: `Mounted::flush` and
  `Mounted::update` return `Result<(), CycleError>` — a failed tick
  leaves the fields holding their new values (only the re-render is
  missing) and leaves the handle fully usable; teardown through
  `dispose` is unaffected, asserted by test and by the walkthrough
  example.
- **The fail-safe cost is budgeted**: the `runtime/flush_checked_cycle`
  bench (the ping-pong cycle — exactly `MAX_PASSES` capped passes, then
  the typed error) and the `runtime_flush_checked_cycle` ledger row pin
  the worst case: release median 37 ns (bench) / 60 ns (in-process gate
  run), ceiling 0.2 µs (≈5× headroom over the reference median), 26 rows
  total, all within budget.

## Design decisions (ADR-0034)

- Two flush fronts, one body: generated code keeps the zero-cost
  panicking call it emits today (rewiring every `fire_N` to the checked
  form would churn D-8 byte-reproducibility across all 36 fixtures for
  no consumer need — the mount front is the checked entry consumers
  drive); the convergence loop exists exactly once.
- The error carries the pass cap, not reaction attribution: the
  diagnostic value lives in the catalog entry and in the compile-time
  cycle check (typed LIG-E020) that should have denied the shape.

## Evidence

- **TDD**: one red→green cycle. Red: the `schedule.rs` tests failed to
  compile (`cannot find function flush_checked in this scope`, 3 tests);
  green: `flush_checked` + `CycleError` + the `flush` delegation + the
  mounted-cycle test, full `cargo test --workspace` green (the
  pre-existing `cycle_panics_with_lig_e401` test passes unchanged — the
  catalog code still heads the panic message).
- **Bench + budget**: `runtime/flush_checked_cycle`, ledger row
  `runtime_flush_checked_cycle` — release median 37 ns (bench) / 60 ns
  (in-process), ceiling 0.2 µs; the in-process bound test pins the
  work cap (exactly `MAX_PASSES` reactions, no runaway).
- **Example**: `host_mount` section 6 — a hand-written ping-pong unit
  through the mount front: `checked tick: LIG-E401 reactive cycle at
  runtime: the schedule did not converge within 32 passes`, then a total
  dispose (the failed tick leaves the handle usable).
- **Gates**: as M20 — fmt clean; clippy zero warnings
  (`--workspace --all-targets --all-features -- -D warnings`, pedantic
  from the workspace ladder); the zero-suppression grep (two
  pre-existing policy `expect`s only); tests green `--all-features` and
  `--benches`.

## Known gaps (deferred, deliberate)

- Generated `fire_N` bodies still call the panicking `flush` (event
  dispatch inside a generated unit keeps the assert fail-safe); the
  checked path is the consumer-facing tick. Rewiring the generated calls
  is a codegen milestone coupled to a D-8 regen.
- `lig-taffy-profile` stays unwired and `cargo vet` stays deferred
  (carried from M19's gap list).
- `lig-sourcemap`'s `IrRustLink` stays defined-but-unpopulated (the
  designed §9.4 link-table shape; a codegen-internals milestone).
