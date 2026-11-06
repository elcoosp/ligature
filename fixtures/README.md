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

## Behavior fixtures (19)

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
| `event-object` | `$event.currentTarget.value` payload flow |
| `for-index` | getter-only list signal + `<For>`/`v-for` index accessor |
| `for-keyed` | keyed list reconciliation (M5): `<For key={(t) => t}>` / `v-for … :key="t"` — identity keys on both legs, `plan_keyed` LIS-stable moves in the generated unit |
| `for-list` | `<For>` mount over a list signal |
| `map-children` | `.map` child expression (unkeyed list; the Vue leg uses `v-for`) |
| `memo-derived` | `createMemo`/`computed` read in a text hole |
| `show-conditional` | `Show` with `fallback` / `v-if`/`v-else` at the template root |
| `static-template` | fully static subtree → template inlining, zero dynamics |
| `style-object` | static style object → const style profile |
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
