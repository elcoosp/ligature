# M11 milestone report — the else leg

Date: 2026-11-09 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the last silent fail-open of the mount-time family)

The frontends have lowered `else` since M2 and the emitter even emitted the
else template tables — but the mounting arm mounted the `then` branch only
(the M10 report's flagged gap). The deep dive confirmed the full fail-open
shape and this milestone closes it:

| Exit criterion | Result |
| --- | --- |
| **The else leg mounts** | ✅ An If dynamic carrying an else leg emits a tracked-leg arm: a `leg_f<frag>_d<dyn>: u8` field records the mounted leg (0/1/2); the arm unmounts, disposes BOTH legs' per-mount instances (idempotent takes), and mounts the wanted leg. `show-conditional`'s dead `Gate_TPL_T0` renders for the first time; the `ternary-child` probe (an expression ternary lowering to an If with an else) carried the same dead template — fixed too. Then-only Ifs keep the byte-stable M7 emission: the counter unit is unchanged (7,385 B, digest `0x85d06a5c087b3589`). |
| **Else dynamics address honestly** | ✅ `branch_owner_of` maps `i.else_` to its If owner and the SHARED branch slot, so else binds/events/refs/StyleProps classify `Ctx::Branch` like their then twins (before: `Ctx::Direct` — a bind inside else content emitted an arm addressing `self.frag` with the ELSE fragment's hole index, a silent cross-fragment write). |
| **Per-mount identity on the else leg** | ✅ A component use on an else branch builds as `Option<Child>`: the else mount rebuilds the instance and inserts its root into the branch's component hole, the transition disposes it (idempotent take + teardown), and the prop-pass arm guards on the instance (before: an eager field built at build time — the M10 stale-eager-instance shape in a new location). |
| **Branch-time assembly on the else leg** | ✅ An outlet inside an else branch mounts its use-site fill (author side) or its own fallback (any branch) WHEN THE ELSE MOUNTS, through the composed `branch_mut` → `slot_fill_mut` walk — the ctx map already walked else branches (M10), only the mounting arm ignored them. The marking and assembly are per leg (guarded by the tracker), so the else leg's marking can never land on a mounted then branch. |
| **Leg-guarded post-flush captures** | ✅ Event attaches and ref captures for else-carrying Ifs guard on the tracker (`if s.leg_fX_dY == L { … }`) — `Fragment::hole()` falls back to the slab root out of bounds, so an unguarded capture would silently attach the wrong node. Then-only Ifs stay unguarded (byte-stable). |
| **Fail-closed boundaries unchanged** | ✅ Fills for child-side branch-internal outlets still deny through the unmatched-fill backstop (the registry milestone, ADR-024 §4); list-body component uses still deny (`LIG-E001`); the admission ladder is untouched (22 cases, 11 rules). |

## Fixtures

✅ One new behavior fixture, both legs (29 → 30):

| Fixture | Pins |
| --- | --- |
| `show-else` | Two root-level conditionals with else legs: `<Show when={on()} fallback={<i class="off">{tag}</i>}>` (a dynamic bind on BOTH legs) and `<Show when={on()} fallback={<Badge note="off" />}>` (a per-mount component on the else leg). One canonical IR: the If carries `then` + `else_`; the else fragment carries the bind / the Component with its prop binding. |

The KR-1 demo pins the runtime behavior on the mock host: build (then up,
else component absent) → flip off (the else legs mount, the else bind
initializes through the mount marking, the fresh Badge's prop initializes)
→ late write lands while the else is mounted → flip on (the else instance
disposes with its branch; the then leg remounts fresh and re-initializes).

## Engineering baselines held

- **TDD**: 293 tests across 39 test binaries (M10: 287/39). The M11 shapes
  landed red-first: five codegen contract tests (mount/swap, bind
  addressing, per-mount identity, author-side else outlets, child-side
  else fallbacks) failed at codegen before the implementation; the fixture
  and the demo went red→green with it.
- **clippy pedantic**: `cargo clippy --all-targets --all-features -- -D
  warnings -W clippy::pedantic` green, benches included. Zero inline
  `#[allow]`/`#![allow]` in the tree; the workspace lints table carries
  the documented allowlist (ADR-023 §5 discipline).
- **Benches from day one**: `cargo bench --workspace --no-run` stays a CI
  gate; M11 adds the `codegen/else` bench workload (module codegen over
  the show-else fixture — the tracked-leg arms, the per-leg assembly and
  marking emission) with a `codegen_else` ledger row (§14.2): release
  median 1 067 µs against a 6 000 µs ceiling (≈5.6× headroom). 13 ledger
  rows, all enforced in release.
- **Byte-stability**: the counter component digest is unchanged
  (`0x85d06a5c087b3589`, 7 385 B); then-only shapes emit token-identical
  units. `ternary_child` and `show_conditional` regen (their else legs now
  mount) — both fixtures' demos stay green.
- **Docs drift fixed en route**: the README version badge said `0.1.0-m9`
  at a 1.0.0 release; the fixtures README table said "(27)" and was
  missing the two M10 rows; `Cargo.lock` recorded workspace members at
  0.1.0 since the M10 bump; the internal `[workspace.dependencies]` pins
  (`version = "0.1.0"`) made `cargo metadata` FAIL at v1.0.0 — the tag
  itself did not resolve. The pin rows now carry path-only entries (the
  release flow publishes changelogs, not crates) and the lock catches up.
- **Zero new external dependencies**: the Cargo graph is untouched.
- **CI/CD**: unchanged job set — the new fixture rides the parity gate
  (30 fixtures, both legs), the new gen module rides compile-clean.

## Full gate run at delivery

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic` | ✅ (workspace table carries pedantic; ADR-023 §5 CLI-flag precedence note) |
| `cargo test --workspace` | ✅ 293 tests / 39 binaries |
| `cargo bench --workspace --no-run` | ✅ |
| `lig fixtures verify` | ✅ 30/30 fixtures, both legs |
| `lig fixtures admission` | ✅ 22/22 cases (NFR-5) |
| `lig fixtures rules-sync` | ✅ 11 rules 1:1 with fixtures |
| `lig reproducibility` | ✅ byte-identical (7 385 B counter unit — digest unchanged `0x85d06a5c087b3589`) |
| `lig incremental` | ✅ 6 passes green (D-9 + LIG-E020) |
| `lig errors-gen` | ✅ 15 codes + 15 embedded docs, regeneration is a no-op |
| `lig patch-spike` | ✅ two generations, < 1 s per save |
| `lig budgets` (release) | ✅ all 13 rows within budget — else 1 067 µs / 6 000 µs (5.6×), branch 1 047 µs / 6 000 µs, composed 939 µs / 6 000 µs, counter 574 µs / 3 000 µs, unit 7 385 B / 8 192 B |
| `lig no-std` | ✅ proof green (2 bare-metal targets + the generated unit) |
| `lig doctor` | ✅ all environment checks pass (view IR v5) |

## Notes and decisions

- **Why a leg tracker instead of runtime branch identity**: the runtime
  sees one mounted branch per slot — teardown, dispose, and host semantics
  are unchanged. Which LEG is mounted is a codegen-level fact (the leg's
  template/holes/branches live in the generated module); a `u8` field per
  else-carrying If dynamic is the smallest honest state, and then-only
  shapes stay tracker-free so every pre-existing generated unit is
  byte-identical.
- **Why `needs_assembly` includes the else leg**: a fresh else branch's
  binds must initialize through the mount marking (the M8 mechanism) — a
  plain arm would mount content whose binds never ran (the show-else demo
  pins the init through the marking on both flips).
- **Why the dispose lists run on every transition**: the leg swap tears
  down whatever was mounted; `Option::take` + teardown is idempotent, so
  running both legs' lists in one transition is correct without knowing
  which leg is outgoing (the tracker says, but the take makes it moot).
- **Why the else leg always marks but events/refs guard separately**:
  binds are reactions (mask-driven, cheap, correct per leg); dispatch
  records and ref captures are node-addressed — they need the tracker
  guard to avoid the `hole()` out-of-bounds fallback (slab root).
- **Why the registry stays next**: mounting a parent fill into a child's
  branch-mount arm needs the child to call parent code that evaluates
  parent-authored binds against parent state — a real protocol (ADR-024
  §4). With both branch legs honest, that design lands on solid ground.

## Known gaps (deferred, deliberate)

- Fills for child-side branch-internal outlets: fail-closed (the
  unmatched-fill backstop) — the cross-component registry is the next
  milestone's mechanism (ADR-024 §4).
- Per-item component/outlet instances in list bodies: fail-closed
  (`LIG-E001`, the keyed-component story).
- Remount-time re-attach of dispatch records and ref re-capture for
  DIRECT-rooted plain branches (the build-time captures are leg-correct;
  the mount marking initializes binds, not dispatch records) —
  pre-existing, deferred with the co-read marking refinement.
- `v-model` on non-input elements and `<slot :name>` forwarding:
  fail-closed today (M6 gap, unchanged).
- Template refs on the Vue leg: the Solid leg's `ref={local}` works end
  to end; the Vue template has no `ref` spelling yet.
- Component-instance disposal at app teardown rides the host's document
  teardown (pre-existing boundary).
- `cargo vet` (§14.2 supply-chain onboarding, ADR-012): still deferred.
- A `lig patch` developer driver over real app dylibs: the engine surface
  is what that work wires into.
