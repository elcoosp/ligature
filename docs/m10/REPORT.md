# M10 milestone report — per-mount component identity and the branch-time slot assembly

Date: 2026-11-08 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (mount-time completeness)

M9 left three fail-closed denials and — the deep dive's finding — three
SILENT fail-opens in the same mount-time family. M10 closes all six:

| Exit criterion | Result |
| --- | --- |
| **The universal attach contract** | ✅ A DIRECT component use's root now enters its owner's tree at the use hole in the build body (`h.insert(s.frag.hole(di), s.<field>.frag.roots()[0], 0)`, before slot assembly) — the deep dive's commit-stream probe showed the child's DOM never attached since M2 (Create commits, no positioning insert; the demo tests never asserted the tree). The commit probe now shows the section under the App marker; props still land through the initial flush. The M9 content insert and the M10 branch insert complete the contract: one `Host::insert` per mount, every ownership shape. Zero runtime changes. |
| **Per-mount instance identity** | ✅ A component use on an If-branch fragment (content-rooted — the M8/M9 named gap — or direct-rooted, author or child side) builds as an `Option<Child>` field: the mounting arm rebuilds the instance and inserts its root into the branch's component hole, the unmount arm takes the field and tears the slab down (`inst.frag.teardown(h)`), and the prop-pass arm guards on the instance. The mount marking covers the branch subtree's reactions (the prop-pass reaction is the branch's), so a signal written while the branch was absent lands on the NEXT mount — the demo pins build → dispose → late write → remount with the fresh instance carrying the new value. |
| **Branch-time slot assembly** | ✅ An outlet on a branch fragment mounts its use-site fill (`SlotOrigin::Fill`) or its own fallback (`SlotOrigin::Fallback`) WHEN THE BRANCH MOUNTS, through the composed accessor chain (`slot_fill_mut` → `branch_mut` → `slot_fill_mut`/`slot_fallback_mut`), then the M9 recursive walk recurses into the freshly mounted content for its outlets and component holes. The mount mask widens by the branch-mounted fills' reactions (author-side fills are not under the branch subtree), so the fresh content's binds initialize on the next flush pass. `build_content_ctx` grows the branch leg (`map_branch_outlets`): fill contexts compose `Fill { base: Branch { .. } }`, and the unmatched-fill backstop covers the new matches. |
| **Child-side branch outlets** | ✅ The deep dive's probe 1c: an outlet inside the child's own `<Show>` compiled and rendered an empty hole forever — its fallback never mounted (the M6-era fail-open in a new location). The child's mounting arm now mounts the outlet's own fallback (child statics, child-local — no cross-component coordination). FILLS for child-side branch outlets stay codegen-denied with the precise reason: the mount arm is the child's, the fill is parent-authored — that coordination is the follow-up milestone's mechanism. |
| **The Vue structural-directive dispatch** | ✅ Probe 6: `<Badge v-if="on">` lowered as an unconditional component use — the directive silently dropped (the tag dispatch ran before the directive check; the same fail-open shape M9 closed for content roots). The dispatch order is fixed (For → If → slot → component → element) and `fragment_from_elem` dispatches component/slot BRANCH roots (the generalization of `fragment_from_content_root`), so `<Badge v-if>`, `<slot v-for>`, and `v-else*` companions lower onto the Solid leg's If/For shapes. Both new fixtures converge on one canonical IR with no parity-ignored fields. |
| **Fail-closed boundaries, redrawn precisely** | ✅ Component/outlet uses inside LIST bodies (content or own tree) deny with one precise reason each (`LIG-E001`, the per-item/keyed-component story) — the deep dive's probes 4/5 showed both previously LOWERED silently broken (a never-mounted outlet hole per item; one eager component field shared by every item, its root detached). The codegen backstops the For-body component case. Events in For bodies (LIG-E118), children/prop-pass/whole-style (M2), and `<template>`-outside-fills hold unchanged. |

## Fixtures

✅ Two new behavior fixtures, both legs each (27 → 29):

| Fixture | Pins |
| --- | --- |
| `slots-branch-component` | A `<Badge count={n()} />` inside an If branch on the header fill — the badge builds per mount as App's `Option<Badge>` field, inserts into the branch's component hole, initializes through the mount marking, disposes on unmount, and rebuilds fresh on remount (a late `set_n` lands on the next mount). One canonical IR: the fill carries the If; the branch carries the Component with its prop binding. |
| `slots-branch-outlet` | Two use sites fill `header` with a div carrying a `<Slot name="inner">` INSIDE an If branch; use 1 also fills `inner` (a dynamic `<b>`), use 2 does not (the outlet's own fallback mounts with the branch). One canonical IR: the branch fragment carries the nested Slot with its fallback; the fill and fallback carry Text binds. |

The KR-1 demo pins the runtime behavior on the mock host: the per-mount
rebuild/dispose cycle (including the late-write discipline), the
branch-time fill and fallback mounts, the composed-walk updates, and the
branch unmount taking its mounted content with it.

## Engineering baselines held

- **TDD**: 287 tests across 39 test binaries (M9: 276/39). The M10 shapes
  landed red-first: the frontend contract tests (branch admission both
  legs; the list-body denials; the own-tree For denials), the codegen
  contract tests (the direct-use attach, the per-mount rebuild/dispose,
  the branch fill/fallback mounts), the two fixtures, and the demo
  assertions failed at lowering/codegen/runtime before the implementation.
- **clippy pedantic**: `cargo clippy --all-targets --all-features -- -D
  warnings` green, benches included. M10 also retires every inline
  `#[allow]`/`#![allow]` in the tree (the workspace table's rows carry the
  rationales; `lig-patch` and the generated-code hygiene live in per-crate
  `[lints]`/`#![expect]` with reasons; the generator emits plain code).
- **Benches from day one**: `cargo bench --workspace --no-run` stays a CI
  gate; M10 adds the `codegen/branch` bench workload (module codegen over
  the slots-branch-component fixture — the per-mount rebuild/dispose arms,
  the assembly walk, the widened masks) with a `codegen_branch` ledger row
  (§14.2): release median 1 065 µs against a 6 000 µs ceiling (≈5.6×
  headroom). 12 ledger rows, all enforced in release.
- **Byte-stability**: the counter component digest is unchanged
  (`0x85d06a5c087b3589`); the unit grew 7 285 → 7 385 B (the build-phase
  doc comment names the M10 child-attach phase) after the generated lint
  blanket moved to the compile-clean crate's `[lints]` (7 437 → 7 285).
  `lig reproducibility` stays green.
- **Zero new external dependencies**: the Cargo graph is untouched.
- **CI/CD**: unchanged job set — the new fixtures ride the parity gate
  (29 fixtures, both legs), the new gen modules ride compile-clean; the
  admission ladder is unchanged (22 cases, 11 rules — the M10 denials are
  `LIG-E001` reasons, not new codes).

## Full gate run at delivery

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ (workspace table carries pedantic; see ADR-023 §5 for the CLI-flag precedence note) |
| `cargo test --workspace` | ✅ 287 tests / 39 binaries |
| `cargo bench --workspace --no-run` | ✅ |
| `lig fixtures verify` | ✅ 29/29 fixtures, both legs |
| `lig fixtures admission` | ✅ 22/22 cases (NFR-5) |
| `lig fixtures rules-sync` | ✅ 11 rules 1:1 with fixtures |
| `lig reproducibility` | ✅ byte-identical (7 385 B counter unit — digest unchanged `0x85d06a5c087b3589`) |
| `lig incremental` | ✅ 6 passes green (D-9 + LIG-E020) |
| `lig errors-gen` | ✅ 15 codes + 15 embedded docs, regeneration is a no-op |
| `lig patch-spike` | ✅ two generations, ~270 ms total, < 1 s per save |
| `lig budgets` (release) | ✅ all 12 rows within budget — branch 1 065 µs / 6 000 µs (5.6×), composed 1 001 µs / 6 000 µs (6.0×), codegen 586 µs / 3 000 µs, unit 7 385 B / 8 192 B |
| `lig no-std` | ✅ proof green (2 bare-metal targets + the generated unit) |
| `lig doctor` | ✅ all environment checks pass (view IR v5) |

## Notes and decisions

- **Why the attach contract unifies the milestone**: the deep dive found
  the direct-use detach (every nested component tree since M2), the
  branch-use detach (probe 3: an eager field, a never-inserted root, a
  prop-pass writing into it regardless of mount state), and the M9
  content insert was the ONE shape that worked. One contract — insert at
  the use hole exactly once per mount — covers all three, rides the
  existing `Host::insert`, and the per-mount identity story falls out of
  making the branch case honest (a stale eager instance cannot survive a
  remount cycle).
- **Why `Option<Child>` instead of re-insertion**: a remounted branch
  would re-insert a root the previous teardown destroyed (M9's reason).
  Rebuilding per mount gives the instance mount-scoped identity; the
  take-and-teardown on unmount keeps remount cycles leak-free without
  runtime changes (the slab teardown is the existing disposal walk).
- **Why fills for child-side branch outlets stay denied**: the fill is
  parent-authored (parent statics, parent reactions) and the mount arm is
  the child's — mounting it needs the child to call back into the parent
  at mount time (a registry or callback mechanism, a real design). The
  M9 unmatched-fill backstop denies with the precise reason instead of
  silently never mounting.
- **Why the Vue dispatch reorders instead of adding checks inside the
  component lowering**: the chain machinery (`lower_if_chain` /
  `lower_for`) already lowers ANY element as a branch/item root once
  `fragment_from_elem` dispatches component/slot tags — the reordering
  reuses it verbatim and keeps one lowering path per construct.
- **Why the generated lint blanket moved to `[lints]`**: the tree-wide
  zero-attribute discipline moves the suppression the blanket carried to
  the crate that compiles generated code, with a rationale per row
  (ADR-023 §5). The counter digest is unchanged — the blanket lived
  outside the digest.

## Known gaps (deferred, deliberate)

- Fills for child-side branch-internal outlets: fail-closed (the
  unmatched-fill backstop) — cross-component branch-mount-time fill
  coordination is the follow-up mechanism.
- Per-item component/outlet instances in list bodies: fail-closed
  (`LIG-E001`, the keyed-component story).
- Direct-rooted branch binds still initialize via co-read marking for
  branches without assembly; the `else_`/`v-else` mount arm still mounts
  the `then` branch only — both pre-existing, deferred.
- `v-model` on non-input elements and `<slot :name>` forwarding:
  fail-closed today (M6 gap, unchanged).
- Template refs on the Vue leg: the Solid leg's `ref={local}` works end
  to end; the Vue template has no `ref` spelling yet.
- Component-instance disposal at app teardown rides the host's document
  teardown (pre-existing boundary; the per-mount dispose covers remount
  cycles, which is where unbounded growth lived).
- `cargo vet` (§14.2 supply-chain onboarding, ADR-012): still deferred.
- A `lig patch` developer driver over real app dylibs: the engine surface
  is what that work wires into.
