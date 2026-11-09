# ADR-0027: M14 — per-item slot assembly (slot outlets in list bodies)

- Status: accepted (M14)
- Deciders: compiler team
- Date: 2026-11-09

## Context

The follow-up every milestone report since M10 names for outlets: **slot
outlets inside list bodies**. The composition matrix admitted outlet
mounting on component roots at build (M6/M7 — the parent's init walk), on
both legs of a branch (M10/M11 — branch-time assembly, child-side outlets
mounting their own fallbacks), and composed through nested fill contexts
(M9) — list bodies were the last site where an outlet still denied with
`LIG-E001`:

- A `<Slot>`/`<slot>` inside a `.map`/`<For>`/`v-for` body lowered (before
  M10) into a never-mounted outlet hole — nothing assembles fills or
  fallbacks per item, so the hole rendered empty forever (the same silent
  fail-open M10 closed for branches).
- Per-item slot content is the natural row abstraction: a keyed list whose
  rows are `<Panel>`-style components that expose outlets, with the row's
  fallback (or the use site's fill, once cross-component coordination
  lands) rendering per item.
- M13 built the per-item base: the keyed/unkeyed replays already run a
  per-item pass (`apply_item`) over every live item, and the runtime
  `Fragment` already owns a per-instance slot table (`mount_slot`,
  `slot_fallback_mut`, teardown-recursion) — the outlet hole of an item is
  just another hole of the item's own slab.

The M10 child-side precedent is the proven template: an outlet the CHILD
owns (not at its root) mounts its own fallback at mount time, child-local —
no cross-component coordination.

## Decision

### 1. The item replay mounts the outlet's own fallback

A slot outlet DIRECTLY on a For-body fragment emits into the per-item
replay (`apply_item`): `f.mount_slot(h, di, SlotOrigin::Fallback, …fb
triple…)` on the ITEM fragment — idempotent (`mount_slot` is a no-op on an
already-mounted item), so the replay can run per reconcile without
double-mounting. The content triple (template, hole anchors, branch
anchors) is the outlet's own fallback fragment, statics riding the
authoring component's prefix — child-local, exactly the M10 child-side
shape.

### 2. The fallback's item-bound binds ride the same replay

The mounted content's scalar binds (Text/Class/Attr/StyleProp) apply right
after the mount, addressed through `f.slot_fallback_mut(di)` — the item's
own slot table — with the item/index locals substituted (`gen_item_expr`,
the M13 prop-replay discipline). The binds re-run on every replay, so
unkeyed re-indexing refreshes them like the body's own binds. Their
reactions leave the general run arms (one reaction, N live items — the
flush scope has no item addressing); they stay schedule-inert (locals read
no state fields), so the const schedule is untouched.

### 3. Move/dispose identity is free — the item owns the content

The mounted content lives in the ITEM fragment's slot table: `list_move`
relocates the item's `Box<Fragment>` wholesale (the same content moves —
no rebuild), and `list_remove` → `teardown` recurses into slots
(child-first LIFO with the item's own records). Zero runtime changes.

### 4. Fail-closed boundaries, redrawn one step

- **Outlets inside list-item BRANCHES stay denied** (the sticky `in_for`
  discipline, both legs, one precise reason): per-item × per-mount slot
  composition is the follow-up.
- **Outlets on COMPOSED list bodies stay denied** (the content-side
  `content_contract` keeps its case): fills/fallbacks resolving through a
  `Ctx::Fill` chain at reconcile time is the composed follow-up.
- **Use-site fills for list-body outlets stay fail-closed** through the
  M9 unmatched-fill backstop (items do not exist at the parent's build, so
  the fill would never mount — the cross-component fill coordination is
  the named follow-up, as for M10 child-side branches).
- **Per-item slot content admits scalar binds only** (item/index-bound or
  static): state-reading binds (the flush fan-out over live items), events
  (the `LIG-E118` per-item dispatch discipline), refs, and structural
  content keep the codegen backstop with precise reasons.

## Consequences

- The matrix closes for own-tree lists: an outlet compiles on a component
  root (M6/M7), in content and branches (M8/M9/M10/M11), and in list
  bodies (M14) — each with the assembly semantics its container demands
  (build-time, mount-time, per-item).
- Zero runtime changes, zero IR schema changes (v5 stands), zero new
  dependencies; the counter unit stays byte-identical.
- The generated-code growth is confined to units using the surface (the
  per-item mount + bind block inside `apply_item`).
- The follow-ups keep their precise denials: branch-in-item slot
  composition, composed-content list outlets, cross-component fill
  coordination for non-root outlets, per-item events/refs, state-reading
  per-item binds.
