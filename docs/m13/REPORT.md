# M13 milestone report — per-item instance identity (components in list bodies)

Date: 2026-11-09 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the keyed-component story)

The follow-up every milestone report since M9 named: component uses inside
list bodies. M10's deep dive showed the pre-M10 lowering built ONE eager
instance shared by every item; M10 closed it with a precise denial and
deferred the mechanism. M13 delivers the mechanism:

| Exit criterion | Result |
| --- | --- |
| **Per-item instance identity** | ✅ A component use on a For-body fragment builds as a `Vec<Option<Child>>` store on the owner; the generated reconcile mirrors the keyed plan onto it — inserts build + attach the child's root into the item's component hole (one `Host::insert` per item mount, the M10 attach contract), removes take + tear the instance's slab down BEFORE the item unmounts (child-first LIFO), and keyed moves reposition the SAME instance (`remove(from)` + `insert(to)` — the exact `Vec` semantics of `Fragment::list_move`). The demo pins identity across a keyed rotation by node-handle equality: a move rebuilds nothing. |
| **The per-item replay drives props** | ✅ The replay loop calls a generated per-use children fn per live item — prop bindings with item/index locals substituted, parent-state reads pre-cloned (the borrow-split discipline; `collect_body_reads` walks component prop values too) — then flushes the child. Uses with no dynamic props emit no replay fn. |
| **Branch-unmount disposal** | ✅ A branch whose subtree owns list-body component stores drains them in the unmount arm (M10's dispose list, `drain(..)` is idempotent like the `Option::take` list); a remount rebuilds through the mount marking + the list's first reconcile. |
| **The replay covers every live item** | ✅ One rule for both plans: the M5 keyed replay already covered all items; the unkeyed Index diff now does too (the survivor-only `.take(common)` shape left appended items' binds uninitialized until the next reconcile — the per-item component props made it unambiguously wrong, so it is closed here; the unkeyed fixtures' regenerated artifacts carry the extended replay). |
| **Fail-closed boundaries, redrawn one step** | ✅ Component uses inside list-body BRANCHES stay denied (the sticky in-for discipline, both legs, one precise reason). Slot outlets inside list bodies keep the M10 denial (per-item slot assembly stays the follow-up). Events in list bodies keep `LIG-E118`. The codegen backstops remain as the honest second line. |
| **One canonical IR, both legs** | ✅ The `list-component` fixture pins the parity: a keyed `<For>`/`v-for` whose item body renders `<Badge count={t} />` byte-compares against one committed `expected.ir.json` (modulo the parity-ignored fields) — 31 → 32 fixtures, both legs, `lig fixtures verify` green. |

## Fixtures

✅ One new behavior fixture, both legs (31 → 32):

| Fixture | Pins |
| --- | --- |
| `list-component` | A keyed list of component uses: the store starts empty, the initial flush mounts one `Badge` per item with its prop bound by the replay, keyed rotations move instances without rebuilds, removals dispose them, and fresh keys build fresh instances. The demo pins the full identity cycle on the mock host. |

## Engineering baselines held

- **TDD**: 310 tests across 39 binaries (M12: 297/39). The M13 shapes
  landed red-first: the frontend contract tests (own-tree and content
  admission on both legs, the outlet-only denials, the branch-in-item
  boundary), the codegen contract test (the store, the plan-op mirroring,
  the per-item replay arms), and the demo identity assertions failed at
  lowering/codegen/runtime before the implementation existed.
- **clippy pedantic**: `cargo clippy --all-targets --all-features -- -D
  warnings` green, benches included. Zero new suppressions — the tree
  still carries zero `#[allow]`/`#![allow]` attributes.
- **Benches from day one**: `cargo bench --workspace --no-run` stays a CI
  gate; M13 adds the `codegen/list-component` bench workload (module
  codegen over the list-component fixture — the store field, the plan-op
  mirroring, the children replay) with a `codegen_list_component` ledger
  row (§14.2): release median 901 µs against a 4 800 µs ceiling (≈5.3×
  headroom). 15 ledger rows, all enforced in release.
- **Byte-stability**: the counter component digest is unchanged
  (`0x85d06a5c087b3589`); the unit is byte-identical (7 385 B) — the
  milestone's generated-code growth is confined to units using the
  surface. `lig reproducibility` stays green.
- **Zero new external dependencies**: the Cargo graph is untouched. Zero
  runtime changes — the `Fragment` list table, `plan_keyed`, and the
  attach/teardown primitives are pre-existing machinery. Zero IR schema
  changes (v5 stands).
- **CI/CD**: unchanged job set — the new fixture rides the parity gate
  (32 fixtures, both legs), the new gen module rides compile-clean; the
  admission ladder is unchanged (22 cases, 11 rules — the M13 denials are
  `LIG-E001` reasons, not new codes).

## Full gate run at delivery

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ (workspace table carries pedantic) |
| `cargo test --workspace` | ✅ 310 tests / 39 binaries |
| `cargo bench --workspace --no-run` | ✅ |
| `lig fixtures verify` | ✅ 32/32 fixtures, both legs |
| `lig fixtures admission` | ✅ 22/22 cases (NFR-5) |
| `lig fixtures rules-sync` | ✅ 11 rules 1:1 with fixtures |
| `lig reproducibility` | ✅ byte-identical (7 385 B counter unit — digest unchanged `0x85d06a5c087b3589`) |
| `lig budgets` (release) | ✅ all 15 rows within budget — list-component 901 µs / 4 800 µs (5.3×), ref 238 µs / 1 200 µs (5.0×), branch 1 126 µs / 6 000 µs (5.3×), codegen 576 µs / 3 000 µs, unit 7 385 B / 8 192 B |
| `lig doctor` | ✅ all environment checks pass (view IR v5) |

## Notes and decisions

- **Why this milestone**: M10's known-gaps list named the mechanism gaps —
  the keyed-component story is the one every report since M9 defers to
  ("per-item instance identity is the keyed-component story"). Lists are
  the last top-level composition site that denied component uses, so the
  matrix is now complete: a component use compiles in content (M9), in
  branches (M10), in else legs (M11), and in list bodies (M13) — each with
  the identity semantics its container demands (eager, per-mount,
  per-item).
- **Why a store on the owner, not instances inside the item fragment**: the
  runtime `Fragment` is a node slab — it cannot own generated child structs
  (the runtime knows nothing about generated types). The M10 per-mount
  `Option<Child>` field is the proven owner-side shape; a `Vec<Option<Child>>`
  mirrored against the plan ops is its per-item generalization, with zero
  runtime changes.
- **Why remove-before-unmount**: the child's root hangs inside the item's
  component hole; tearing the child down first keeps the LIFO disposal
  honest (the item's own teardown then removes the (already empty) hole),
  and a `Vec::remove` at the live index keeps the store aligned with the
  plan's sequential addressing.
- **Why the sticky branch denial stays**: a component on a branch INSIDE a
  list body composes per-mount identity with per-item identity — the
  instance would need to live on a per-item per-branch shape. That is the
  next composition step, not this one; the walk state (`in_for`,
  `item_branch`) keeps the boundary precise on both legs.
- **ADR numbering**: 0026 continues the sequence (0024 = M11, 0025 = M12).

## Known gaps (deferred, deliberate)

- Slot outlets in list bodies (per-item fill/fallback assembly): the M10
  denial stands; the per-item instance machinery is the base.
- Component uses inside list-body branches: the sticky denial stands (the
  walk's `item_branch` state documents the shape).
- Per-item events (`LIG-E118`) and per-item refs: both sit on the store's
  `Option` slots; neither has a committed contract yet.
- Cross-component branch-mount-time fill coordination: still fail-closed
  (the M10 unmatched-fill backstop) — unchanged.
- `cargo vet` (§14.2 supply-chain onboarding): still deferred.
