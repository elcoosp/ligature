# Conformance fixtures (§4)

Each fixture directory is a behavior contract, named for the Vapor behavior it
pins. The full triple form is:

```
fixtures/conformance/<behavior>/
  input.solid.tsx     # Solid frontend input
  input.vue           # Vue frontend input (lands with the Vue frontend, M2)
  expected.ir.json    # canonical View IR v2 — the shared contract both
                      # frontends must produce (modulo the `frontend:` header)
```

## M0 scope (S4)

Twelve **Solid-leg** behavior fixtures translated from the Vapor behavior
list, with the canonical-JSON harness (`xtask parity`) byte-comparing the
lowering against the committed `expected.ir.json`:

| Fixture | Behavior pinned |
| --- | --- |
| `class-binding` | dynamic `class={expr}` (incl. ternary expression) |
| `counter` | signal + memo + delegated click + `Show` + static class + style consts |
| `delegated-events` | multiple delegated events, setter calls, pure builtins |
| `destructure-props` | destructured prop declaration + prop reads |
| `dynamic-attribute` | dynamic attribute (`src={expr}`) |
| `effect-log` | `createEffect` reaction (post tier, write-masked) |
| `map-children` | `.map` child expression (unkeyed list) |
| `memo-derived` | `createMemo` read in a text hole |
| `show-conditional` | `Show` with `fallback` branch |
| `static-template` | fully static subtree → template inlining, zero dynamics |
| `style-object` | static style object → const style profile |
| `ternary-child` | conditional child with element branches |

## Rules

- **Golden files**: `expected.ir.json` is regenerated only with
  `cargo run -p xtask -- parity-gen` and reviewed like code — it *is* the
  executable definition of the frontend contract.
- **A new frontend feature requires a new fixture before the lowering
  merges** (draft V.6 policy, kept).
- **Fail-closed**: a fixture that stops lowering (any `LIG-E001`) fails the
  harness; there is no "expected error" escape at M0.
- The Vue leg is exercised by the same harness from M2; parity is claimed
  only when a fixture has both legs and the harness is green on both.
