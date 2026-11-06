# M5 milestone report — keyed `For` (§7.5) end to end

Date: 2026-11-06 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (§17.1 M5)

The longest-deferred compiler item — open in every report since M2 — plus
the latent runtime bug the milestone's harness caught. No new crates, no
new external dependencies, no IR schema change (`ForIr.key` was already
v2), no new gate: the §7.5 keyed contract becomes real on both frontends.

| Exit criterion | Result |
| --- | --- |
| **Solid leg** | ✅ `<For each={tags()} key={(t) => t}>` lowers to `ForIr.key` — an arrow taking exactly the body's item binding, body a pure projection of the item (state reads denied with `LIG-E001`, non-arrow values denied, duplicate `key` attrs denied). Absent `key` keeps the unkeyed Index diff byte-for-byte (all 18 prior fixtures unchanged). |
| **Vue leg** | ✅ `:key="t"` / `v-bind:key="t"` on the `v-for` element lowers to the same canonical key shape (ADR-018 §1) and is stripped from the item fragment — the `li` template carries no `key` attribute on either leg. A static `key="…"` (same identity for every item) is fail-closed. |
| **Codegen** | ✅ keyed lists emit a `key_of_fN` fn (strings hash via `lig_runtime::key_hash` — FNV-1a 64, no_std; numeric/bool keys cast; list keys fail-closed) and a keyed `reconcile_fN`: build `old_keys` from the slab's stored keys, `new_keys` per item, run `plan_keyed`, and execute the sequential `SeqOp`s onto the existing slab ops (`list_remove`/`list_move`/`list_insert` with the plan's keys), then the standard full bind replay (index-driven binds track post-move positions). The committed unit `tests/compile-clean/src/gen/for_keyed.rs` is byte-pinned by the regen gates. |
| **Runtime planner** | ✅ `plan_keyed(old, new) -> Vec<SeqOp>` — the executable form of the keyed reconcile: removes (multiset-aware) then a left-to-right prefix walk, each step addressing the live list at apply time (a raw `keyed_diff` op list is *intent* — old-sequence indices corrupt under removals/moves; the harness proves it). Exhaustive pins: every 4-key permutation pair (24 × 24) plus shrink/grow sets land exactly on the new sequence. |
| **Latent bug fixed** | ✅ `keyed_diff` anchored its LIS over the surviving keys' *values* instead of the surviving old *indices* (Vue's `getSequence` criterion) — value-order and index-order disagree exactly on swaps, so `[2, 1] → [1, 2]` emitted zero moves and froze the swap. Fixed to LIS over indices, pinned by a dedicated test (ADR-018 §5). The M4 budget row `runtime_list_diff_keyed` now measures the shipped planner. |
| **Fixture** | ✅ `for-keyed` — 19th behavior fixture, both legs (`<For key>` / `:key`), one canonical contract: `ForIr { key: Local(item) }`, `"key": 1` in `expected.ir.json`. The KR-1 demo pins the runtime behavior on the mock host: a 3-item rotation is exactly one host `Move` with zero `Create`/`Destroy` — items move, they are not recreated — and remove+insert routes through the same diff. |

## Engineering baselines held

- **TDD**: 249 tests across 39 test binaries (M4: 235/39). The runtime
  planner landed with the permutation harness, the FNV-1a vectors, and the
  swap-criterion pin; each frontend landed with an admission-allowed case
  plus three fail-closed diagnostics; the KR-1 demo pins the keyed
  reconcile end to end.
- **clippy pedantic**: `cargo clippy --workspace --all-targets -- -D
  warnings` green, benches included; no new allowlist rows.
- **Benches from day one**: `cargo bench --workspace --no-run` stays a CI
  gate; the `runtime/list_diff_keyed` criterion bench and the budget
  workload now measure `plan_keyed` — the path the generated code runs —
  at 3.8 µs against the 25 µs ceiling (6.6× headroom, ledger note
  re-based with provenance).
- **CI/CD**: unchanged job set — the parity/admission/reproducibility/
  patch-spike/budgets/no-std jobs cover the milestone (the new fixture
  rides the existing parity gate; no new gate, no new error code).
- **git-cliff**: changelog continues on conventional commits (`v0.1.0-m5`).
- **Zero new external dependencies**: `plan_keyed`/`SeqOp`/`key_hash` are
  workspace code in `lig-runtime` (still zero-dep, `#![no_std]`,
  `#![forbid(unsafe_code)]` — the no_std proof re-proved it on both bare
  metal targets).

## Full gate run at delivery

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ |
| `cargo test --workspace` | ✅ 249 tests / 39 binaries |
| `cargo bench --workspace --no-run` | ✅ |
| `lig fixtures verify` | ✅ 19/19 fixtures, both legs |
| `lig fixtures admission` | ✅ 22/22 cases (NFR-5) |
| `lig fixtures rules-sync` | ✅ 11 rules 1:1 with fixtures |
| `lig reproducibility` | ✅ byte-identical (7 146 B counter unit, unchanged — keyed code lives in the keyed fixture's unit) |
| `lig incremental` | ✅ 6 passes green (D-9 + LIG-E020) |
| `lig errors-gen` | ✅ 15 codes + 15 embedded docs, regeneration is a no-op |
| `lig patch-spike` | ✅ two generations, **252 ms** total, < 1 s per save |
| `lig budgets` (release) | ✅ all 10 rows within budget — keyed row below |
| `lig no-std` | ✅ proof green (2 bare-metal targets + the generated unit) |
| `lig doctor` | ✅ all environment checks pass |

The M5 keyed budget row at delivery (release median, this runner):

| Workload | Measured | Ceiling | Headroom |
| --- | --- | --- | --- |
| `runtime_list_diff_keyed` | 3.78 µs | 25 µs | 6.6× |

## Notes and decisions

- **Why a planner instead of executing `keyed_diff`'s ops directly**: the
  diff's removes and moves address the old sequence; the first removal
  shifts every later index and the first move shifts every survivor past
  it. The permutation harness fails on `[1,2,3,4] → [2,4,3,1]` when a
  naive sequential application is attempted. `plan_keyed` simulates the
  placement on a key model and emits live-index steps, so the generated
  code stays a plain sequential `match` — the same emission discipline as
  the unkeyed reconcile (ADR-018 §4).
- **Why the generated keyed path re-applies every item's binds**: the LIS
  win is structural (no destroy/recreate on reorder — the expensive part
  per Vapor's own rationale); binds are the cheap part, and index-driven
  binds (`{i} {r}`) *must* refresh after moves to stay correct. The
  refresh loop is the unkeyed path's existing shape, unchanged.
- **Why the key fn normalizes rather than requiring u64 keys**: the
  admitted item alphabet is strings and numbers; `key_hash` (FNV-1a 64)
  keeps `lig-runtime` dependency-free (the workspace FNV in `lig-core`
  would add a dependency edge the §10 zero-dep contract forbids) and is
  deterministic across hosts, which the D-8 byte-pinning requires.
- **Why the frontends deny state-reading keys twice** (admission
  `LIG-E001` + codegen `Unsupported`): admission gives the author a code
  frame; codegen is the fail-closed backstop for IR that never passed the
  frontends (hand-built modules, future frontends).
- **The ledger note moved with the measurement**: the row's `note` now
  names `plan_keyed` and both medians (M4 diff 2.6 µs, M5 planner 3.8 µs)
  — provenance is part of the row, per the M4 discipline.

## Known gaps (deferred, deliberate)

- `cargo vet` (§14.2 supply-chain onboarding, called out in ADR-012):
  still deferred — the pin table remains the reviewed surface.
- Named slots and `<slot>` fallback content; `v-model`; multi-statement
  effect bodies: the IR models the default outlet only (v2 schema).
- A `lig patch` developer driver over real app dylibs (app-crate scaffold
  + dev channel): the engine surface (`PatchSession` / `apply_patch` /
  `hot_call` / `PatchManifest`) is what that work wires into; the M3 gate
  proves the protocol on the reference host.
- A `key` option on `.map` children (the IR side is this milestone's
  `ForIr.key`; the `.map` lowering does not accept it yet).
