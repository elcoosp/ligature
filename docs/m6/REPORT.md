# M6 milestone report — template surface completion (slots, v-model, multi-statement effects, `.map` keys)

Date: 2026-11-06 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (§17.1 M6)

The four constructs every report since M2 carried as "the IR models the
default outlet only (v2 schema)": named slots with `<slot>` fallback
content, `v-model`, multi-statement effect bodies — plus the `.map` key
M5 itself deferred. One IR field (`Slot.fallback`, v4 → v5), no new
crates, no new gates, no new error codes, no new external dependencies.

| Exit criterion | Result |
| --- | --- |
| **Named slots + `<slot>` fallback (§7.4)** | ✅ `<slot name="header">fallback</slot>` / `<Slot name="header">fallback</Slot>` lower to `DynamicKind::Slot { name, fallback }` — the outlet owns its fallback fragment; fills (`<template #header>` on the Vue leg, element-valued props `header={<h1>…</h1>}` on the Solid leg) match outlets by name. A bare `<slot />` keeps the synthesized `children` prop; named outlets synthesize nothing. The SFC scanner balances nested `<template>` elements. |
| **Per-use-site mounting** | ✅ The parent's `build` mounts each use site's fill by name, else the outlet's fallback, else nothing — fill statics ride the parent's prefix, fallback statics the child's, both in the same generated module scope. Two latent M0-era defects fixed with it: the fill lookup matched the first component dynamic of that component anywhere in the module (wrong fill under multi-use), and multi-use parents collided on one struct field (now deterministic `panel_2`-style suffixes). The committed `tests/compile-clean/src/gen/slots_named.rs` pins the emission. |
| **`v-model` (§7.6 sugar)** | ✅ `<input v-model="email" />` produces byte-equal IR to the explicit `:value` + `@input` spelling — the `value` Attr dynamic plus the input Event dynamic calling the synthesized setter with `$event.currentTarget.value`. Admitted on `<input>` only, bound expression must be a plain state identifier, everything else fail-closed (`LIG-E001`/`LIG-E100`). A dedicated unit test pins sugar ≡ explicit. |
| **Multi-statement effects (§7.1)** | ✅ `createEffect(() => { s1; s2; })` / `watchEffect(() => { … })` lower one post-tier reaction per expression statement — per-statement reads, exact schedules, zero IR change. Two M0-era fail-open holes closed: block bodies were silently truncated to the first statement, and empty bodies lowered to nothing with no diagnostic. Non-expression statements stay denied; an effect writing a field it reads trips the E401 flush cycle guard (pinned). |
| **`.map` keys (§7.5 follow-up)** | ✅ `items().map((item) => <li key={item}>{item}</li>)` — the React-style spelling on the mapped element — strips the attribute and lowers to `ForIr.key` (plain expression over the item, or an arrow taking exactly the item binding). M5's key rules unchanged: state reads, static keys, and duplicates denied. The generated unit runs the M5 `plan_keyed` reconcile unchanged. |
| **Slot-content contract** | ✅ Fills and fallbacks are static subtrees — the runtime has no run arms addressing a slot instance's holes, so both frontends deny dynamic binds inside slot content (`LIG-E001`) instead of emitting a unit that writes nowhere. |

## Fixture

✅ Four new behavior fixtures, both legs each (19 → 23):

| Fixture | Pins |
| --- | --- |
| `slots-named` | Two Panel uses in one parent: use 1 fills `header` (element-valued prop / `<template #header>`) and the default outlet; use 2 fills only the default outlet, so the `header` outlet shows the child's own `<em>no header</em>` fallback. One canonical IR: `Slot { name, fallback }` outlets, `SlotIr` fills, both mounting paths in one generated unit. |
| `v-model` | Solid's explicit `value`/`onInput` pair vs Vue's `v-model` — identical IR, modulo the parity-ignored fields. |
| `effect-multi` | A two-statement block body → two post-tier reactions, each with its own reads (`tick` for both; the fixture writes each target from the tick, since a statement reading what it writes is a genuine E401 cycle). |
| `map-keyed` | `key={item}` on the mapped element / `:key="item"` on the `v-for` element → `ForIr.key = Local(item)`, the key stripped from the item template. |

The KR-1 demo pins the runtime behavior on the mock host: per-use-site
slot mounting (`slot_is` on both Panel instances — fill on use 1,
fallback on use 2), the v-model payload write, and the two-reaction
effect flush.

## Engineering baselines held

- **TDD**: 255 tests across 39 test binaries (M5: 249/39). Each construct
  landed with unit tests on both frontends (slot shape validation,
  v-model sugar ≡ explicit, multi-statement statement rules, `.map` key
  rules, the SFC nested-template scan) plus the four two-leg fixtures and
  three new demo assertions.
- **clippy pedantic**: `cargo clippy --workspace --all-targets -- -D
  warnings` green, benches included; no new allowlist rows.
- **Benches from day one**: `cargo bench --workspace --no-run` stays a CI
  gate; the M6 features ride the existing workloads (no new bench
  surfaces — slot mounting is build-time, the `.map` key runs the M5
  planner).
- **CI/CD**: unchanged job set — the new fixtures ride the parity gate,
  the new gen modules ride compile-clean; no new gate, no new error code.
- **git-cliff**: changelog continues on conventional commits (`v0.1.0-m6`).
- **Zero new external dependencies**: the only Cargo change is a
  test-only `serde_json` dev-dependency on `lig-parse-vue` (the IR-shape
  unit test); the dependency graph of every compiler crate is unchanged.

## Full gate run at delivery

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ |
| `cargo test --workspace` | ✅ 255 tests / 39 binaries |
| `cargo bench --workspace --no-run` | ✅ |
| `lig fixtures verify` | ✅ 23/23 fixtures, both legs |
| `lig fixtures admission` | ✅ 22/22 cases (NFR-5) |
| `lig fixtures rules-sync` | ✅ 11 rules 1:1 with fixtures |
| `lig reproducibility` | ✅ byte-identical (7 146 B counter unit — none of the M6 paths touch a component-free module) |
| `lig incremental` | ✅ 6 passes green (D-9 + LIG-E020) |
| `lig errors-gen` | ✅ 15 codes + 15 embedded docs, regeneration is a no-op |
| `lig patch-spike` | ✅ two generations, **263 ms** total, < 1 s per save |
| `lig budgets` (release) | ✅ all 10 rows within budget — keyed row 3.9 µs / 25 µs (6.4×), codegen 561 µs / 3 000 µs |
| `lig no-std` | ✅ proof green (2 bare-metal targets + the generated unit) |
| `lig doctor` | ✅ all environment checks pass (view IR v5) |

## Notes and decisions

- **Why the outlet owns the fallback**: the fallback is authored where
  the outlet is (the child's template), it mounts only when some use site
  declines to fill, and it belongs to the child's fragment tree — the
  outlet dynamic is the natural owner. `SlotIr.fallback` keeps its
  M0-era role (the parent-provided fill) so the fill side of the schema
  is untouched; the two meanings never meet in one field.
- **Why fill statics and fallback statics can share one emitted module**:
  `generate_module` emits every component's pieces into one scope, so the
  parent's build references the child's fallback templates directly. The
  single-component `generate_component` (the counter's path) never
  crosses that boundary — and it already references child types by name
  for component uses, so the module form remains the self-contained
  artifact.
- **Why `v-model` lowers to `CurrentTargetValue`** (not Vue's
  `e.target.value`): handlers are delegated (C7) — `currentTarget` is the
  element carrying the listener, which is what the sugar binds to; it
  matches the Solid leg's `(e) => setX(e.currentTarget.value)` and keeps
  the two legs byte-equal.
- **Why one reaction per statement instead of a block IR**: the
  const-scheduled model is a flat reaction table; sequencing inside one
  effect body would need an ordered body construct in the IR and a block
  executor in the runtime. Per-statement reactions give the same
  execution order through the existing schedule (topological, then
  declaration order), keep reads/writes exact per statement, and cost
  zero IR/runtime surface.
- **Why the mapped element's `key` is stripped like Vue's `:key`**: the
  key drives diffing, it is not an attribute of the item template — the
  same rule ADR-018 §1 established for `v-for`; the Solid leg now follows
  it with a consumed-root-attribute flag during body lowering.

## Known gaps (deferred, deliberate)

- Dynamic slot content: fills/fallbacks are static subtrees (denied at
  lowering); lifting the restriction needs slot-scoped run arms and an
  init-flush story for mounted slot instances.
- `v-model` on non-input elements (checkbox/radio/select) and
  `<slot :name>` forwarding: fail-closed today.
- `cargo vet` (§14.2 supply-chain onboarding, ADR-012): still deferred —
  the pin table remains the reviewed surface.
- A `lig patch` developer driver over real app dylibs (app-crate scaffold
  + dev channel): the engine surface (`PatchSession` / `apply_patch` /
  `hot_call` / `PatchManifest`) is what that work wires into.
