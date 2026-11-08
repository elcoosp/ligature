# M12 milestone report — template refs on the Vue leg

Date: 2026-11-09 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (frontend-parity completeness)

The deep dive's finding: the `Ref` dynamic — part of the canonical
contract since M2, working end to end on the Solid leg — had NO Vue
template spelling, and its two accidental spellings silently mis-compiled.
M12 closes all three:

| Exit criterion | Result |
| --- | --- |
| **The placeholder declaration** | ✅ `let box;` in `<script setup>` is admitted as a placeholder local (M12's `ScriptDecl::Placeholder`): no state, no reactions, recorded in the lowering's non-reactive local set (the `v-for` item/index set). The Solid leg's bare `let box;` lowers to nothing — the Vue leg now produces the identical IR shape through an explicit, validated declaration instead of a denial. Destructuring patterns stay denied (the pre-M12 `LIG-E001` text). |
| **The static spelling** | ✅ `ref="box"` lowers through the capture contract (`lower_template_ref`): `DynamicKind::Ref` over `Expr::Local(box)`, empty `reads` — and the element's static attribute table no longer carries a literal `ref` attribute. Pre-M12, the attribute rendered as dead HTML while the script's name stayed unconnected (the fail-open). |
| **The bound spelling** | ✅ `:ref="box"` / `v-bind:ref="box"` lower to the SAME dynamic (the longhand test pins the three spellings' convergence). Pre-M12, the bound form became `DynamicKind::Attr { key: "ref" }` — a dynamic attribute write, never a node capture. |
| **Fail-closed boundaries, drawn precisely** | ✅ Bare `ref` → `LIG-E001` (no target). `ref`/`:ref` naming a SIGNAL → `LIG-E001` (a capture would shadow the setter write path; two-way binding is the `v-model` discipline). `ref="x"` naming nothing → `LIG-E100` (the unknown identifier, the same code the bound form's lowering produces). `:ref` over a non-`Local` shape (members, calls, binary shapes, signal reads) → `LIG-E001` ("template refs bind a placeholder local"). The codegen `Ref` arm's "local only" backstop is unchanged — admission owns the diagnosis first. |
| **One canonical IR, both legs** | ✅ The `template-ref` fixture pins the parity: `<input ref={box} />` (Solid) and `<input ref="box" />` (Vue) byte-compare against one committed `expected.ir.json` (modulo the parity-ignored fields) — 29 → 30 fixtures, both legs, `lig fixtures verify` green. |

## Fixtures

✅ One new behavior fixture, both legs (29 → 30):

| Fixture | Pins |
| --- | --- |
| `template-ref` | `let box;` + `<input ref="box" />` / `<input ref={box} />` — one canonical IR (the `Ref` dynamic, no state, no rendered attribute) and one generated shape (`pub ref_box: NodeId`, sentinel init, build-body capture). The demo pins the runtime half: after build the field holds the mounted `<input>`'s node, not the `NodeId(u32::MAX)` sentinel. |

## Engineering baselines held

- **TDD**: 297 tests across 39+ binaries (M10: 287/39). The M12 shapes
  landed red-first: the eight frontend contract tests (the placeholder
  declaration, the three spellings' convergence, the state/unknown/
  non-local/bare denials, the both-legs parity assertion) and the two
  codegen contract tests failed at lowering before the implementation
  existed — the static and bound spellings lowered "successfully" as the
  fail-open shapes, which is exactly what the red tests caught.
- **clippy pedantic**: `cargo clippy --all-targets --all-features -- -D
  warnings` green, benches included. Zero new suppressions — the tree
  still carries zero `#[allow]`/`#![allow]` attributes.
- **Benches from day one**: `cargo bench --workspace --no-run` stays a CI
  gate; M12 adds the `codegen/ref` bench workload (module codegen over the
  template-ref fixture — the ref-field declaration, the sentinel init, the
  build-body capture) with a `codegen_ref` ledger row (§14.2): release
  median 234 µs against a 1 200 µs ceiling (≈5.1× headroom). 13 ledger
  rows, all enforced in release.
- **Byte-stability**: the counter component digest is unchanged
  (`0x85d06a5c087b3589`); the unit is byte-identical (7 385 B) — the
  milestone touches the Vue frontend, one fixture, and the ledger. `lig
  reproducibility` stays green.
- **Zero new external dependencies**: the Cargo graph is untouched (the
  two new dev-dependencies are workspace-internal: parse-vue ↔ parse-solid
  test-side cross-lowering for the parity assertions).
- **CI/CD**: unchanged job set — the new fixture rides the parity gate
  (30 fixtures, both legs), the new gen module rides compile-clean; the
  admission ladder is unchanged (22 cases, 11 rules — the M12 denials are
  `LIG-E001`/`LIG-E100` shapes, not new codes).

## Full gate run at delivery

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ (workspace table carries pedantic) |
| `cargo test --workspace` | ✅ 297 tests / 39 binaries |
| `cargo bench --workspace --no-run` | ✅ |
| `lig fixtures verify` | ✅ 30/30 fixtures, both legs |
| `lig fixtures admission` | ✅ 22/22 cases (NFR-5) |
| `lig fixtures rules-sync` | ✅ 11 rules 1:1 with fixtures |
| `lig reproducibility` | ✅ byte-identical (7 385 B counter unit — digest unchanged `0x85d06a5c087b3589`) |
| `lig incremental` | ✅ 6 passes green (D-9 + LIG-E020) |
| `lig errors-gen` | ✅ 15 codes + 15 embedded docs, regeneration is a no-op |
| `lig patch-spike` | ✅ two generations, < 1 s per save |
| `lig budgets` (release) | ✅ all 13 rows within budget — ref 234 µs / 1 200 µs (5.1×), branch 1 124 µs / 6 000 µs (5.3×), composed 1 001 µs / 6 000 µs, codegen 597 µs / 3 000 µs, unit 7 385 B / 8 192 B |
| `lig no-std` | ✅ proof green (2 bare-metal targets + the generated unit) |
| `lig doctor` | ✅ all environment checks pass (view IR v5) |

## Notes and decisions

- **Why this milestone**: M10's known-gaps list names the mechanism gaps —
  cross-component branch-fill coordination and the keyed-component story
  are the deep runtime/codegen follow-ups, and the else leg is in flight
  on its own branch (ADR numbering leaves 0024 for it). Template refs are
  the remaining gap in the repo's HEADLINE claim — "the same component
  spelled twice compiles to byte-identical IR": today a labeled-input
  component compiles on Solid and silently mis-compiles on Vue. Closing a
  contract hole with zero runtime changes restores the parity story
  without touching the in-flight work's territory.
- **Why admission owns the denials**: the codegen `Ref` arm already
  rejects non-`Local` targets ("ref must be a local binding in v1") — but
  that failure surfaces at codegen, after lowering "succeeded". The
  fail-closed ladder (§4) gives each shape its precise code at admission;
  the backstop stays as the honest second line.
- **Why the placeholder rides the local set, not the state set**: the
  `ref_<name>` capture field is generated-code state, not reactive state —
  a `StateDecl` would add a signal the Solid leg doesn't have (breaking
  IR parity) and would suggest the setter write path applies to a node
  handle. The local set is exactly where the `v-for` item binding (the
  other non-reactive `Local` source) already lives.
- **Why the static form validates like the bound form**: `ref="x"` is
  sugar for `:ref="x"` — one contract, two spellings. Emitting `LIG-E100`
  for an unknown static target mirrors what the bound form's lowering
  produces naturally, so both spellings fail with the same code and help.
- **ADR numbering**: 0024 is reserved by the in-flight M11 branch (the
  else leg); this milestone's ADR is 0025 to keep both branches mergeable
  without renumbering.

## Known gaps (deferred, deliberate)

- Component instance refs (`<Badge ref="b">`): `ref` passes through as a
  prop name on both legs (parity) — instance-handle refs would need
  per-use identity wiring (M10's `Option<Child>` fields are the base).
- Per-item refs in list bodies follow the Solid leg's existing shape; the
  keyed-component story (per-item instance identity) remains the named
  follow-up for components and outlets in lists.
- Cross-component branch-mount-time fill coordination: still fail-closed
  (the M10 unmatched-fill backstop) — unchanged, the deeper runtime
  follow-up.
- The else leg (`Show`/`v-if` fallback mounts): in flight on its own
  branch, not part of this tree.
- `cargo vet` (§14.2 supply-chain onboarding): still deferred.
