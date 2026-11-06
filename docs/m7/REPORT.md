# M7 milestone report — dynamic slot content (slot-scoped addressing, the ref-capture split, exact build assembly)

Date: 2026-11-06 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the first row of M6's "known gaps")

The M6 report deferred dynamic slot content: "fills/fallbacks are static
subtrees (denied at lowering); lifting the restriction needs slot-scoped
run arms and an init-flush story for mounted slot instances." M7 lifts it.
One runtime concept (`SlotOrigin` — who authored the mounted content), no
new crates, no new error codes, no new external dependencies, no IR change
(schema stays v5).

| Exit criterion | Result |
| --- | --- |
| **Dynamic fills** | ✅ A fill may carry scalar binds and events — `header={<button onClick={() => setLabel("next")}>{label}</button>}` (Solid) / `<template #header><button @click="label = 'next'">{{ label }}</button></template>` (Vue). The bind run arms address the mounted content through `slot_fill_mut`; the event attaches across slabs at mount time (`attach_foreign`) with the handler record on the parent's fragment — dispatch marks the parent, the parent's flush rewrites the content. |
| **Dynamic fallbacks** | ✅ A fallback may carry scalar binds — `<slot name="foot"><i class="fb">{{ title }}</i></slot>`. The child's own bind run arm addresses `slot_fallback_mut`; because the child's build flushed before the content was mounted, the PARENT's build completes initialization: it marks the child's fallback reactions (exact per-reaction mask, event handlers excluded) and flushes the child instance — the init-flush story M6 named as a prerequisite. |
| **The ref-capture split** | ✅ `ref={el}` captures the hole NodeId of the fragment the ref lives on, in four phases: direct refs in the build body (`s.ref_x = s.frag.hole(n)`, byte-stable M0 shape), fill refs through the mounted content, fallback refs through the child's outlet (the parent writes the child's `pub` field — the fallback belongs to the child's tree), branch refs after the initial flush mounts the branch. Every ref field starts as the absent sentinel `NodeId(u32::MAX)`. Pinned by a codegen contract test covering all four contexts. |
| **One addressing discipline (`Ctx`)** | ✅ Every run arm — binds, classes, attrs, style props, html, If mounts — resolves its receiver through one `Ctx` classification of the owner fragment: `Direct` (`self.frag`), `Branch { slot }`, `Fill { field, slot }`, `Fallback { slot }`. Before M7 only Text had a branch special case; every other kind wrote to `self.frag` regardless of where the dynamic lived. The branch-slot lookup also fixed: it was the raw children position (fill fragments are tree children too and would shift it), now it is the owner dynamic's If/For ordinal. |
| **Exact build assembly** | ✅ The initial-flush mask is per-member exact: the bit IS the global reaction id. The M0-era `(1 << n) - 1` band marked the wrong component's ids for every component after the first in a module (the M6 fixtures could not see it — static slot content never needed a late-id flush) and missed its own late ids. Script effects still run at build (the `effect_multi` demo pins it); only event-driven handler reactions are excluded (their bits are exactly the dispatch set). |
| **Fail-closed boundaries held** | ✅ If/For/Component/Slot inside slot content stay denied (`LIG-E001`, one precise reason each): branch/list tables hang on `self.frag`, prop-pass crosses into a grandchild, nested outlets need composed contexts. Two M6-era fail-opens closed on the Vue leg: `fragment_from_elem` silently STRIPPED structural directives on fill/fallback roots (the static-content check ran after the strip, so `<template #header><p v-if="on"/></template>` was accepted with the `v-if` erased), and component tags on content roots lowered as plain elements (the use silently vanished). Both now deny with `LIG-E001`. |

## Fixture

✅ One new behavior fixture, both legs each (23 → 24):

| Fixture | Pins |
| --- | --- |
| `slots-dynamic` | Use 1 fills `header` with a button carrying a delegated click + a live text bind; the `foot` outlet is unfilled, so the child's own fallback (`<i class="fb">{{ title }}</i>`) mounts with its bind. One canonical IR: the fill fragment (Event + Text dynamics) hangs on the parent's tree, the fallback fragment (Text) on the child's. |

The KR-1 demo pins the runtime behavior on the mock host: per-outlet
origins (`SlotOrigin::Fill` / `SlotOrigin::Fallback`), the fallback bind
initialized by the parent-driven init-flush, the fill bind initialized at
build, and the fill event dispatching across slabs (`fire_1` → the parent's
handler record → the slot-scoped bind rewrites the content text).

## Engineering baselines held

- **TDD**: 263 tests across 39 test binaries (M6: 255/39). The runtime
  landed with `slot_origin`-gated accessor tests and an
  `attach_foreign`-across-slabs dispatch test; the codegen with the
  four-context ref-capture test; both frontends with admit + deny contract
  tests; the fixture and its demo assertion pin the end-to-end behavior.
- **clippy pedantic**: `cargo clippy --workspace --all-targets -- -D
  warnings` green, benches included; no new allowlist rows.
- **Benches from day one**: `cargo bench --workspace --no-run` stays a CI
  gate; the M7 features ride the existing workloads (slot-content binds
  are run arms; the fixture adds no new bench surface).
- **CI/CD**: unchanged job set — the new fixture rides the parity gate
  (24 fixtures, both legs), the new gen module rides compile-clean; no new
  gate, no new error code, the admission ladder is unchanged (22 cases,
  11 rules).
- **git-cliff**: changelog continues on conventional commits (`v0.1.0-m7`).
- **Zero new external dependencies**: the Cargo graph is untouched.

## Full gate run at delivery

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ |
| `cargo test --workspace` | ✅ 263 tests / 39 binaries |
| `cargo bench --workspace --no-run` | ✅ |
| `lig fixtures verify` | ✅ 24/24 fixtures, both legs |
| `lig fixtures admission` | ✅ 22/22 cases (NFR-5) |
| `lig fixtures rules-sync` | ✅ 11 rules 1:1 with fixtures |
| `lig reproducibility` | ✅ byte-identical (7 437 B counter unit — the build fn grew by the M7 phases doc comment; digest unchanged `0x85d06a5c087b3589`) |
| `lig incremental` | ✅ 6 passes green (D-9 + LIG-E020) |
| `lig errors-gen` | ✅ 15 codes + 15 embedded docs, regeneration is a no-op |
| `lig patch-spike` | ✅ two generations, **274 ms** total, < 1 s per save |
| `lig budgets` (release) | ✅ all 10 rows within budget — keyed row 3.9 µs / 25 µs (6.4×), codegen 580 µs / 3 000 µs, unit 7 437 B / 8 192 B |
| `lig no-std` | ✅ proof green (2 bare-metal targets + the generated unit) |
| `lig doctor` | ✅ all environment checks pass (view IR v5) |

## Notes and decisions

- **Why `SlotOrigin` lives in the runtime**: fill and fallback run arms
  address DIFFERENT fragments behind the same outlet — a fallback
  reaction must never write into a fill and vice versa. Origin-gated
  accessors (`slot_fill`/`slot_fill_mut` vs `slot_fallback`/
  `slot_fallback_mut`) make the wrong-fragment write unrepresentable;
  a bare `slot_content_mut` would have made the mix-up a runtime bug.
- **Why the parent owns the whole mount-time story**: the parent's build
  is the only place both origins exist — it mounts the content, attaches
  the content's events (record on the parent for fills, on the child for
  fallbacks), captures the content's refs, and runs the fallback
  init-flush. The child's build emits its fallback bind arms but CANNOT
  attach or capture at its own build time (the content does not exist
  yet); the M6 module-wide statics sharing already names the child's
  templates from the parent, so naming the child's holes and fields is
  the same discipline, not a new boundary crossing.
- **Why the init mask became per-member exact now**: with static slot
  content, a late-id initial bind either ran against statics (no-op
  difference) or was masked in by the width band by accident. The
  fallback init-flush needs EXACT bits — marking the wrong component's
  reactions would flush stale arms; missing the fallback's own bits
  would leave the content uninitialized. The band could not be made
  correct, so it is gone.
- **Why `Ctx` is flat (no composed contexts)**: content is denied If/For
  (M7 boundary), so a branch can only hang on the component's own root —
  `Branch { slot }` with a `Direct` base is the only reachable shape.
  Composed contexts (branch-in-content) arrive with the milestone that
  lifts that denial; the enum is ready to grow a base field then.
- **Why the branch-slot lookup changed**: the old lookup returned the
  fragment's raw children position. That was the branch table index only
  while branch fragments were the only tree children — M6's fill
  fragments made it wrong (a fill before an If in the parent's children
  shifted the slot). The lookup now reads the owner dynamic's If/For
  ordinal, which IS the branch table index by construction.

## Known gaps (deferred, deliberate)

- If/For/Component/Slot inside slot content: fail-closed (`LIG-E001`).
  Branch-in-content and list-in-content need content-scoped branch/list
  tables or composed `Ctx` contexts; component-in-content needs
  grandchild prop-pass through mounted instances.
- Template refs on the Vue leg: the Solid leg's `ref={local}` works end
  to end (the ref-capture split pins it), but the Vue template has no
  `ref` spelling yet — a two-leg ref fixture waits for that lowering.
- `v-model` on non-input elements (checkbox/radio/select) and
  `<slot :name>` forwarding: fail-closed today (M6 gap, unchanged).
- `cargo vet` (§14.2 supply-chain onboarding, ADR-012): still deferred —
  the pin table remains the reviewed surface.
- A `lig patch` developer driver over real app dylibs (app-crate scaffold
  + dev channel): the engine surface is what that work wires into.
