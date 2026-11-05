# M2 milestone report — Vue frontend, two-leg parity, gates in the CLI

Date: 2026-11-06 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (§17.1 M2)

Vue frontend + parity legs + list/array admission-tier completion, and the
xtask→CLI gate migration.

| Exit criterion | Result |
| --- | --- |
| **Vue SFC frontend** | ✅ `lig-parse-vue` — hand-rolled, fail-closed `<template>` parser and `<script setup>` lowering to the canonical View IR (ADR-014): `ref`/`computed`/`watchEffect`/`defineProps` normalization, `v-if`/`v-else-if`/`v-else` chains, `v-for` with index, delegated events with synthesized `setX` model fns, `$event` payload reads, mixed style objects (`LIG-W102` + per-property `StyleProp`), slots via a synthesized `children` prop, and multi-section modules for cross-component fixtures. |
| **Two-leg parity green** | ✅ `lig fixtures verify` — all **18 fixtures × 2 legs**: the Solid leg byte-exact against the committed contract, the Vue leg identical modulo the parity-ignored fields (frontend header, source-file identity, spans — §6.4, ADR-014 §5). |
| **List/array tier completion** | ✅ getter-only signal destructures (`const [items] = createSignal(…)`), `<For>` index accessors (`(item, index)`, `i()` reads), and array-spread list updates (`setItems([...items(), x])` → the new whitelisted `arr.concat` builtin + `lig_runtime::pure::concat`) — pinned by two new fixtures (`for-index`, `array-update`) on **both legs**. |
| **xtask removed** | ✅ all eight gates are native `lig` subcommands (`fixtures gen/verify/parity-gen/admission/rules-sync`, `reproducibility`, `patch-spike`, `errors-gen`) — the gate tool ships inside the compiler CLI, with the xtask-quality harnesses (line-checked admission, NFR-5 render checks) replacing the weaker M1 CLI duplicates. Zero `xtask` references remain outside historical reports. |

## Canonical-contract corrections (parity-forced, reviewed like code)

- **Conditional fragments renamed to `branch_N`** (was `then_N`/`els_N` for
  ternaries): the IR must not encode which surface syntax produced a branch —
  ternary, `Show`, and `v-if` are the same concept. `ternary-child` regenerated.
- **`Show`'s `fallback` lowers to the else fragment** — M1 silently dropped it.
  `show-conditional` now pins what its name always claimed.
- **A bare `createEffect(…)` statement lowers** — M1 dropped the idiomatic
  spelling; `effect-log` regenerated with an admitted body (`console.*` stays
  outside the pure whitelist).
- `assign_anchor_paths` → `lig-ir::anchor`, `collect_reads` → `lig-ir::expr`,
  the expression typer → `lig-expr::ty`, `camel_to_kebab` → `lig-style`:
  frontend-agnostic helpers now live with the IR, not inside one frontend.

## Engineering baselines held

- **TDD**: 199 tests across 37 test binaries; the Vue frontend landed with
  unit tests (SFC scanner, template parser, lowering) plus integration
  coverage through the two-leg parity gate and the CLI surface (`tests/cli.rs`
  now locks the gate subcommands too).
- **clippy pedantic**: `cargo clippy --workspace --all-targets -- -D warnings`
  green, including benches; no new allowlist rows.
- **Benches from day one**: `cargo bench --workspace --no-run` is a CI gate;
  `lig-parse-vue` ships a `vue/counter` lower bench alongside the Solid one.
- **CI/CD**: `.github/workflows/ci.yml` runs the parity/admission/rules-sync/
  reproducibility/patch-spike gates through `cargo run -p lig-cli` (the
  malformed `branches:` selector from M1 is fixed); release flow unchanged.
- **git-cliff**: changelog continues on conventional commits (`v0.1.0-m2`).

## Full gate run at delivery

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ |
| `cargo test --workspace` | ✅ 199 tests / 37 binaries |
| `cargo bench --workspace --no-run` | ✅ |
| `lig fixtures verify` | ✅ 18/18 fixtures, both legs |
| `lig fixtures admission` | ✅ 22/22 cases (NFR-5) |
| `lig fixtures rules-sync` | ✅ 11 rules 1:1 with fixtures |
| `lig reproducibility` | ✅ byte-identical (7 094 B counter unit) |
| `lig errors-gen` | ✅ 15 codes, regeneration is a no-op |
| `lig patch-spike` | ✅ save→frame **118–140 ms** < 1 s budget |

## Known gaps (deferred, deliberate)

- Named slots and `<slot>` fallback content; `v-model`; `watch`/`watchEffect`
  with multi-statement bodies: the IR models the default outlet only (v2
  schema) — later tier work.
- Keyed `For` (`key` expression → LIS diff): the schema exists, both
  frontends emit index-diff lists at M2.
- Salsa incremental, digest gating, watch mode, ThinLink patch engine: M3.
- Budgets + `no_std` proof + Appendix-A docs: M4.
