# ADR-0042: M28 — nested-list events (the per-item × per-item lift)

- Status: accepted (M28)
- Deciders: compiler team
- Date: 2026-11-27

## Context

The E118 boundary (ADR-0030 §4, narrowed by M15/M17/M18/M27) carried two
named denial zones; the M27 report carried the second unchanged: **events
inside nested list bodies** (per-item × per-item) — "events inside nested
list bodies (per-item × per-item). Each needs per-item control flow … the
named foundation follow-up, judged an increment away from one mechanism
milestone, not a codegen flag". The branch-in-item zone closed with M27;
the nested-list zone is the remaining one, its own mechanism milestone.

The premise the nested denial relied on is gone. Each list item is its own
runtime `Fragment` — `list_insert` instantiates the body template with its
hole AND branch anchors, and `ensure_list` resolves a list table's anchor
from the fragment's `branch_holes` — so the inner list's reconcile state
(the keys, the live items, the anchors) IS the fragment-per-item: mounting
the inner list per outer item needs no parallel store, no new schedule and
no build-time addressing. The per-item replay is already a control-flow
fan-out (the reconcile re-runs `apply_item_f{body}` over EVERY live item on
every pass — the M13/M14 discipline), the per-item event machinery is
receiver-free (M15 fire fns address items through the source), and
attach-per-fresh-insert is an established shape (the M15 insert arms, the
M27 mount-time attaches). What was missing was exactly one emission: the
item method had a codegen backstop (`emit_item_method` rejected all
control flow inside item bodies) while the machinery it guards already
existed per level.

Real rows compose lists of lists: a board of columns, each rendering its
own cards, with a per-card handler. Without nested-list events, every such
handler hoists to the component root or wraps the card in a keyed
component — paying the M13 instance machinery for one handler — for a
shape whose dispatch machinery is already in place one level up.

## Decision

### 1. The inner list mounts per outer item, inside the item replay

`emit_item_method` grows the For arm: the inner reconcile is ONE commit in
the outer apply (`reconcile_item_f{body}(f, h, item.clone(), index)`), so
the three replay call sites (keyed, unkeyed, composed) cannot drift — the
M27 arm's single-mechanism discipline. The inner reconcile is a free fn
that addresses the ITEM fragment's own list table (the inner For's branch
anchor slot; `let slot: usize = <literal>` binds the shared attach
renderer's param spelling), runs the same keyed/unkeyed plans as the
top-level reconcile (`plan_keyed`/Index diff), and its replay loop
re-applies the inner binds per pass with BOTH items substituted. No new
schedule, no run arms, no build-time addressing: the inner body's
reactions join the bodies', the per-item fallbacks' and the legs' in the
flush-scope exclusion (one reaction, N×M live items — the M14 discipline
squared).

### 2. The per-item-square state is the fragment-per-item itself

The inner list's keys, items and anchors live on the item fragment's
`lists` vec — one outer index, one inner index, no `Vec<Vec<…>>` store on
the component. Teardown is recursive (`Fragment::teardown` walks mounted
branches, list items and slots LIFO), so removing an outer item disposes
its inner table and its inner handler records with it; the outer plan ops
need no mirrors (the M13 instance-store mirroring applies to component
uses, which stay denied inside nested bodies). The item fragment's inner
slot is resolved by `branch_slot_of_dynamic` on the OUTER body (the same
numbering the M27 branch arms share).

### 3. The inner surface is the inner replay's reach

The inner body's OWN scalar binds (text/class/attr/style-prop — inner-item,
outer-item or index-bound; state-reading binds keep the M14 state→per-item
fan-out boundary) and its OWN events admit; the binds re-apply per replay
pass through the inner apply, and the inner events attach ONCE at the
inner insert arm (`item_event_attaches` on the item fragment's inner
slot — the record disposes with the inner item; a stale pair stays
inert). Anything structural inside the inner body (nested If/For, another
list level, components, slots, refs, html) keeps the fail-closed denial —
the boundary moves, it does not disappear. The inner source must read the
outer item binding directly (`each={g}` — the backend lattice types the
item's element list), and inner sources/keys/binds never read state.

### 4. Dispatch rides the M15 fire fns, squared

One index-PAIR-addressed fire fn per inner event dynamic
(`fire_f{body}_d{di}(&mut self, h, outer_index, index, ev)` — the outer
source re-evaluated through `self`, the inner source re-evaluated through
the live outer item, both items substituted into the handler body, the
checked `CycleError` front; a stale pair returns as a no-op). The mounts
and the fire fns agree on the same canonical fragment ids.

### 5. Admission walks each For chain exactly once

The E118 walk rooted at a For body recurses into its nested bodies with
`owner_direct` one level deep (`nested_ok`), and a For hanging on a For
body never fires its own walk — the admitted surface is consistent no
matter which fragment triggers the walk (the pre-M28 walks double-fired on
nested chains, which the one-precise-reason contract cannot tolerate).
The nested surface is ONE level: the nested body's own events admit, its
own branch legs and its own nested lists stay denied (`leg_direct` gates
on `nested_ok`). The composed content contract lifts the same surface on
both frontends (`ItemSurface::NestedBody` — the M18 precedent: one
milestone moves own-tree and composed together).

### 6. Nested literal state initializers

The per-item × per-item shape's data is a list of lists; the M1-era signal
initializer check accepted only arrays of literal scalars. `init_is_literal`
accepts nested literal arrays recursively (the generation renders
`vec![vec![…]]` already), so `createSignal([["alpha", "beta"]])` lowers —
a prerequisite the fixture pins, additive to every pre-existing shape.

## Consequences

- The two-leg `for-event-nested` fixture (40 × 2 legs now) pins the
  generated shape: the inner reconcile + apply + key trio, the replay
  commit, the attach per fresh inner item, and the index-pair fire fn;
  `lig fixtures verify` matches the committed canonical IR on both legs.
- The demo (`nested_list_events_mount_and_dispatch_per_item_square`) and
  the `for_event_nested` walkthrough pin the runtime story: the inner
  table on the item fragment, per-pair dispatch, stale pairs inert, the
  keyed inner move repositioning the same fragment, removal disposing the
  inner table with the item, and a fresh remount re-attaching exactly once.
- The budget ledger grows the `codegen_for_event_nested` row (33 rows),
  all enforced in release.
- Admission narrows E118 a fifth time: the allow case moves to the nested
  shape and the deny case moves to a nested-body-leg shape; the rule row
  and the catalog entry move together (the embedded catalog re-generated by
  `lig errors-gen`).
- The M27-era committed gen module `for_event_item_branch` had gone stale
  against its own codegen (a later M27-series commit refined the tracker
  param, the index conversion, and the identical-leg bind collapse without
  regenerating); `lig fixtures gen` re-verifies every module, and the
  refreshed module is committed with this milestone.
- Carried gaps unchanged: cross-component branch-mount fill coordination
  (ADR-024 §4 — still a real design), state-reading per-item
  conditions/binds and sources (the state→per-item fan-out), per-item³
  lists (doubly-nested — the next level of the same mechanism),
  `lig-taffy-profile` parked, `cargo vet` deferred.
