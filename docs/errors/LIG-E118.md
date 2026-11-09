# LIG-E118 — Event handler beyond the list item body

Status: deny (M1; narrowed by M15, by M17, then by M18)

## message
Event handler beyond the list item body

## explanation
Per-item dispatch for events DIRECTLY on a `For`/`v-for` body is the M15
surface: the handler attaches per fresh item, and dispatch goes through the
index-addressed fire fn with the item/index substituted. Since M17 that
extends to COMPOSED bodies whose owner hangs on a branch, and since M18 to
composed bodies inside slot CONTENT — a `For`/`v-for` inside a fill or a
fallback admits its own direct handler too: the composed reconcile's insert
arms attach through the receiver, and the fire fns address items through
the source, so the composition is invisible to dispatch (M17's documented
"branch- or slot-owned" contract, completed on the content leg). What stays
denied is everything BEYOND the body fragment itself — events inside
list-item branches (branch-in-item composition) and events inside nested
list bodies (per-item × per-item). Each needs per-item control flow
(per-item × per-mount and per-item × per-item composition respectively):
the named foundation follow-up, judged an increment away from one mechanism
milestone, not a codegen flag.

## suggestion
Hoist the handler to the list body level (direct, branch-composed, or
content-composed — all admit), or extract the row into a keyed component
(M13 per-item instance identity carries the handler per item).
