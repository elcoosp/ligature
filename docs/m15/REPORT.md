# M15 milestone report — per-item event dispatch (events in list bodies)

Date: 2026-11-09 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the keyed-interaction story)

The follow-up both ADR-0026 and ADR-0027 name for interaction: event
handlers inside list bodies. `LIG-E118`'s v1 denial stated its own premise
— "per-item dispatch needs keyed component instances" — and M13 delivered
exactly that. M15 delivers the event story on the same base:

| Exit criterion | Result |
| --- | --- |
| **Per-item event dispatch** | ✅ An event handler DIRECTLY on a For-body fragment emits an index-addressed fire fn (`fire_f{body}_d{di}(&mut self, h, index, ev)`) on the owning component: capture the payload, re-evaluate the For source, address the LIVE item at `index` (a stale index returns — no panic), substitute the item/index locals into the handler body, run it, and flush the owner — the handler's state writes cascade through the standard schedule. The handler's Effect reaction emits NO run arm (the flush scope has no item addressing — the M13 rule). |
| **The attach rides the insert arms** | ✅ The handler record attaches inside the plan's INSERT arms (keyed `SeqOp::Insert` + the unkeyed insert loop, direct and composed receivers) — once per fresh item, at the event's hole with the handler reaction id. The per-item replay never re-attaches; `list_remove` → `teardown` disposes the records with the item's slab (child-first LIFO); keyed `list_move` relocates the record with its item — no rebuild, no re-attach. The demo pins the full cycle on the mock host. |
| **Substitution threads every level** | ✅ The item-expression renderer becomes an explicit context (item/index scope + state-read discipline: `self.` reads vs the replay's `s_<field>` params) threaded through the whole expression generator — handler bodies substitute at every nesting level and clone the item per reference; the replay keeps its byte-stable rendering. Event-field reads and setter calls inside item binds stay codegen-denied with precise reasons. |
| **Fail-closed boundaries, redrawn one step** | ✅ The flat v1 check (the body's direct dynamics only — nested events slipped through to the codegen backstop) becomes a recursive item-subtree walk mirroring the codegen's composition rule: events in list-item BRANCHES, in NESTED list bodies, and on COMPOSED (branch/slot-owned) lists keep `LIG-E118`, reported at the event's own span with one precise reason. Direct item-body events admit on both legs through the shared admission crate; the composed codegen backstop stays as the second line. |
| **One canonical IR, both legs** | ✅ The `for-event` fixture pins the parity: a keyed `<For>`/`v-for` whose item body is a `<button>` with an item-bound `onClick`/`@click` writing parent state byte-compares against one committed `expected.ir.json` (modulo the parity-ignored fields) — 33 → 34 fixtures, both legs, `lig fixtures verify` green. |

## Fixtures

✅ One new behavior fixture, both legs (33 → 34):

| Fixture | Pins |
| --- | --- |
| `for-event` | A keyed list whose item body carries a button with an item-bound handler writing parent state: one Attach commit per fresh item (distinct nodes, one handler id), index-addressed dispatch substitutes the live item (alpha/beta/gamma), keyed rotations move the handler records without re-attaching, removals dispose them with the item's slab, stale indices are no-ops, and fresh keys attach exactly once. The demo pins the full identity cycle on the mock host. |

The admission ladder redraws one row: `e118-for-handler/allow` now pins the
admitted direct item-body handler; `e118-for-handler/deny` pins the still-
denied branch-in-item event (the rules-sync gate stays 1:1, 22 cases, 11
rules).

## Engineering baselines held

- **TDD**: 321 tests across 39 binaries (M14: 316/39). The M15 shapes
  landed red-first: the admission tests (direct-item-body admission, the
  branch-in-item and nested-list denials), the codegen contract test (the
  fire-fn shape, the item-substituted handler body, the insert-arm attach),
  and the demo identity cycle all failed before the implementation existed.
- **clippy pedantic**: `cargo clippy --workspace --all-targets
  --all-features -- -D warnings` green, benches included, zero warnings.
  Zero new suppressions — the tree still carries zero `#[allow]`/
  `#![allow]` attributes outside the workspace `[lints.clippy]` rationale.
- **Benches from day one**: `cargo bench --workspace --no-run` stays a CI
  gate; M15 adds the `codegen/for-event` bench workload (module codegen
  over the for-event fixture — the fire fns, the item-substituted handler
  bodies, the attach arms) with a `codegen_for_event` ledger row (§14.2):
  release median 680 µs against a 3 500 µs ceiling (≈5.1× headroom).
  17 ledger rows, all enforced in release.
- **Byte-stability**: the counter component digest is unchanged
  (`0x85d06a5c087b3589`); the unit is byte-identical (7 385 B) — the
  milestone's generated-code growth is confined to units using the surface.
  `lig reproducibility` stays green; `lig fixtures gen` regenerated all
  artifacts with zero diffs beyond the new `for_event` module.
- **Zero new external dependencies**: the Cargo graph is untouched. Zero
  runtime changes — `Fragment::attach`, the teardown recursion, and the
  flush loop are pre-existing machinery. Zero IR schema changes (v5 stands).

## Full gate run at delivery

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ (workspace table carries pedantic; zero warnings) |
| `cargo test --workspace` | ✅ 321 tests / 39 binaries |
| `cargo test --workspace --all-features` | ✅ |
| `cargo bench --workspace --no-run` | ✅ |
| `lig fixtures verify` | ✅ 34/34 fixtures, both legs |
| `lig fixtures admission` | ✅ 22/22 cases (NFR-5) |
| `lig fixtures rules-sync` | ✅ 11 rules 1:1 with fixtures |
| `lig reproducibility` | ✅ byte-identical (7 385 B counter unit — digest unchanged `0x85d06a5c087b3589`) |
| `lig budgets` (release) | ✅ all 17 rows within budget — for-event 676 µs / 3 500 µs (5.2×), list-slot 577 µs / 3 000 µs (5.2×), list-component 901 µs / 4 800 µs (5.3×), codegen 576 µs / 3 000 µs, unit 7 385 B / 8 192 B |
| `lig doctor` | ✅ all environment checks pass (view IR v5) |

## Notes and decisions

- **Why this milestone**: the per-item base has been proven twice (M13
  instances, M14 slot assembly); events were the last interactivity
  primitive denied in list bodies, and `LIG-E118`'s explanation named the
  exact mechanism M13 shipped. Lists of interactive rows are the canonical
  shape the composition matrix exists for — the row abstraction is now
  complete end to end (binds, components, outlets, events).
- **Why index-addressed dispatch, not per-item reaction flush**: the
  flush scope has no item addressing (one reaction, N live items — the
  M14 bind reasoning). The per-item fns (replay, children, handlers) are
  the one addressing discipline the codebase already trusts; the fire fn
  joins it instead of inventing an item-scoped flush. Stale indices
  resolve through the live source — a removed item can never ghost-fire.
- **Why the attach lives in the insert arms, not the replay**: the replay
  runs on every reconcile; `Fragment::attach` records + commits, so a
  replay-side attach would multiply host commits per reconcile. The insert
  arms run exactly once per item lifetime — the same once-per-mount
  reasoning as M10's branch attaches.
- **Why the admission walk mirrors the codegen ctx rule**: admission is
  the fail-closed FIRST line; it must predict exactly what codegen can
  address (Direct-ctx reconciles). Mirroring the codegen's owner rule
  (an owner not referenced as a branch/list/slot body ⇒ direct) keeps the
  two lines consistent instead of relying on the codegen backstop to catch
  shapes admission should have caught — the v1 flat check's hole.
- **ADR numbering**: 0028 continues the sequence (0026 = M13, 0027 = M14).

## Known gaps (deferred, deliberate)

- Events inside list-item BRANCHES (per-item × per-mount composition) and
  inside NESTED list bodies (per-item × per-item): the sticky denial
  stands, now caught at admission (the v1 flat check missed them).
- Events on COMPOSED list bodies (branch- or slot-owned — the composed
  dispatch addressing through a receiver fragment): the admission denial
  and the codegen backstop both stand.
- Per-item refs (the store's `Option` slots are the base) and
  state-reading binds inside per-item slot content (the flush fan-out over
  live items): both keep their precise denials.
- Use-site fills for child-side non-root outlets and cross-component
  branch-fill coordination: the M9/M10 backstops stand.
- `cargo vet` (§14.2 supply-chain onboarding): still deferred.
