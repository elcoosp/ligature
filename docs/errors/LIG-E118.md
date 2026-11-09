# LIG-E118 — Event handler beyond the direct list item body

Status: deny (M1; narrowed by M15)

## message
Event handler beyond the direct list item body

## explanation
Per-item dispatch for events DIRECTLY on a `For`/`v-for` body is the M15
surface: the handler attaches per fresh item, and dispatch goes through the
index-addressed fire fn with the item/index substituted. What stays denied
is everything BEYOND that direct body — events inside list-item branches
(branch-in-item composition), events inside nested list bodies (per-item ×
per-item), and events on composed list bodies (branch- or slot-owned lists,
whose dispatch addressing is not the `&mut self` shape). Each is the named
follow-up of its mechanism milestone (M13/M14 lineage).

## suggestion
Hoist the handler to the list level, or extract the row into a component.
