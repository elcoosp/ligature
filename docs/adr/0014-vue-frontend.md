# ADR-014: the Vue SFC frontend and the two-leg parity contract

- Status: accepted (M2)
- Date: 2026-11-06

## Context

FR-1 names two frontends over one canonical View IR: Solid TSX (M1) and Vue
SFC. The M1 fixture contract reserved the `input.vue` leg of every
conformance fixture (fixtures/README.md) and the M1 report deferred "Vue
frontend + parity legs" to M2. This ADR fixes the admitted Vue subset, the
`<script setup>` → state/reaction normalization, and the exact meaning of
"the shared contract both frontends must produce".

## Decision

### 1. The admitted `<template>` grammar (fail-closed, `LIG-E001`)

Elements, static attributes, `:k`/`v-bind:k`, `@ev`/`v-on:ev`,
`v-if`/`v-else-if`/`v-else`, `v-for` (`item in list`,
`(item, index) in list`), `v-html`, text with `{{ … }}` interpolations, and
HTML comments. Everything else — `v-model`, named slots, `<slot>` fallback
content, `<template>`/`<script>` inside templates, multi-root templates —
is denied. The parser is hand-rolled over the raw template text with
absolute byte offsets; template expressions are re-parsed out of a
blank-padded copy of the `.vue` source so spans (and diagnostics) land in
true source positions (oibraries the `is_dynamic` port for the
static-inline decision, §6.1).

### 2. The admitted `<script setup lang="ts">` subset

`ref(init)` → signal, `computed(() => e)` → memo, `watchEffect(…)` →
post-tier effect, `const { … } = defineProps<{ name: type }>()` → prop
states (the Vue 3.5 reactive-props destructure), single-expression named
handlers, `defineOptions({ name: "…" })`, and `vue` imports. States
materialize props-first, then in declaration order — the Solid
`scan_scope` order — so the canonical `states[]` order matches across legs.
Anything else is denied.

### 3. Multi-section modules

A `.vue` file admits one or more SFC *sections* (optional `<script setup>`
+ one `<template>` each). Single-section files are plain SFCs named by the
file stem's PascalCase; multi-section files (the fixture/module form for
cross-component behavior like `component-use`) require `defineOptions` in
every section. Production SFCs stay one-component-per-file; the grammar
admits both because the canonical IR is a *module* with N components, and a
template-based frontend needs a module form to pin cross-component
fixtures.

### 4. Normalization table (Vue → canonical IR)

| Vue | IR |
| --- | --- |
| `const x = ref(init)` | `StateDecl { kind: signal, init }` |
| `const x = computed(() => e)` | `StateDecl { kind: memo }` + memo reaction |
| `watchEffect(() => e)` / `state.value = e` inside it | post-tier effect; `state.value = e` lowers as a synthesized model-fn call |
| `const { p } = defineProps<{ p: t }>()` | `StateKind::Prop` states + component props |
| `<slot />` | `Slot { name: "default" }` dynamic + a synthesized `children` prop (rides last, §7.4) |
| `@ev="state = expr"` | `Event` dynamic whose body is a synthesized `set<State>` model-fn call (`set` + PascalCase — matches the Solid `setX` convention) |
| `@ev="handler"` | the named handler's body lowers at the site, with the handler's parameter as the event-object parameter |
| `$event.currentTarget.value` | the admitted event-field shapes (§6.1), `$event` as the parameter name |
| `v-if`/`v-else-if`/`v-else` | `If` dynamic + `branch_N` fragments (chains nest, C4) |
| `v-for="item in list"` / `(item, index)` | `For` dynamic + `items_N` fragment; the index is a local read |

Template assignments (`count = count + 1`, compound forms, `state.value =
expr` in script) synthesize a model fn named `set<State>` and record the
write — the §6.3 setter normalization with Vue spelling. Fixture authors
keep `ref` names aligned with the Solid leg's `setX` destructures.

### 5. The parity contract, made precise (§6.4)

"Two frontends produce byte-identical canonical JSON" means: identical
after normalizing the **parity-ignored fields** — the `frontend` header,
the source-file identity (`source_files[].path`/`hash`), and every span
(expr spans, dynamic spans, lint positions, the span table). Ids, `reads`,
fragment `children`, strings, templates, styles, and reactions must be
identical. `lig fixtures verify` enforces this on both legs of every
fixture; the Solid leg additionally stays byte-exact against the committed
`expected.ir.json`.

### 6. Canonical-contract cleanups forced by parity (deliberate)

- **Conditional fragment names unify to `branch_N`** (Solid's ternary
  spelled `then_N`/`els_N` through M1). The canonical IR must not encode
  which surface syntax produced a branch — ternary, `Show`, and `v-if` are
  the same IR concept. `ternary-child` regenerated.
- **`Show`'s `fallback` lowers to the else fragment** — M1 silently dropped
  it (`else_: null`, the fallback string never interned). `show-conditional`
  regenerated; its stated purpose ("Show with fallback branch") is now
  actually pinned.
- **A bare `createEffect(…)` statement lowers** — M1 only lowered the
  `const _ = createEffect(…)` spelling and silently dropped the idiomatic
  statement form. `effect-log` regenerated; its body moved onto the
  admitted expression grammar (`console.*` stays outside the pure-builtin
  whitelist — effects are lowered, not logged).

### 7. Anchor assignment and expression typing are frontend-agnostic

`assign_anchor_paths` moves to `lig-ir::anchor`, `collect_reads` to
`lig-ir::expr`, and the small-lattice typer (`ty_of`, `value_kind_is_str`)
to `lig-expr::ty`; `camel_to_kebab` moves to `lig-style`. Both frontends
call the same helpers — anchor walks, dataflow edges, and state types are
properties of the canonical IR, not of the surface syntax.

## Consequences

- `lig-parse-vue` mirrors `lig-parse-solid`'s public shape
  (`lower_module(source, path) -> Result<ViewModule, Vec<LigDiagnostic>>`),
  so the CLI, the parity harness, and future frontends treat them
  uniformly; `lig check/build/inspect` dispatch on the file extension.
- The 18 conformance fixtures each carry both legs; the `fixtures verify`
  gate fails on a missing or drifting `input.vue`.
- `console.log` is still outside the pure whitelist — effect bodies express
  their dataflow (e.g. `log.value = tick.value.toString()`), which is what
  the canonical reaction graph models anyway.
- Named slots and slot fallback content remain unadmitted; the IR models
  the default outlet only (v2 schema's `SlotIr` covers named fills at the
  *use* site, which is what `component-use` pins).
