# LIG-E118 — Event handler beyond the list item body

Status: deny (M1; narrowed by M15, by M17, by M18, by M27, then by M28)

## message
Event handler beyond the list item body, its branch legs, and nested list bodies

## explanation
Per-item dispatch for events DIRECTLY on a `For`/`v-for` body is the M15
surface: the handler attaches per fresh item, and dispatch goes through the
index-addressed fire fn with the item/index substituted. Since M17 that
extends to COMPOSED bodies whose owner hangs on a branch, and since M18 to
composed bodies inside slot CONTENT — a `For`/`v-for` inside a fill or a
fallback admits its own direct handler too: the composed reconcile's insert
arms attach through the receiver, and the fire fns address items through
the source, so the composition is invisible to dispatch (M17's documented
"branch- or slot-owned" contract, completed on the content leg). Since M27
the zone narrows once more: events on the DIRECT branch legs of a list item
body admit (per-item × per-mount composition — the branch mounts per item
inside the per-item replay, the leg's handler attaches at the mount, and
dispatch rides the same index-addressed fire fns; an else leg adds a
per-item leg tracker mirrored by the keyed plan ops). Since M28 the zone
narrows a fifth time: events on the body's DIRECT nested list bodies admit
(per-item × per-item composition — the inner list mounts per outer item
inside the replay, the inner reconcile addresses the ITEM fragment's own
list table so the per-item-square state is the fragment-per-item itself,
the inner handler attaches per fresh inner item, and dispatch rides the
index-PAIR-addressed fire fns: outer index, inner index). What stays denied
is everything BEYOND that surface — events inside the legs' own branches
and subtrees, events inside the nested bodies' own branches, and events
inside doubly-nested lists. The remainder needs composition the inner
replay cannot reach (per-item³ and the state-reading fan-out are carried
boundaries, not codegen flags).

## suggestion
Hoist the handler to the list body level (direct, branch-composed, or
content-composed — all admit), onto a direct branch leg of the item body
(admitted since M27), onto a nested list body one level down (admitted
since M28), or extract the row into a keyed component (M13 per-item
instance identity carries the handler per item).
