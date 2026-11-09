# ADR-0026: M13 — per-item instance identity (components in list bodies)

- Status: accepted (M13)
- Deciders: compiler team
- Date: 2026-11-09

## Context

Every milestone report since M9 carries the same deferred follow-up: the
**keyed-component story** — component uses inside list bodies. The
composition matrix admitted component uses in content (M9), in branch
bodies (M10, per-mount `Option<Child>` identity), and on both legs of a
conditional (M11) — lists were the last top-level site where a component
use still denied with `LIG-E001`:

- A `.map`/`<For>` body carrying a component use lowered into ONE eager
  instance field shared by every item — its root attached once (or never),
  and every item rendered the same child (the M10 finding).
- Real UIs are lists of components (rows, cards, badges). The denial forced
  authors to break the component abstraction: hoist the use out of the
  body, or flatten the row into raw elements.
- The smaller per-item gaps sit on the same base: `LIG-E118`'s explanation
  ("per-item dispatch needs keyed component instances") and per-item refs
  both wait for per-item identity to exist.

The M10 per-mount machinery (rebuild per mount, attach at the hole, dispose
on unmount) is the proven template; a list is the same story per item, with
the keyed plan (`plan_keyed`, §7.5) driving when items come and go.

## Decision

### 1. A `Vec<Option<Child>>` store mirrors the keyed plan

A component use on a For-body fragment generates a per-item instance store
on the owning component: `pub <child>: Vec<Option<Child>>` (the per-use
dedup naming applies, so a second use of the same child appends a suffix).
The generated reconcile method mirrors every `SeqOp` onto the store:

- **Insert** — after `list_insert` mounts the item fragment, build a fresh
  `Child`, insert its root into the item's component hole
  (`h.insert(f.hole(di), inst.frag.roots()[0], 0)` — the M10 attach
  contract, one insert per mount), and store it at the live index.
- **Remove** — before `list_remove` unmounts the item, take the instance
  at that index and tear its slab down (`inst.frag.teardown(h)`). Child
  roots hang inside the item's holes, so the LIFO ordering is child-first,
  then the item.
- **Move** — after `list_move`, reposition the SAME instance
  (`remove(from)` + `insert(to)` — the exact `Vec` semantics
  `Fragment::list_move` applies to the items). Identity survives reorders:
  no teardown, no rebuild, the node handles are stable. The demo pins this
  by comparing `NodeId`s across a keyed rotation.

Unkeyed lists mirror their Index diff the same way (inserts at the tail
indices, removals in reverse). The store starts empty — items mount on the
first reconcile, which the initial flush schedules like any bind.

### 2. The per-item replay drives the props

Item-body reactions are excluded from the component's `run()` dispatch (the
M5 rule that routes item binds through the reconcile replay). The component
use's prop bindings follow the same discipline: the reconcile's replay loop
calls a generated per-use children fn (`children_f{body}_d{di}`) per live
item, which applies every prop binding (item/index locals substituted,
parent-state reads pre-cloned into the call) and flushes the child. A use
with no dynamic props needs no replay fn — its instance is prop-stable
after the initial sets. `collect_body_reads` walks component prop values
too, so parent-state-driven props pre-clone with the body's own reads.

### 3. Branch-unmount disposal drains the stores

A branch whose subtree contains list bodies with component uses tears its
whole fragment tree down on unmount — items included. The mounting arm's
dispose list (M10) drains each store in the subtree
(`for inst in self.<field>.drain(..) { if let Some(c) = inst { … } }`),
idempotent like the `Option::take` list it joins: a remount rebuilds
everything through the fresh branch's mount marking and the list's first
reconcile.

### 4. The boundary stays fail-closed, redrawn one step

- Component uses inside list-body **branches** (an `If` inside a `For`
  body) keep the sticky denial — branch-in-item composition remains the
  follow-up, and the codegen backstops stay as the second line.
- Slot outlets inside list bodies keep the M10 denial (per-item slot
  assembly — fills/fallbacks per item — is its own milestone).
- Events inside list bodies keep `LIG-E118` (per-item dispatch).
- The IR schema is untouched (v5): a `ComponentIr` on a For body was always
  representable — the milestone changes WHO mounts it, not WHAT the IR
  says. Both legs converge on the same canonical IR (`list-component`
  fixture, byte-identical modulo the parity-ignored fields).

### 5. The per-item replay covers every live item (both plans)

The M5 keyed plan already replayed every item; the unkeyed Index diff only
refreshed survivors (`.take(common)`) — appended items' binds never
initialized until the next reconcile. M13 applies one rule to both plans:
the replay covers EVERY live item. The unkeyed-list fixtures' regenerated
artifacts show the extended replay; the per-item component props make the
survivor-only shape unambiguously wrong (a fresh instance would hold
default props forever).

## Consequences

- The composition matrix is complete for components: a component use
  compiles in content, in branches, in else legs, and in list bodies —
  always with the identity semantics its container demands (eager,
  per-mount, per-item).
- Zero runtime changes: the `Fragment` list table, `plan_keyed`, and the
  attach/teardown primitives are pre-existing machinery. Zero IR schema
  changes. Zero new external dependencies.
- The bench ledger grows the `codegen_list_component` row (M13 reference
  median 901 µs against a 4 800 µs ceiling, ≈5.3× headroom); 15 ledger
  rows, all enforced in release.
- The counter unit is byte-stable (7 385 B, digest unchanged) — the
  milestone's generated-code growth is confined to units that actually use
  the surface.
- Still deferred, deliberate: per-item slot assembly (outlets in list
  bodies), branch-in-item component composition, per-item events
  (`LIG-E118`), per-item refs (the store's `Option` slots are the base),
  cross-component branch-fill coordination, and `cargo vet`.
