# M18 milestone report — the composed-content owner-direct lift (per-item events + slot assembly on composed content list bodies)

Date: 2026-11-09 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the content leg of the composed contract)

M17's documented contract — ADR-0030, the LIG-E118 catalog entry, the
`composed_events` walkthrough — names composed list bodies as "branch- or
slot-owned", but this tree admitted only the branch half: a `For` inside a
fill or a fallback still denied its own direct handler (`LIG-E001`) and its
own direct outlet ("composed fill/fallback resolution is the follow-up").
The docs were drift on this tree, discoverable by one `lig check` probe.
M18 makes the code match the documented contract and redraws the boundary
one step:

| Exit criterion | Result |
| --- | --- |
| **Owner-direct events on composed content bodies** | ✅ An event handler DIRECTLY on a fill- or fallback-owned `For`/`v-for` body lowers and admits — the composed reconcile's receiver-parameterized insert arms attach per fresh item (M17's machinery, the content leg), dispatch goes through the same source-addressed per-item fire fn, keyed moves carry the record without re-attaching, removals dispose it with the item's slab, and stale indices are no-ops. Zero codegen/runtime/admission changes for the event lift itself. |
| **Owner-direct outlets on composed content bodies** | ✅ A slot outlet DIRECTLY on a composed content list body lowers: every live item mounts the outlet's own fallback into its hole through the SHARED per-item replay (`apply_item_f*` → `emit_item_slot_arm` — receiver-neutral by construction), the fallback's item-bound scalar binds re-apply per replay, and the mounted content moves with its item. Zero codegen/runtime/admission changes — the codegen pin test passed on the same commit that lifted the frontend walk. |
| **The per-mount fill assembly (the M18 fixture's catch)** | ✅ Compile-clean REFUSED the fixture's generated unit: a fill authored at a BRANCH-mounted component use emitted `s.<field>.frag.mount_slot(…)` in the build body, where the per-mount instance is `None` until the branch mounts — a latent M10 fail-open (fills INTO branch-mounted instances were never exercised). Fixed two-sided: the build-phase slot-assembly loop skips `Ctx::Branch` uses, and the mounting arm mounts the use-site fills into the FRESH instance after the rebuild + root insert, attaches the content's events per mount (fill records on the parent's slab, fallback records on the child's — the M7 ownership discipline), init-flushes fallback binds on the instance, and widens the mount mask by the fill content's non-handler reactions so a fill-carried list's reconcile re-runs into the fresh instance. `Ctx::Fill` grows the `per_mount` flag; the three ctx emitters guard their `base = Direct` unwraps on it. Direct-fill emission is byte-stable — every pre-existing unit regenerates unchanged. |
| **One canonical IR, both legs** | ✅ The `for-composed-content` fixture pins the parity: a fill carried into a branch-mounted `Cell`, whose `For` body carries the handler AND the nested outlet with an item-bound fallback, byte-compares against one committed `expected.ir.json` — 35 → 36 fixtures, both legs, `lig fixtures verify` green. |
| **Fail-closed boundaries, redrawn one step** | ✅ Events beyond the composed content body (item branches, nested lists) keep the `LIG-E118` discipline; outlets beyond the body keep the branch-in-item denial; fills for list-body outlets keep the M9 unmatched-fill backstop; per-item slot content still admits scalar binds only; state-reading fallback binds keep the codegen backstop. The e118 allow fixture redraws to the composed-content shape (the new boundary), the deny case stays branch-in-item. |

## Fixtures

✅ One new behavior fixture, both legs (35 → 36):

| Fixture | Pins |
| --- | --- |
| `for-composed-content` | A fill carried into a branch-mounted child holds a keyed list whose body carries BOTH the per-item handler and the per-item outlet with an item-bound fallback: the fill mounts into the fresh instance at branch mount, one attach + one fallback mount per live item, keyed rotations move the same slabs without rebuilds or re-attaches, removals dispose with the item, and the branch cycle rebuilds everything per mount. The demo pins the full identity cycle on the mock host. |

## Engineering baselines held

- **TDD**: 333 tests across 39 binaries (M17: 327/39). The M18 shapes
  landed red-first: the composed-content admit pair (event + outlet on a
  content list body) denied through the CLI and the frontend tests before
  the walk lift existed, the codegen pin test failed on the composed
  reconcile emission, the typed-`CodegenFailure` and the extension-denial
  tests failed on the pre-M18 seams — and the fixture's generated unit
  REFUSED TO COMPILE, exposing the per-mount fill assembly gap the same
  day it shipped (the compile-clean gate failing loudly is the fail-open
  turned fail-closed).
- **clippy pedantic**: `cargo clippy --workspace --all-targets
  --all-features -- -D warnings` green — zero warnings, benches, tests,
  and examples included, the `bench-swc` feature path compiled too.
  Pedantic rides the `[lints.clippy]` table (`pedantic = warn`,
  `priority = -1`) and CI's `-D warnings` escalates it — passing `-W
  pedantic` after `--` is still wrong on rustc 1.99.1 (the bare group
  name is rejected as `renamed_and_removed_lints`; re-verified this
  milestone). Zero new suppressions: the tree still carries exactly the
  two pre-existing, rationale-commented `#![expect(unsafe_code)]`
  attributes (lig-patch's single-unsafe-module policy §15, ADR-013; the
  dummy-host spike binary) — both self-auditing.
- **Benches from day one**: `cargo test --workspace --benches` green
  (7 bench targets on the stable criterion suite; no nightly benches
  anywhere in the tree). M18 adds the `codegen/for-composed-content`
  workload with the `codegen_for_composed_content` ledger row (§14.2):
  release median 1 251 µs against a 6 500 µs ceiling (≈5.2× headroom,
  re-measured 1 152–1 251 across three runs). 20 ledger rows, all
  enforced in release.
- **Byte-stability**: every pre-existing generated unit is unchanged —
  `lig reproducibility` diffs the committed counter unit (7 385 B,
  digest `0x85d06a5c087b3589`) against a fresh generation, byte-identical;
  the `per_mount` ctx guards and the mounting-arm assembly are scoped to
  units that actually carry fills into branch-mounted instances.
- **Zero new external dependencies**: the Cargo graph is untouched. The
  admission ladder is unchanged (22 cases, 11 rules — the M18 denials are
  `LIG-E001`/`LIG-E118` reasons, not new codes). Zero IR schema changes
  (v5 stands).
- **CI/CD**: unchanged job set — the new fixture rides the parity gate
  (36 fixtures, both legs), the new gen module rides compile-clean; the
  budget ledger is 20 rows.

## Full gate run at delivery

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | ✅ (workspace table carries pedantic) |
| `cargo test --workspace` | ✅ 333 tests / 39 binaries |
| `cargo test --workspace --all-features` | ✅ 333 / 0 / 39 |
| `cargo test --workspace --benches` | ✅ 233 / 0, 7 bench targets (stable) |
| `lig fixtures verify` | ✅ 36/36 fixtures, both legs |
| `lig fixtures admission` | ✅ 22/22 cases (NFR-5) |
| `lig fixtures rules-sync` | ✅ 11 rules 1:1 with fixtures |
| `lig reproducibility` | ✅ byte-identical (7 385 B counter unit — digest unchanged `0x85d06a5c087b3589`) |
| `lig incremental` | ✅ digest-gated re-emit green (6 passes) |
| `lig patch-spike` | ✅ two generations, 264 ms total (PASS) |
| `lig budgets` (release) | ✅ all 20 rows within budget — for-composed-content 1 251 µs / 6 500 µs (5.2×), for-event-composed 816 µs / 4 500 µs (5.5×), unit 7 385 B / 8 192 B |
| `lig doctor` | ✅ all environment checks pass (view IR v5) |

## Notes and decisions

- **Why this milestone**: M18 completes a contract the tree already
  documented three times (ADR-0030, the E118 catalog entry, the example
  header) but only half-delivered — the docs were drift, and fulfilling a
  documented contract is the honest redraw. The matrix after M18: a list
  body's OWN surface (events + outlets) compiles on a root (M15), in a
  branch (M17), and in content (M18); slots compose on the same three
  sites (M14/M18); what remains is composition UNDER the body
  (branch-in-item, nested lists — the foundation follow-up) and
  cross-component fills for list-body outlets (the M9 backstop stands).
- **Why the walk carries an un-reset subtree bit**: the pre-M18 walk reset
  `in_list_item` at branch boundaries (the sticky-discipline hole that
  admission's E118 walk and the own-tree contract cover). Computing
  `owner_direct` from the reset flag would have WIDENED the nested-list
  boundary (a For inside a branch inside an item would have gained the
  owner-direct surface). The un-reset `in_item_subtree` bit keeps nested
  list bodies fail-closed while branches keep their pre-M18 semantics.
- **Why the per-mount fill assembly rides the mounting arm, not the build
  body**: M10's identity cycle is the one place the instance exists — the
  build body sees `None`. The fill is parent-authored content, so the
  mount, the event records (parent's slab for fills, the child's for
  fallbacks), and the mask widening all ride the rebuild list: one place,
  per mount, idempotent with the cycle.
- **Micro-debt, both items**: the session's LAST bare-`String` seam —
  `compute_generated`/`CompiledUnit::codegen_error` — is now a typed
  miette `Diagnostic` (`ligature::codegen`, help-text per backstop
  family, a `tracing::warn!` on the failure path), mirroring the M16 emit
  seam; and `lower_source` no longer silently lowers unknown extensions
  as Solid — one precise `LIG-E001` names `.tsx`/`.vue`.
- **ADR numbering**: 0031 continues the sequence (0030 = M17).

## Known gaps (deferred, deliberate)

- Events inside list-item branches and inside nested list bodies
  (per-item × per-mount and per-item × per-item composition): the
  foundation follow-up — both keep their precise denials on the composed
  content leg too.
- Use-site fills for child-side list-body outlets (items do not exist at
  the parent's build): the M9 backstop stands.
- Ref captures inside fills carried into branch-mounted instances: the
  `content_ref_inits` build-body emission assumes a plain instance field
  — the same M10-family shape the fill assembly fixed, fail-loud (the
  generated unit refuses to compile); the ref-composed guard is the
  named follow-up.
- `cargo vet` (§14.2 supply-chain onboarding): still deferred.
