# ADR-019: M6 — template surface completion (§7.4 slots, §7.6 v-model, §7.1 multi-statement effects, §7.5 `.map` keys)

- Status: accepted (M6)
- Deciders: compiler team
- Date: 2026-11-06

## Context

With M5's keyed `For` shipped, the longest-deferred *compiler surface*
items were the four constructs every report since M2 carried as "the IR
models the default outlet only (v2 schema)": named slots with `<slot>`
fallback content, `v-model`, multi-statement effect bodies, and — new in
M5's own gap list — a `key` option on `.map` children. The IR already
sketched two of them (`DynamicKind::Slot`, `SlotIr.fallback`), but the
frontends denied every named-slot spelling, `<slot>` children, `v-model`,
and block-bodied effects; nothing in the compiler produced an outlet
fallback or a multi-reaction effect body. M6 closes all four end to end:
both frontends, the canonical IR, codegen, and one fixture per construct.

## Decision

### 1. The slot outlet owns its name and its fallback (IR v5)

`DynamicKind::Slot { name, fallback: Option<FragmentId> }` — the outlet
gains its fallback content as a nested fragment. `VIEW_IR_VERSION` bumps
4 → 5; every committed `expected.ir.json` regenerated (`"fallback": null`
on modules whose outlets carry none). The fill side is unchanged:
`ComponentIr.slots` keeps `SlotIr { name, fallback }` (the M0 naming —
the field holds the parent-provided content), matched against outlets by
name. Duplicate outlet names in one fragment are a lowering error; a
dynamic `:name` on `<slot>` is denied — slot names are identities.

### 2. Surface syntax: idiomatic on each leg, one canonical shape

- **Vue outlet**: `<slot name="header">fallback</slot>` — the children
  lower to the outlet's fallback fragment (one static element; anything
  else fail-closed). A bare `<slot />` stays the default outlet and keeps
  the synthesized `children` prop; a *named* outlet synthesizes nothing.
- **Solid outlet**: `<Slot name="header">fallback</Slot>` — a ligature
  extension in the `<Show>`/`<For>` tag position (real Solid passes slots
  as props; this compiler spells the outlet explicitly). `<Slot />` is
  the default outlet, equivalent to the existing `{children}` read.
- **Vue fill**: `<template #header>…</template>` (longhand
  `v-slot:header` included) on a component element — multiple named fills
  admitted in child order; the wrapper's single static element child is
  the fill fragment. Plain element children stay the default fill.
- **Solid fill**: an element-valued prop — `header={<h1>…</h1>}` (braces,
  the idiomatic JSX spelling) or `header=<h1>…</h1>` (bare). They are
  slot content, not prop bindings; `children={<…/>}` is denied (element
  children are the default slot).
- The SFC scanner now balances nested `<template>` elements (a named fill
  no longer closes the enclosing section); a `<template>` reaching plain
  element lowering is fail-closed.

### 3. The v1 slot-content contract: static subtrees

Fills and fallbacks mount as static subtrees — the runtime addresses a
slot instance's holes through no run arms, so dynamic binds inside slot
content would write nowhere. Both frontends deny dynamics (and nested
fragments) inside fill/fallback content with `LIG-E001` rather than emit
a broken unit. The component-use fixture's fill was already static; the
rule is now explicit and enforced. Lifting it needs slot-scoped run arms
(a follow-up, not a patch).

### 4. Slot mounting is per use site, and the child's prefix is fair game

The parent's `build` mounts each used child's outlet content: this use's
fill by name, else the outlet's own fallback, else nothing. The fill
fragment lives in the parent's tree, the fallback in the child's — both
statics are emitted into the same generated module scope, so the emitted
mount references the owning component's prefix either way. Two latent
M0-era defects fall out of the rework: the fill lookup searched the whole
module for the *first* component dynamic of that component (wrong fill
under multi-use parents), and two uses of one component collided on a
single struct field. Both fixed; a second use gets a deterministic
numbered field (`panel_2`), and `lig reproducibility` pins the unchanged
counter unit byte-for-byte.

### 5. `v-model` is sugar with byte-equal IR

`<input v-model="email" />` lowers to exactly what the explicit spelling
produces: a `value` Attr dynamic reading the state, plus an `input` Event
dynamic calling the synthesized setter with `$event.currentTarget.value`
(the Vue leg's model-fn shape from M2, `EvField::CurrentTargetValue` so
delegated handlers see the listening element). Admitted on `<input>`
only; the bound expression must be a plain state identifier; anything
else fail-closed. The `v-model` fixture's two legs (Solid's explicit
`value`/`onInput` pair vs Vue's `v-model`) produce one canonical IR.

### 6. Multi-statement effect bodies: one reaction per statement

`createEffect(() => { s1; s2; })` / `watchEffect(() => { … })` lower each
expression statement to its own post-tier effect reaction — reads stay
per statement, the const schedule stays exact, and no IR change is needed
(reactions are a table). Before M6 the block body was silently truncated
to its first statement, and an empty block lowered to *nothing* with no
diagnostic — both now fail closed. Non-expression statements
(declarations, `if`, `return`) remain denied. A statement that both reads
and writes the same field is a genuine reactive cycle; the E401 tripwire
fires at flush (the fixture pins statements that write from a distinct
reader).

### 7. `.map` children take a key (§7.5 follow-up)

The React-style spelling on the mapped element — `items().map((item) =>
<li key={item}>{item}</li>)` — turns the list keyed: the attribute is
stripped from the item fragment and lowered to `ForIr.key` (a plain
expression over the item, or an arrow taking exactly the item binding).
The M5 rules apply unchanged: state reads denied, static keys denied,
duplicates denied. The Vue leg is `:key` on the `v-for` element (M5), and
the two legs share `ForIr.key` bit-for-bit.

## Consequences

- View IR v5; the schema delta is one field (`Slot.fallback`), regenerated
  across the 23 fixtures' `expected.ir.json`.
- Four new behavior fixtures (`slots-named`, `v-model`, `effect-multi`,
  `map-keyed`) ride the existing gates — no new gate, no new error code,
  no new external dependency (the `serde_json` dev-dependency on
  `lig-parse-vue` is test-only).
- The counter unit's generated text is byte-identical (7 146 B): none of
  the M6 paths touch a component-free module.
- Deferred, deliberate: dynamic slot content (needs slot-scoped run
  arms), `cargo vet`, the `lig patch` developer driver, named-slot
  forwarding (`<slot :name>`), and `v-model` on non-input elements.
