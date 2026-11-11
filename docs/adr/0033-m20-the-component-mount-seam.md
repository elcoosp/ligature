# ADR-0033: M20 — the component-mount seam

- Status: accepted (M20)
- Deciders: compiler team
- Date: 2026-11-12

## Context

M19 (ADR-0032) closed the compile half's seams: the pass is observable
(`Session::compile_with` → stage events), and emission is one fail-closed
call (`Session::emit_to` → any `ArtifactSink`). The **runtime half** stayed
assembled by hand — M19's known-gaps list named it: a consumer implements
the 15-method `Host` trait and then drives the generated unit through
free functions and conventions no document owns:

- **The lifecycle is a three-recipe story.** Mount = call the generated
  `build` against `&mut dyn Host` (whose init flush is a build-phase
  detail of §9.2, not a consumer-visible contract). Drive = import the
  free `flush` and call it after every setter batch, with the argument
  order (state first, host second) the compiler enforces but no type
  names. Dispose = know that `Fragment::teardown` is the total LIFO
  teardown and reach through the generated state's `frag` field — a path
  no example walked and no test outside `compile-clean`'s demo asserted.
  Every embedding host loop re-assembles the same three recipes.
- **The teardown story was undiscoverable.** The fragment contract
  ("Disposal is per-fragment and total: teardown removes and destroys in
  LIFO slab order", §10.1) existed only as a rustdoc line and a
  `list_teardown` used by generated reconcile arms. A consumer who drops
  the component state instead of tearing it down silently skips every
  `destroy` commit — the host leaks the surface. Nothing in the public
  API pointed at the difference.
- **The style-table binding stayed a convention.** The generated unit
  emits `pub static {Component}_STYLE_CSS: &[&str]` and the host is
  expected to bind it "by its own `bind_style_table` convention" —
  correct, but the walkthrough that makes the convention visible did not
  exist anywhere.

The compile-side seams were closed first because they are compile-side;
the host side is a runtime-embedding milestone of its own. That milestone
is this one.

## Decision

### 1. One consumer-owned trait: `ComponentUnit`

`lig-runtime` gains a `mount` module (`#![no_std] + alloc`, zero
dependencies unchanged) with exactly one trait:

- `ComponentUnit: Reactions` — the `Reactions` supertrait is the contract
  generated code already implements (§7.2: const schedule, dirty bits,
  monomorphic `run`); the trait adds one accessor, `fragment(&mut self)
  -> &mut Fragment`, naming the root fragment as the teardown surface.
  The implementation is three lines next to the `include!`d (or
  otherwise compiled) unit — the crate that owns the type implements the
  seam, which the orphan rule enforces naturally.
- The trait is object-safe (`&mut dyn ComponentUnit` works for host
  registries), synchronously driven, and must-not-panic / must-not-block
  by contract — the same seam discipline as `&mut dyn Host` (§10.1),
  `&mut dyn ArtifactSink` (§9.5), and `&mut dyn SessionObserver`
  (ADR-0032).

### 2. One handle: `Mounted<C: ComponentUnit>`

The lifecycle front, mirroring the session seams' one-call shape:

- `Mounted::mount(host, build)` — pairs the generated `build` with the
  handle. The build's init flush stays inside `build` (mount adds no
  second flush; on a converged dirty set it would be a no-op anyway).
- `Mounted::state()` — the component state, for setters and the generated
  `fire_N` dispatch; the handle adds the lifecycle, never a second way
  into the state.
- `Mounted::update(host, f)` — the save-tick: setter batch, then the
  const-schedule flush, in one call.
- `Mounted::flush(host)` — the bare tick.
- `Mounted::dispose(host)` — the total teardown: the root fragment's
  recursive LIFO walk (branches, list items, slot content, then every own
  slab record), one `destroy` commit per node the build created, and the
  state returned (fields intact; the slab handles are spent).

### 3. The flush loop widens to trait objects

`flush` (and everything built on it) takes `R: Reactions + ?Sized` — the
one signature change the object-safe seam needs. Generated code passes
concrete state and is unaffected.

### 4. Walkthrough example + budget row

- `tests/compile-clean/examples/host_mount.rs` walks the committed
  counter unit end to end on `MockHost` — style-table bind, mount, one
  save-tick, one event dispatch, dispose, and the checked-tick failure
  path (M21) — runnable with `cargo run -p compile-clean --example
  host_mount`, asserting the balanced commit stream so the walkthrough is
  also a gate.
- The `runtime/mount_lifecycle` bench + `runtime_mount_lifecycle` ledger
  row (release median 0.29 µs, ceiling 1.5 µs) pins the lifecycle cost.

## Trade-offs accepted

- The mount front does not hide the `Host` implementation: writing the
  15-method host remains the embedding work (D-5 makes it the contract
  boundary on purpose). The seam closes the *driving* half, which is
  where the recipes were duplicated.
- `Mounted` is generic over `C: ComponentUnit` rather than a `dyn` box —
  consumers keep their concrete state for setters and `fire_N`; the trait
  stays object-safe for registries that need `dyn`.
- The style table stays a convention (no new trait): the walkthrough
  example pins the wiring instead of adding an abstraction only one
  static per unit would use.

## Consequences

- Every lifecycle recipe collapses to one call each; the counter
  walkthrough is the reference a consumer copies.
- Total disposal is now a public, tested, budget-pinned surface — a
  consumer who wants a leak-free teardown has a named path.
- The remaining runtime-half work (an embedded host registering mounted
  components behind `dyn ComponentUnit`, the patch engine's host bridge)
  builds on the same trait.
