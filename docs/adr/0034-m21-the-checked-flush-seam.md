# ADR-0034: M21 — the checked flush seam

- Status: accepted (M21)
- Deciders: compiler team
- Date: 2026-11-12

## Context

The §7.2 flush loop's cycle fail-safe was a bare `assert!`:
`LIG-E401 reactive cycle at runtime` when the schedule does not converge
within `MAX_PASSES` passes. The catalog entry (docs/errors/LIG-E401.md)
documents the assertion as "the fail-safe" — but the failure mode is a
panic, and the embedder story diverged from the compile half:

- **Embedded targets cannot unwind.** The runtime's stated posture is
  `#![no_std] + alloc` on bare metal (§10: thumbv7em, aarch64-none
  builds). On a `panic = "abort"` target the fail-safe is a hard reset;
  the host never sees the failure as data, cannot log it, cannot fall
  back to a safe surface.
- **The mount front inherited the panic.** M20's `Mounted::flush` /
  `Mounted::update` are the one-call lifecycle; a tick that hits the cap
  panicked out of the consumer's event loop mid-tick, after the setter
  batch had already applied.
- **The codebase convention is typed, catalog-coded failures.** The
  compile half fails as miette diagnostics with stable codes
  (`ligature::emit`, `ligature::codegen`, help-text naming the fix —
  M16/M18); the session denies cycles at compile time as typed
  `LIG-E020` diagnostics. The runtime's equivalent code, LIG-E401,
  existed only as a panic message string.

## Decision

### 1. `flush_checked` — the checked form of the same loop

`lig-runtime::schedule` gains `flush_checked<R: Reactions + ?Sized>(rx,
host) -> Result<(), CycleError>`: byte-for-byte the §7.2 convergence
semantics `flush` has always run (topological order, write-mask
re-dirtying, converge when a full pass produces nothing new), reporting a
non-converging schedule as `Err` instead of panicking. `flush` stays the
panicking front — generated code (`fire_N` bodies, the build's init
flush) calls it unchanged — and now delegates to the shared body, panicking
with the error's `Display` (the catalog code plus the pass cap, strictly
more context than the bare assert).

### 2. `CycleError` — the typed LIG-E401

`pub struct CycleError { pub passes: u8 }` with `Display` =
`"LIG-E401 reactive cycle at runtime: the schedule did not converge
within {passes} passes"` and `core::error::Error`. No `miette` — the
runtime keeps zero dependencies; the code string is the cross-cutting
contract (the catalog, `lig explain LIG-E401`, and the panic message all
carry it). The `passes` field records the cap the loop enforced.

### 3. The mount front goes checked

`Mounted::flush` and `Mounted::update` return `Result<(), CycleError>`.
The mount front is new API (M20); it adopts the compile half's
fail-closed discipline from the start instead of carrying two failure
modes. The setter batch semantics on failure are documented: the fields
hold their new values; only the re-render is missing — a consumer can
log the error, drop to a safe surface, and dispose through the same
handle (teardown is unaffected by a failed tick, asserted by test and by
the walkthrough example).

### 4. Budget row for the fail-safe path

The `runtime/flush_checked_cycle` bench + `runtime_flush_checked_cycle`
ledger row (release median 37 ns, ceiling 0.2 µs) pins the worst-case
cost: the ping-pong cycle through `flush_checked` runs exactly
`MAX_PASSES` capped passes, then the typed error. The bound is the
fail-safe's contract — no runaway.

## Trade-offs accepted

- Two flush fronts (`flush` panicking, `flush_checked` typed) instead of
  one: generated code keeps the zero-cost call it emits today, and a
  codegen change (rewiring every `fire_N` to the checked form) is not
  worth the byte-reproducibility churn (D-8) this milestone would carry.
- The error carries the pass cap, not the offending reaction ids: the
  diagnostic value lives in the catalog entry and the static cycle check
  that should have denied the shape; reaction-id attribution belongs to
  the compile-time LIG-E020 path.

## Consequences

- An embedded host can run the mount front fail-closed: every lifecycle
  call either completes or returns a catalog-coded error — no unwinding.
- The runtime's typed-failure story now covers the flush loop the same
  way the session covers lowering, admission, codegen, and emission.
- The panic message grew the pass cap; the existing `LIG-E401` panic
  tests keep passing (the catalog code is still the message's head).
