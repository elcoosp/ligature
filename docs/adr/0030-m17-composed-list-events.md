# ADR-0030: M17 — events on composed list bodies

- Status: accepted (M17)
- Deciders: compiler team
- Date: 2026-11-09

## Context

The boundary LIG-E118 drew at M15 has three denial zones left after the
direct item body admitted: events inside list-item branches
(branch-in-item), events inside nested list bodies (per-item × per-item),
and events on COMPOSED list bodies — a For whose owner fragment hangs on a
branch or inside slot content. Both the E118 doc and the M15 codegen
backstop name the composed zone as its own follow-up ("the composed
dispatch addressing is the follow-up"), and M16's report repeated it with
the consumer's eye: the first milestone judged by people who can run
`examples/` first.

The premise the composed denial once relied on is gone. M8 shipped the
composed reconcile — an associated fn that addresses the list through
whatever receiver the ctx chain provides (`branch_mut`, `slot_fill_mut`,
`slot_fallback_mut`), not through `self.frag`. M13 mirrored the per-item
instance ops into both plans' arms. M14/M15 parameterized the per-item
attach emission by receiver — `item_event_attaches(recv, index_expr)` is
called with `self.frag` for direct lists and with the composed `recv` for
branch- and slot-owned lists — and the composed reconcile has emitted those
attach calls in its insert arms since M15. The fire fns are
source-addressed (`&mut self` methods that re-evaluate the For source and
substitute the live item): they never touch the owner fragment, so the
composed ctx needs no special dispatch shape. A composed body's events were
denied by exactly two checks — the admission walk's `owner_direct` flag and
the codegen `emit_item_method` backstop — while every mechanism they guard
was already receiver-neutral.

Real UIs compose lists inside conditionals and layout slots: a results
table inside a collapsible panel, a row list inside a tab's fill. Without
composed-body events, every such handler hoists to the component root or
wraps the row in a component — paying the M13 instance machinery for one
handler — for a shape whose dispatch machinery is already in place.

## Decision

### 1. Events directly on a composed For body admit

The admission walk admits events on the For body fragment itself
regardless of ownership: direct, branch-owned, and slot-owned bodies are
one admission class. The `owned_body_fragments` precomputation and the
`owner_direct` flag die with the distinction — the walk keeps a single
boolean that only separates the body's OWN events (admit) from its
subtree (deny).

### 2. The composed backstop dies; the fire fns were already context-free

`emit_item_method` drops its `direct` parameter and the composed-body
error: the per-item replay and the fire fns emit identically for both
classes. The attach story needs no new code — the composed reconcile's
insert arms have carried the receiver-parameterized attaches since M15 —
and the dispatch story needs none: `fire_f{body}_d{di}` addresses items
through the source, so a stale index is a no-op and a keyed move carries
the record exactly as in the direct shape.

### 3. List-carrying branches remount through the mount mark

The composed-event demo test exposed a latent M8-era gap, orthogonal to
events: a plain (no-assembly) branch never marked its subtree on mount, so
a remounted branch's list table — reconcile state, not template state —
stayed empty until its source next changed. The plain If arm now emits the
M8 mount mark when the branch subtree carries a For dynamic; the mark
re-runs the subtree's reactions on the next flush pass (the reconcile
re-inserts into the fresh table, binds re-apply). Scoped to
list-carrying branches: every list-free unit's bytes stay pinned, and the
reproducibility gate still reports the counter unit byte-identical.

### 4. Fail-closed boundaries, redrawn one step — again

Events inside list-item branches and inside nested list bodies keep the
LIG-E118 denial at the event's own span, with the help-text narrowed to
the two remaining zones (both need per-item control flow — per-item ×
per-mount and per-item × per-item composition — the foundation follow-up,
not an increment). The E118 doc, the rule row, and the admission ladder's
allow case (now the composed shape) move together; the deny case stays
branch-in-item.

## Consequences

- The composed-event fixture (`for-event-composed`, both legs) pins the
  generated shape: the associated reconcile, the attach in its insert
  arms, the per-item fire fn — and the demo test pins the runtime story
  through a branch cycle (mount → dispose → remount with fresh records).
- The `codegen_for_event_composed` ledger row joins the §14.2 budget
  ledger (release median 816 µs against a 4 500 µs ceiling, ≈5.5×
  headroom); 19 rows, all enforced in release.
- The consumer surface gains a third runnable example:
  `composed_events` — the admitted composed unit end to end, the
  generated surface, and the still-denied zone rendered through miette.
- Branch-in-item and nested-list events remain the named follow-ups; the
  E118 message stays one precise reason per event.
