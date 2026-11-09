# M14 milestone report — per-item slot assembly (slot outlets in list bodies)

Date: 2026-11-09 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the keyed-outlet story)

The follow-up every milestone report since M10 named: slot outlets inside
list bodies. M10's deep dive showed the pre-M10 lowering carried a
never-mounted outlet hole in the item template; M10 closed the branch case
(child-side outlets mount their own fallback at branch mount) and deferred
the list case. M13 delivered the per-item base machinery. M14 delivers the
outlet story on it:

| Exit criterion | Result |
| --- | --- |
| **Per-item slot assembly** | ✅ A slot outlet DIRECTLY on a For-body fragment mounts its own fallback into EVERY live item's outlet hole — the per-item replay (`apply_item`) calls `f.mount_slot(h, di, SlotOrigin::Fallback, …)` on the item fragment, idempotently (an already-mounted item skips), then applies the fallback's scalar binds to the mounted content with the item/index substituted (`f.slot_fallback_mut(di)`, the M13 substitution discipline). Zero runtime changes: `mount_slot` is pre-existing machinery, keyed moves relocate the item's `Box<Fragment>` (the mounted content moves with it — no rebuild), and `list_remove` → `teardown` already recurses into slots (child-first LIFO). |
| **The per-item replay drives the fallback binds** | ✅ The fallback's Text/Class/Attr/StyleProp binds emit into the same `apply_item` block; item/index locals substitute (`gen_item_expr`), and the binds re-run on every replay — unkeyed re-indexing refreshes them exactly like the body's own binds. State-reading binds deny with a precise reason (the flush fan-out over live items is the follow-up). |
| **Reaction addressing stays honest** | ✅ The fallback fragments' reactions leave the general run arms (one reaction, N live items — the flush scope has no item addressing) and stay schedule-inert (locals read no state fields), so the const schedule and the flush loop are untouched. |
| **Fail-closed boundaries, redrawn one step** | ✅ Outlets inside list-item BRANCHES stay denied (the sticky `in_for` discipline, both legs, one precise reason — per-item × per-mount composition is the follow-up). Outlets on COMPOSED list bodies keep the content-side denial (composed fill/fallback resolution is the follow-up). Use-site fills for list-body outlets stay fail-closed through the M9 unmatched-fill backstop (items do not exist at the parent's build). Per-item slot content admits scalar binds only — events/refs/structural content keep the codegen backstop. |
| **One canonical IR, both legs** | ✅ The `list-slot` fixture pins the parity: a keyed `<For>`/`v-for` whose item body renders `<Slot name="row"><i class="row">{t}</i></Slot>` byte-compares against one committed `expected.ir.json` (modulo the parity-ignored fields) — 32 → 33 fixtures, both legs, `lig fixtures verify` green. |

## Fixtures

✅ One new behavior fixture, both legs (32 → 33):

| Fixture | Pins |
| --- | --- |
| `list-slot` | A keyed list whose item body is a named outlet with an item-bound fallback: each live item mounts the fallback into its outlet hole with the item text bound by the replay, keyed rotations move the SAME mounted content without rebuilds, removals dispose it with the item, and fresh keys build fresh content. The demo pins the full identity cycle on the mock host. |

## Engineering baselines held

- **TDD**: 316 tests across 39 binaries (M13: 310/39). The M14 shapes
  landed red-first: the frontend contract tests (direct item-body outlet
  admission on both legs, the branch-in-item denial), the codegen contract
  tests (the per-item mount + bind emission, the state-read denial, the
  fill backstop, the scalar-content backstop), and the demo identity
  assertions failed at lowering/codegen/runtime before the implementation
  existed.
- **clippy pedantic**: `cargo clippy --all-targets --all-features -- -D
  warnings` green, benches included. Zero new suppressions — the tree
  still carries zero `#[allow]`/`#![allow]` attributes.
- **Benches from day one**: `cargo bench --workspace --no-run` stays a CI
  gate; M14 adds the `codegen/list-slot` bench workload (module codegen
  over the list-slot fixture — the per-item mount block, the item-bound
  bind emission, the reaction filtering) with a `codegen_list_slot`
  ledger row (§14.2): release median 582 µs against a 3 000 µs ceiling
  (≈5.2× headroom). 16 ledger rows, all enforced in release.
- **Byte-stability**: the counter component digest is unchanged
  (`0x85d06a5c087b3589`); the unit is byte-identical (7 385 B) — the
  milestone's generated-code growth is confined to units using the
  surface. `lig reproducibility` stays green.
- **Zero new external dependencies**: the Cargo graph is untouched. Zero
  runtime changes — `mount_slot`, the item slot table, and the teardown
  recursion are pre-existing machinery. Zero IR schema changes (v5 stands).
- **CI/CD**: unchanged job set — the new fixture rides the parity gate
  (33 fixtures, both legs), the new gen module rides compile-clean; the
  admission ladder is unchanged (22 cases, 11 rules — the M14 denials are
  `LIG-E001` reasons, not new codes).

## Full gate run at delivery

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ (workspace table carries pedantic) |
| `cargo test --workspace` | ✅ 316 tests / 39 binaries |
| `cargo bench --workspace --no-run` | ✅ |
| `lig fixtures verify` | ✅ 33/33 fixtures, both legs |
| `lig fixtures admission` | ✅ 22/22 cases (NFR-5) |
| `lig fixtures rules-sync` | ✅ 11 rules 1:1 with fixtures |
| `lig reproducibility` | ✅ byte-identical (7 385 B counter unit — digest unchanged `0x85d06a5c087b3589`) |
| `lig budgets` (release) | ✅ all 16 rows within budget — list-slot 582 µs / 3 000 µs (5.2×), list-component 901 µs / 4 800 µs (5.3×), ref 238 µs / 1 200 µs (5.0×), branch 1 126 µs / 6 000 µs (5.3×), codegen 576 µs / 3 000 µs, unit 7 385 B / 8 192 B |
| `lig doctor` | ✅ all environment checks pass (view IR v5) |

## Notes and decisions

- **Why this milestone**: M10's known-gaps list named the outlet
  mechanism gap — "per-item slot assembly stays the follow-up" — and M13
  built the per-item base (the replay pass, the per-item addressing
  discipline). Lists were the last top-level composition site that denied
  outlets, so the matrix is now complete: an outlet compiles on a root
  (M6/M7), in content and branches (M8/M9/M10/M11), and in list bodies
  (M14) — each with the assembly semantics its container demands
  (build-time, mount-time, per-item).
- **Why mount in the replay, not the reconcile mirrors**: M13's component
  stores hang on the OWNER (generated structs cannot live in the runtime
  slab). Mounted slot content is the opposite case — it IS runtime state
  (`Fragment`s), so it lives exactly where it belongs: the item's own slot
  table. The replay's `apply_item` is the one place per-item work already
  runs for both plans (keyed and unkeyed, direct and composed), so the
  mount rides it with zero reconcile changes and zero new owner fields.
- **Why idempotent mount + re-apply binds**: `mount_slot` returns false on
  a mounted outlet, so the replay can run unconditionally; the binds
  re-apply per replay so unkeyed re-indexing (items changing position ⇒
  value) refreshes them with the same discipline as the body's binds — one
  rule for both plans.
- **Why fills stay backstopped**: the mount arm is the child's (its
  reconcile), the fill is parent-authored (its init walk) — items do not
  exist at the parent's build, so a fill for a list-body outlet would
  never mount. That cross-component coordination is the same follow-up
  M10 named for child-side branch outlets; the M9 unmatched-fill backstop
  already fails closed on it, and the frontends keep the composed-content
  denial.
- **ADR numbering**: 0027 continues the sequence (0025 = M12, 0026 = M13).

## Known gaps (deferred, deliberate)

- Slot outlets inside list-body BRANCHES (per-item × per-mount
  composition): the sticky denial stands; the walk's `item_branch` state
  documents the shape.
- Outlets on COMPOSED list bodies (fill/fallback resolution through a
  `Ctx::Fill` chain at reconcile time): the content-side denial stands.
- Use-site fills for child-side non-root outlets (list bodies and
  branches): the M9 backstop and the M10 codegen denial stand — the
  cross-component fill coordination is the named follow-up.
- State-reading binds inside per-item slot content (the flush fan-out
  over live items), per-item events (`LIG-E118`), and per-item refs: all
  keep their precise denials.
- `cargo vet` (§14.2 supply-chain onboarding): still deferred.
