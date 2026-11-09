# ADR-0028: M15 — per-item event dispatch (events in list bodies)

- Status: accepted (M15)
- Deciders: compiler team
- Date: 2026-11-09

## Context

The follow-up both ADR-0026 (per-item instance identity) and ADR-0027
(per-item slot assembly) name for interaction: **events inside list
bodies** — `LIG-E118`'s v1 denial. The composition matrix admitted events
on component roots (M0, delegated), in slot content across slabs (M7,
`attach_foreign`), and on branch legs (M7/M11, per-mount attach records) —
list bodies were the last top-level site where an event still denied:

- The v1 denial's own explanation states the premise: "per-item dispatch
  needs keyed component instances". M13 delivered exactly that (the
  `Vec<Option<Child>>` stores mirrored against the keyed plan), and M14
  proved the per-item replay discipline (`apply_item`) on the same base.
- Real row UIs are lists of interactive elements — buttons, toggles, input
  rows. Without per-item events, every list handler must hoist to the list
  level (losing the row context) or wrap the row in a component (paying the
  M13 instance machinery for one handler).
- The v1 admission check was flat: it inspected only the For body's DIRECT
  dynamics, so events nested deeper inside the item subtree (behind a
  branch, inside a nested list) slipped past admission and only hit the
  codegen backstop — an admission hole, not just a missing feature.

## Decision

### 1. Direct item-body events admit; dispatch is index-addressed

An event handler DIRECTLY on a For-body fragment emits a per-item fire fn
on the owning component — `fire_f{body}_d{di}(&mut self, h, index, ev)`:
it captures the event payload (`self.ev`), re-evaluates the For source and
addresses the LIVE item at `index` (a stale index returns — no panic, no
ghost dispatch), substitutes the item/index locals into the handler body
(the M13 substitution discipline, now threading through every nesting
level), runs the body, and flushes the owner — the handler's state writes
cascade through the standard schedule. The handler's Effect reaction keeps
its place in the reaction table (its write masks and dispatch bits are
business as usual) but emits NO run arm: the flush scope has no item
addressing, the M13 rule that keeps item work in per-item fns.

### 2. The attach rides the reconcile's insert arms, once per fresh item

The handler record attaches inside the plan's INSERT arms (keyed
`SeqOp::Insert` and the unkeyed insert loop, direct and composed
receivers): the item fragment exists exactly there, freshly built, and the
attach runs once per item lifetime — the per-item replay never re-attaches.
`list_remove` → `teardown` disposes the records with the item's slab
(child-first LIFO), and `list_move` relocates the item's `Box<Fragment>`
wholesale — the handler record moves with its item without re-attaching,
the same identity story as M13's instances and M14's mounted content.

### 3. The substitution threads through every nesting level

M15 generalizes the item-expression renderer: the item/index substitution
and the state-read discipline (component `self.` reads vs the replay's
pre-cloned `s_<field>` params) become an explicit context threaded through
the whole expression generator, instead of a top-level override. Handler
bodies substitute at every nesting level (call args, array elements,
template holes) and clone the item per reference (a handler may read the
item twice); the replay keeps its byte-stable single-reference rendering.
Event-field reads and setter calls inside item BINDS stay codegen-denied
(fail-closed, precise reasons).

### 4. Fail-closed boundaries, redrawn one step — and the walk fixed

The flat v1 check becomes a recursive item-subtree walk with the codegen's
own composition rule mirrored: a For whose owner hangs on a branch or
inside another list is a COMPOSED list — its body events keep the denial
(the composed dispatch addressing is the follow-up), as do events in
list-item branches (per-item × per-mount composition) and in nested list
bodies (per-item × per-item). Each denial reports at the EVENT's span with
one precise reason. Direct item-body events admit on both legs through the
shared admission crate; the composed codegen backstop stays as the second
line.

## Consequences

- The matrix closes for interaction: a handler compiles on a root (M0), in
  slot content (M7), on branch legs (M7/M11), and in list bodies (M15) —
  each with the dispatch semantics its container demands (direct,
  cross-slab, per-mount, per-item).
- Zero runtime changes: `Fragment::attach`, the slab teardown recursion,
  and the flush loop are pre-existing machinery. Zero IR schema changes
  (v5 stands — an Event on a For body was always representable). Zero new
  external dependencies.
- The bench ledger grows the `codegen_for_event` row (M15 reference median
  680 µs against a 3 500 µs ceiling, ≈5.1× headroom); 17 rows, all
  enforced in release.
- The counter unit stays byte-identical (digest unchanged) — the
  milestone's generated-code growth is confined to units using the surface.
- Still deferred, deliberate: events inside list-item branches, events in
  nested list bodies, events on composed (branch/slot-owned) list bodies,
  per-item refs (the M13 store's `Option` slots are the base),
  state-reading binds inside per-item slot content, cross-component fill
  coordination, and `cargo vet`.
