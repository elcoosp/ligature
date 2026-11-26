# ADR-0038: The component registry and the patch host bridge

- Status: accepted
- Deciders: compiler team
- Date: 2026-11-26

## Context

The mount seam (M20, ADR-0033) shipped the runtime half's lifecycle front —
one `ComponentUnit` impl per component, one `Mounted<C>` handle — and its
report's gap list carried two named, deferred items:

- **A `dyn` component registry.** "Host stores mounted components behind
  `&mut dyn ComponentUnit`" had the trait ready (object-safe by design, its
  object-safety pinned by test) but no shipped container: a host mounting a
  scene of components — a grid of tiles, a stack of panels — kept its own
  `Vec`-of-boxes and re-assembled the lifecycle per component on its own.
  The object-safe test proved the shape; nothing shipped it.
- **The patch engine's host bridge.** `lig-patch` (ADR-013/ADR-016) owned
  the protocol — jump table, `PatchSession`, `hot_call` at the `fn() -> R`
  spike shape — and the runtime owned the mounted surface, but nothing
  connected them: the mounted event front (the generated `fire_N` glue:
  capture, mark, checked flush) had no safepoint, so a hot patch could not
  reach a running component's dispatch.

Two costs followed: embeddings re-assembled the same store and lifecycle
per host (with the fail-closed tick order left to each one), and the patch
protocol's steady-state benefit — patched behavior on the next event,
no remount, no state loss — stopped at the spike's `fn() -> R` symbols.

## Decision

### 1. `ComponentRegistry` is the shipped container (in `lig-runtime`)

`lig_runtime::ComponentRegistry` — a `BTreeMap`-backed store of
`Box<dyn ComponentUnit>` keyed by `CompId` (`Copy`, never reused after
disposal, the `NodeId` discipline). The lifecycle mirrors `Mounted` per id
and across the scene: `mount` runs the same build contract (init flush
included) and boxes the unit; `update`/`flush` are the checked front
through the trait object (`flush_checked`'s `?Sized` widening makes the
`&mut dyn ComponentUnit → &mut dyn Reactions` upcast the whole
implementation — no boxing glue, no second flush path); `flush_all` drains
the scene in id order, failing closed on the first non-converging schedule;
`dispose`/`dispose_all` are the mount seam's total teardown per unit / per
scene. What the trait object drives is the reaction mark + the checked
tick — the exact body the generated glue runs after its event capture;
setters stay inherent on the concrete state (the `Mounted` front, reached
through a host side-table keyed by the same ids — the D-5 representation
boundary, unchanged).

### 2. The bridge is a safepoint pair plus one front (in `lig-patch`)

`lig-patch` gains the `lig-runtime` dependency edge (std over no_std, the
one place the patch protocol meets the runtime half) and a `surface`
module:

- `hot_dispatch(f, state, host, ev)` — the mounted dispatch as a safepoint
  call: the `hot_call` pattern at the generated glue's shape
  (`fn(&mut C, &mut dyn Host, &E) -> Result<(), CycleError>`). A table hit
  transmutes to the same-signature patched body; a miss calls the fat
  build's body; the checked front is preserved either way.
- `captured_dispatch_address` / `call_captured_dispatch` — the
  never-dlclose proof on the surface (§2.4): capture a generation's
  committed address before the next apply, call it after any number of
  later generations.
- `HotSurface<C>` — the mounted front whose every dispatch rides the
  safepoint: mount once, dispatch across generations (the newest committed
  table routes every call — no remount, no state loss), setters and ticks
  stay the mount seam's (`surface()` hands back the `Mounted` handle),
  dispose through the same total teardown.

Real generations arrive through `PatchSession::apply` against loaded
dylibs; the route mechanics proven in-process are identical, and the
`lig patch-spike` gate keeps proving the dylib path end to end.

### 3. Bench, no new ledger row

`patch-surface/dispatch_hit` joins the isolation bench (the M22/M24
precedent): the route at the dispatch shape per iteration. The ledger's
`patch_dispatch_hit` row already bounds the raw route cost and
`runtime_dispatch_checked` bounds the mounted tick, so the surface adds no
end-to-end row of its own. The registry's scene lifecycle is the
exception-as-row: `runtime_registry_lifecycle` (29 rows now) bounds the
container's mount → update → flush_all → dispose_all walk, the four calls
a scene-owning host loop repeats.

## Trade-offs accepted

- The registry stores `Box<dyn ComponentUnit>` — mounting hands the
  concrete state into the container, and the trait object drives marks,
  ticks and teardown, not setters. That is the D-5 boundary doing its job:
  `Mounted<C>` stays the setter front, the registry is the store, and a
  host side-table keyed by `CompId` holds what stays concrete. No
  downcasting surface is added to keep the trait object-safe and
  `no_std`-clean.
- `flush_all` fails closed on the first non-converging schedule rather
  than aggregating: earlier components stay drained, the failing one keeps
  its dirty set, and the caller ticks it alone via `flush` — one typed
  error, no error-vector taxonomy.
- `lig-patch` (std, the §15 single-unsafe module) now depends on
  `lig-runtime` (no_std, `forbid(unsafe_code)`) — a one-way edge; the
  runtime never learns the jump table exists, and the bridge's unsafe
  stays inside the crate that already owns the protocol's unsafe.

## Consequences

- The mount seam's gap list closes: the trait's object-safety test stops
  being the only proof the registry shape works — the container ships
  with its own tests, walkthrough (`component_registry`) and ledger row.
- The patch protocol reaches the runtime half: a hot patch lands on the
  next event dispatch of a mounted component, with the checked front and
  the never-dlclose discipline preserved on the surface; the walkthrough
  (`hot_surface`) doubles as the M22 gap list's consumer example for the
  protocol and for a custom `Host` implementation, spelled out in full.
- Carried gaps unchanged: `lig-taffy-profile` stays parked (the
  ADR-worthy wiring decision), `cargo vet` stays deferred, and the
  deliberately-denied language surfaces (branch-in-item events, nested
  list bodies, state-reading per-item slot binds) keep their precise
  denials.
