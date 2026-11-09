# Conformance fixtures (§4)

Each fixture directory is a behavior contract, named for the Vapor behavior it
pins. Since M2, the full triple form is mandatory:

```
fixtures/conformance/<behavior>/
  input.solid.tsx     # Solid frontend input
  input.vue           # Vue SFC input (the M2 leg)
  expected.ir.json    # canonical View IR v2 — the shared contract both
                      # frontends must produce
```

## The two-leg parity contract (M2)

`lig fixtures verify` checks **both legs** of every fixture:

- the **Solid leg** byte-compares against the committed `expected.ir.json`
  (trailing-newline tolerant, everything else byte-exact);
- the **Vue leg** compares against the same contract modulo the
  **parity-ignored fields** (§6.4, ADR-014): the `frontend` header, the
  source-file identity (`source_files[].path`/`hash`), and every span (expr
  spans, dynamic spans, lint positions, the span table). Ids, `reads`,
  fragment `children`, strings, templates, styles, and reactions must be
  identical.

A missing `input.vue` fails the gate: parity is claimed only when a fixture
has both legs and the harness is green on both.

Vue-leg conventions (ADR-014): every leg names its component with
`defineOptions({ name })` to match the Solid function name (`counter`'s leg
exercises the file-stem fallback instead); `ref` names align with the Solid
leg's `setX` destructures because template assignments synthesize
`set<State>` model fns; the `v-for` item/index and destructured prop names
match the Solid leg's bindings (the names ride the canonical `strings`
table).

## Behavior fixtures (34)

| Fixture | Behavior pinned |
| --- | --- |
| `array-update` | list state updated through a spread (`[...tags, "hot"]` → `arr.concat`) in an event handler |
| `class-binding` | dynamic `class` (incl. ternary expression) |
| `component-use` | component use + default-slot fill (multi-section Vue module: Panel + App) |
| `counter` | signal + memo + delegated click + `Show`-shape `v-if` + static class + style consts |
| `delegated-events` | multiple delegated events, setter calls, pure builtins |
| `destructure-props` | destructured prop declaration + prop reads |
| `dynamic-attribute` | dynamic attribute (`src`) |
| `dynamic-style` | mixed style object → per-property `StyleProp` dynamics + `LIG-W102` + const fold |
| `effect-log` | `createEffect`/`watchEffect` reaction (post tier, read-masked) |
| `effect-multi` | multi-statement effect bodies (M6): a two-statement `createEffect`/`watchEffect` block → two post-tier reactions with per-statement reads |
| `event-object` | `$event.currentTarget.value` payload flow |
| `for-index` | getter-only list signal + `<For>`/`v-for` index accessor |
| `for-event` | per-item event dispatch (M15): an `onClick`/`@click` handler DIRECTLY on the `<For>`/`v-for` body — one Attach commit per fresh item at its own node, index-addressed dispatch through the generated per-item fire fn with the item substituted into the handler body, keyed moves re-locate the handler record without re-attaching, removals dispose it with the item's slab, and stale indices are no-ops |
| `for-keyed` | keyed list reconciliation (M5): `<For key={(t) => t}>` / `v-for … :key="t"` — identity keys on both legs, `plan_keyed` LIS-stable moves in the generated unit |
| `for-list` | `<For>` mount over a list signal |
| `list-component` | per-item component identity in keyed lists (M13): a `<Badge count={t} />` inside a keyed `<For>`/`v-for` body — the generated `Vec<Option<Badge>>` store mirrors the keyed plan (inserts build + attach into the item's hole, removes dispose, moves reposition the same instance), and the per-item replay drives each instance's props |
| `list-slot` | per-item slot assembly in keyed lists (M14): a `<Slot name="row">` with an item-bound fallback directly on a keyed `<For>`/`v-for` body — every live item mounts the fallback into its outlet hole through the per-item replay (`mount_slot`, idempotent) with the item text bound, moves relocate the same mounted content without rebuilds, and removals dispose it with the item |
| `map-children` | `.map` child expression (unkeyed list; the Vue leg uses `v-for`) |
| `map-keyed` | keyed `.map` children (M6): `key={item}` on the mapped element / `:key="item"` on the `v-for` element — the key stripped from the item fragment, `ForIr.key = Local(item)` |
| `memo-derived` | `createMemo`/`computed` read in a text hole |
| `show-conditional` | `Show` with `fallback` / `v-if`/`v-else` at the template root |
| `show-else` | the else leg (M11): `fallback`/`v-else` content mounts when the condition is false — a dynamic bind on the else branch walks its branch ctx like its then twin, and a component use on the else branch rebuilds per mount (`Option<Child>`), inserts its root into the branch hole, and disposes on the flip back |
| `slots-branch-component` | per-mount component identity in content branches (M10): a `<Badge count={n()} />` inside an If branch on the header fill — the badge builds as App's `Option<Badge>` field, inserts into the branch's component hole, initializes through the mount marking, disposes on unmount, and rebuilds fresh on remount (a late write lands on the next mount) |
| `slots-branch-outlet` | branch-time slot assembly (M10): two use sites fill `header` with a div carrying a `<Slot name="inner">` inside an If branch — one fills `inner` (mounted when the branch mounts, through the composed accessor chain), the other leaves it unfilled (the outlet's own fallback mounts with the branch) |
| `slots-component` | component-in-content (M9): a fill carrying a `<Badge count={n()} />` use — the grandchild builds as the content author's field, inserts into the mounted content's component hole, and receives reactive props through the content author's prop-pass arm; the default `<Slot />` outlet carries the element-children fill |
| `slots-dynamic` | dynamic slot content (M7): a fill carrying a live text bind + a delegated event and a fallback carrying its own text bind — slot-scoped run arms address the mounted content (`slot_fill_mut`/`slot_fallback_mut`), the fill event attaches across slabs (`attach_foreign`), and the fallback bind initializes through the parent-driven init-flush |
| `slots-named` | named slots + fallback (M6): `<slot name="header">`/`<Slot name="header">` outlets with `<em>` fallback, `<template #header>` / element-valued-prop fills, two use sites in one parent (fill on one, fallback on the other) |
| `slots-nested` | nested outlets (M9): a fill carrying a `<Slot name="inner">` outlet on two use sites — one fills `inner` (mounted at content-mount time through the composed `slot_fill_mut` → `slot_fill_mut` walk), the other leaves it unfilled (the outlet's own fallback mounts through the `slot_fallback_mut` accessor); binds on both initialize at build and track updates through the composed walks |
| `slots-structural` | structural slot content (M8): a fill carrying an If with a disjoint-read bind (the mount-marking pin) and a keyed For; the unfilled outlet mounts the child's structural fallback, whose branch initializes through the subtree-wide init-flush |
| `v-model` | `v-model` two-way binding (M6): `<input v-model="email" />` on the Vue leg vs the explicit `value`/`onInput` pair on the Solid leg — byte-equal IR |
| `static-template` | fully static subtree → template inlining, zero dynamics |
| `style-object` | static style object → const style profile |
| `template-ref` | template refs on the Vue leg (M12): `ref="box"` / `:ref="box"` binding an element to a placeholder local (`let box;` in `<script setup>`) — the same `Ref` dynamic the Solid leg's `ref={box}` lowers to, no state for the name, the capture into the generated `ref_box: NodeId` field |
| `ternary-child` | conditional child with element branches (`branch_N` naming, both frontends) |

## Rules

- **Golden files**: `expected.ir.json` is regenerated only with
  `lig fixtures parity-gen` and reviewed like code — it *is* the
  executable definition of the frontend contract.
- **A new frontend feature requires a new fixture before the lowering
  merges** (draft V.6 policy, kept), on **both legs**.
- **Fail-closed**: a fixture that stops lowering (any `LIG-E001`) fails the
  harness; there is no "expected error" escape.
- **The Vue leg is not a translation of the Solid leg's text** — it is the
  idiomatic Vue spelling of the same UI, and the harness proves both spell
  the same canonical IR.
