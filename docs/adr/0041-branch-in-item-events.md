# ADR-0041: M27 — branch-in-item events (the per-item × per-mount lift)

- Status: accepted (M27)
- Deciders: compiler team
- Date: 2026-11-26

## Context

The E118 boundary (ADR-0030 §4) carried two named denial zones after M18:
events inside list-item branches (branch-in-item) and events inside nested
list bodies (per-item × per-item) — "each needs per-item control flow …
the named foundation follow-up, judged an increment away from one mechanism
milestone, not a codegen flag". The M26 report carried both unchanged.

The premise the branch-in-item denial relied on is gone. The per-item
replay is already a control-flow fan-out: the reconcile re-runs
`apply_item_f{body}` over EVERY live item on every pass (the M13/M14
discipline), and each item is its own runtime `Fragment` with its own
branch table (`list_insert` already instantiates the body template with its
branch anchors). The per-item event machinery is receiver-free (M15 fire
fns address items through the source) and attach-per-fresh-mount is an
established shape (the M15 insert-arm attaches, the M18 mount-time
attaches). What was missing was exactly one emission: the body's If
dynamics had a codegen backstop (`emit_item_method` rejected all control
flow inside item bodies) while the machinery they guard already existed.

Real rows compose conditionals with handlers: a status chip that only
renders (and only dispatches) when the row's own data says so — `n.length >
4` — with a fallback leg for the rest. Without branch-in-item events, every
such handler hoists to the list level or wraps the row in a keyed component,
paying the M13 instance machinery for one handler.

## Decision

### 1. The branch mounts per item, inside the item replay

`emit_item_method` grows the branch arm: the If evaluates with the
item/index substituted (`gen_item_expr`), and the flip mounts/unmounts on
the ITEM's own fragment (`f.branch_is(slot)` guards idempotence — each item
owns its branch table, so the per-item leg instances never collide). The
replay loop's fan-out makes the flip re-evaluate per item per pass; no new
schedule or run-arm addressing exists.

### 2. An else leg adds a per-item leg tracker

`branch_is` alone cannot tell the two legs apart (the M11 finding), so an
else-carrying If adds `legs_f{body}_d{di}: Vec<u8>` on the component (0 =
none, 1 = then, 2 = else), passed to the apply fn as a `&mut` param and
mirrored by BOTH reconciles' plan ops — insert pushes 0, remove drops the
index, move repositions — the M13 instance-store discipline, one `u8` per
item. Direct-rooted branches stay tracker-free and byte-stable (the M11
precondition), and bodies without else-carrying item branches gain no
fields at all (D-8: every committed gen module re-verified unchanged).

### 3. The leg surface is the mount arm's reach

The leg's OWN scalar binds (text/class/attr/style-prop, item-bound —
state-reading binds keep the M14 state-to-per-item fan-out boundary) and
its OWN events admit; the binds re-apply per replay pass through the
mounted leg (`branch_mut`), the M14 mount-once-bind-per-replay discipline.
Events attach ONCE at the mount (`attach_foreign` into the item's slab —
the record disposes with the item; a stale id is inert, the M26
never-reused-id discipline). Anything structural inside a leg (nested
If/For, components, slots, refs, html) and any deeper fragment keeps the
fail-closed denial — the boundary moves, it does not disappear. State
never enters the flip either: a state-reading condition needs the
state→per-item fan-out that the M14 slot-bind backstop already names, so
item-scoped conditions only.

### 4. The leg reactions never join the component schedule

One reaction, N live items — the M14 flush-scope discipline: the direct
legs' reactions join the body's and the per-item fallbacks' in the `rxs`
exclusion, so no run arm and no build attach addresses a leg fragment (the
build's event walk skips the legs like it skips bodies), and the M11
component-scope leg trackers skip body-owned Ifs (the per-item tracker
replaces them).

### 5. Dispatch rides the M15 fire fns, one per leg event

Each leg event dynamic emits its own index-addressed fire fn
(`fire_f{leg}_d{di}` — source-addressed, `&mut self`, the checked
`CycleError` front). The mounts and the fire fns agree on the same
canonical fragment ids; the demo pins both legs dispatching through the
LIFO-canonical ids (then = f3, else = f2 in the fixture's module).

### 6. The Vue leg gains the v-for + v-if chain

The parity contract is two-leg, and the Vue frontend had no spelling for a
branch on the item-body root: `v-for` consumed the element first and a
residual `v-if` hit the chain-bug backstop. `lower_for_item_chain` gives
the Vue-3 semantics — `v-if` on a `v-for` element takes precedence and
binds PER ITEM: the For lowers on the outer frame and the item fragment
carries the If dynamic with the chain's legs as its children (the same
shape the Solid leg's `<Show>`-rooted item body produces), trailing
`v-else` siblings consumed. The body fragment registers under the parent's
children (the canonical fragment DFS walks `children` — a fragment
reachable only through a dynamic body would prune as unreachable).

## Consequences

- The two-leg `for-event-item-branch` fixture (39 × 2 legs now) pins the
  generated shape: the per-item flip arm, the tracker mirrors, the
  mount-time attaches, and both leg fire fns; `lig fixtures verify`
  matches the committed canonical IR on both legs.
- The demo (`item_branch_events_mount_and_dispatch_per_item`) and the
  `for_event_item_branch` walkthrough pin the runtime story: side-by-side
  then/else legs across items, per-item dispatch, dispose with the item,
  a fresh mount re-attaching exactly once.
- The budget ledger grows the `codegen_for_event_item_branch` row (32
  rows), all enforced in release.
- Admission narrows E118 a fourth time: the allow case moves to the leg
  shape and the deny case moves to a leg-subtree shape; the rule row and
  the catalog entry move together (the embedded catalog re-generated by
  `lig errors-gen`).
- Carried gaps unchanged: nested-list events (per-item × per-item — the
  remaining E118 zone, its own mechanism milestone), cross-component
  branch-mount fill coordination (ADR-024 §4 — still a real design),
  state-reading per-item conditions/binds (the state→per-item fan-out),
  `lig-taffy-profile` parked, `cargo vet` deferred.
