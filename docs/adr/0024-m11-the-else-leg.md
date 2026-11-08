# ADR-024: M11 — the else leg

- Status: accepted (M11)
- Deciders: compiler team
- Date: 2026-11-09

## Context

The frontends have lowered `else` since M2: Solid `<Show when={x()} fallback={…}>`
lowers the `fallback` attribute to `IfIr.else_`, and the Vue leg's
`v-if`/`v-else-if`/`v-else` chains (M6, extended to component/slot tags in
M10) collapse to the same shape. The IR, the canonicalizer, the parity
harness, and the template emission all treat the else fragment as a first
class branch body — the generated unit even emits its template table and
anchor statics. The M10 report flagged the consequence as a known gap: the
If mounting arm "still mounts the `then` branch only".

The deep dive for this milestone confirmed the full shape of the fail-open,
and it is worse than a missing render:

- The mounting arm emits `if branch_is(slot) != on { if on { mount(then) }
  else { unmount } }` — when the condition is false the arm actively
  UNMOUNTS, so the else content never exists in any tree (the emitted else
  template table is dead code; `show-conditional`'s `Gate_TPL_T0` proves
  the intent — the statics were emitted and never referenced).
- `branch_owner_of` matched only `i.then`, so every dynamic on an else
  fragment classified as `Ctx::Direct`: a bind inside else content would
  emit an arm addressing `self.frag.hole(n)` with the ELSE fragment's hole
  index — a silent cross-fragment write, not a denial.
- Component uses and outlets on else fragments missed the M10 mechanisms
  per-mount identity and branch-time assembly were built for (the ctx map
  already walked else branches — only the mounting arm and the owner walk
  ignored them).
- The conformance contract already claims else: `show-conditional` pins
  `fallback`/`v-else` at the IR level on both legs, and README promises
  constructs outside the contract "deny with a precise code rather than
  mis-compile". Else is inside the contract and mis-compiled — the parity
  gate could not see it because it compares IR, not runtime.

A probe of the committed generated code made it concrete:
`ternary-child`'s `Flip` component (an expression ternary lowering to an
If with an else) carried a dead `Flip_TPL_T0` — the else branch of a
shipped fixture never rendered.

## Decision

### 1. Tracked-leg mounting arms

An If dynamic that carries an else leg emits a tracked-leg arm: a `u8`
leg field per (fragment, dynamic) — `leg_f<frag>_d<dyn>` — records the
mounted leg (0 = none, 1 = then, 2 = else). The arm's transition is
leg-aware (`!(branch_is(slot) && leg == L)` → unmount, run BOTH legs'
dispose lists — `Option::take` is idempotent — then mount the wanted leg
and set the tracker). `branch_is` alone cannot tell the two legs apart,
and the runtime `BranchInstance` stays untouched: the leg identity is a
codegen-level fact (the runtime still sees one mounted branch per slot;
teardown/dispose semantics are unchanged).

Then-only Ifs keep the byte-stable M7 emission — `branch_is(slot) != on`
with no tracker — so the 29 pre-existing fixtures' generated units (and
the counter digest, `0x85d06a5c087b3589`-pinned) do not move.

### 2. Else fragments classify as branches

`branch_owner_of` maps `i.else_` to its If owner and the SHARED branch
slot (one slot carries whichever leg is mounted), so else-fragment binds,
events, refs, StyleProps, component prop-pass arms, and nested Ifs address
through the same `Ctx::Branch` chain their then twins use. An else leg
always carries the marking story (`needs_assembly` includes it): the
mounting arm marks the fresh leg's subtree so its binds initialize on the
next flush pass — per leg, guarded by the tracker, so the else leg's
marking can never land on a mounted then branch.

### 3. Per-leg assembly, rebuilds, and captures

The M10 mechanisms generalize over both legs: per-mount component uses on
either leg own `Option<Child>` fields rebuilt in their leg's mount block
and disposed on every transition; outlets on either leg mount their
use-site fill (author side) or their own fallback (any branch) when THEIR
leg mounts, through the composed `branch_mut` → `slot_fill_mut` walk.
Post-flush event attaches and ref captures for else-carrying Ifs guard on
the leg tracker (`if s.leg_fX_dY == L { … }`) — `Fragment::hole()` falls
back to the slab root out of bounds, so an unguarded capture would
silently attach/capture the wrong node.

### 4. The registry milestone stays next

Fills for CHILD-side branch-internal outlets (the M10 report's named
follow-up) still deny through the unmatched-fill backstop. Mount-time
cross-component fill coordination needs the branch legs honest first —
this milestone is that prerequisite, and the mechanism remains a real
design (the child cannot evaluate parent-authored binds or reach parent
instance state).

## Consequences

- The else leg mounts, swaps, initializes, and disposes with full M10
  semantics on both frontends; `ternary-child` and `show-conditional`
  render their else branches for the first time.
- One `u8` per else-carrying If dynamic of struct size — no runtime
  change, no digest movement for then-only shapes.
- `codegen_else` joins the budget ledger (13 rows); the workload rides the
  new `show-else` fixture (30 fixtures, both legs).
- Deferred (pre-existing, unchanged): remount-time re-attach of events and
  ref re-capture for DIRECT-rooted plain branches (the mount marking
  initializes binds, not dispatch records — the M10 report's deferred
  family); direct-rooted branch binds still initialize via co-read
  marking; fills for child-side branch-internal outlets stay fail-closed.
