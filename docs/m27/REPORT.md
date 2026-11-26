# M27 milestone report — branch-in-item events (per-item × per-mount)

Date: 2026-11-26 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the E118 foundation follow-up, zone one)

The E118 boundary (ADR-0030 §4, narrowed by M15/M17/M18) kept two named
denial zones; the M26 report carried both unchanged: **events inside
list-item branches** (branch-in-item) and **events inside nested list
bodies** (per-item × per-item) — "each needs per-item control flow … the
named foundation follow-up, judged an increment away from one mechanism
milestone, not a codegen flag". M27 grows the per-item control flow for
the branch-in-item zone and closes it:

- **The branch mounts per item.** The item replay grows the branch arm:
  the If evaluates with the item substituted, the flip mounts/unmounts on
  the ITEM's own fragment (each item is its own `Fragment` with its own
  branch table — `list_insert` already instantiated the body template with
  its branch anchors), and the replay loop's fan-out re-evaluates per item
  per pass. No new schedule, no run arms, no build-time addressing: the
  leg reactions join the bodies' and the per-item fallbacks' in the flush
  scope exclusion (one reaction, N live items — the M14 discipline).
- **The leg surface is the mount arm's reach.** The leg's OWN scalar binds
  (item-bound; state-reading binds keep the M14 state→per-item fan-out
  boundary) re-apply per replay pass through the mounted leg; the leg's
  OWN events attach ONCE at the mount into the item's slab
  (`attach_foreign` — the record disposes with the item; stale ids stay
  inert). Structural content inside legs (nested If/For, components,
  slots, refs, html) and any deeper fragment keeps the fail-closed denial
  — the boundary moves, it does not disappear. The flip never reads state:
  item-scoped conditions only (the M14 boundary again).
- **An else leg adds a per-item leg tracker.** `branch_is` alone cannot
  tell the two legs apart (the M11 finding), so `legs_f{body}_d{di}:
  Vec<u8>` rides the component (0 none / 1 then / 2 else), passed to the
  apply fn as a `&mut` param and mirrored by BOTH reconciles' plan ops
  (insert pushes 0, remove drops, move repositions — the M13
  instance-store discipline, one `u8` per item). Direct-rooted branches
  stay tracker-free and byte-stable — D-8 untouched: every committed
  compile-clean gen module re-verified unchanged by `lig fixtures gen` +
  `lig reproducibility`.
- **Dispatch rides the M15 fire fns.** One index-addressed fire fn per leg
  event dynamic (`fire_f{leg}_d{di}` — source-addressed, the checked
  `CycleError` front); the mounts and the fire fns agree on the same
  canonical fragment ids.
- **The Vue leg gains the v-for + v-if chain.** The two-leg parity
  contract had no Vue spelling for a branch on the item-body root
  (`v-for` consumed the element; a residual `v-if` hit the chain-bug
  backstop). `lower_for_item_chain` gives the Vue-3 semantics — `v-if` on
  a `v-for` element binds PER ITEM — with trailing `v-else` siblings
  consumed; the item fragment carries the If dynamic with the legs as its
  children, the same shape the Solid leg's `<Show>`-rooted body produces.
- **The denial lifts for the leg surface.** `walk_item_body_events` admits
  events on the body's DIRECT If legs (one level — the mount arm's reach);
  the content-contract walk lifts the same surface on both frontends (the
  M18 precedent: one milestone moves own-tree and composed together). The
  legs' subtrees and nested lists keep the denial with one precise reason
  at the event's own span.

## Design decisions (ADR-0041)

- The item replay IS the per-item control flow: no parallel walker — the
  flip lives in the same `apply_item_f{body}` the binds ride, so the three
  replay call sites (keyed, unkeyed, composed) cannot drift.
- Mount-time, guarded: the leg mounts inside the replay's apply, guarded
  on the item's branch table and the per-item tracker, so a remount cycle
  re-mounts and re-attaches into fresh nodes — never the disposed ones.
- Bench, one new ledger row: `codegen/for-event-item-branch` joins the
  codegen bench family (the M11/M13/M14/M15/M25/M26 precedent — a new
  language surface measures `generate_module` over its fixture) with the
  `codegen_for_event_item_branch` row (32 rows; ceiling 3 500 µs, the
  for-event family shape ≈5× the M15 row).

## Evidence

- Tests (`cargo test -p lig-codegen --test codegen`): the new
  `for_event_item_branch_attaches_per_mount` pins the plan's acceptance —
  the shape compiles (the E118 zone lifts), the branch mounts per item
  inside the replay (guarded on the item's branch slot), the leg's handler
  attaches at the mount, the leg's item-bound bind re-applies per replay,
  and dispatch stays index-addressed. 37 tests in the file, all green;
  the counter snapshot byte-stable.
- Admission: the boundary tests move — `for_handler_in_item_branch_admits`
  and `for_handler_in_composed_body_branch_admits` (own-tree and composed
  legs admit), `for_handler_in_item_branch_leg_subtree_is_e118` and
  `for_handler_in_composed_body_branch_subtree_is_e118` (the legs'
  subtrees keep one precise denial). Fixture: `e118-for-handler` — the
  allow case moves to the leg shape, the deny case to a leg-subtree shape
  (line 12), 22/22 admission cases green.
- Fixture: `for-event-item-branch` (both frontend legs, 39 × 2 legs now) —
  a keyed `For` whose body root is a `Show` with `when={n.length > 4}` and
  a `fallback` leg, both legs carrying handlers; `lig fixtures verify`
  matches the committed canonical IR on both legs, and `lig fixtures gen`
  regenerates only the new gen module (every other module
  byte-identical).
- Demo (`cargo test -p compile-clean --test demo`): 26 tests — the new
  `item_branch_events_mount_and_dispatch_per_item` drives the whole cycle
  on the mock host: side-by-side then/else legs across items (alpha →
  then, beta → else), two attaches (one per item), per-item dispatch
  through the leg fire fns (alpha / short), dispose with the item, and a
  fresh mount re-attaching exactly once (never the disposed node).
- Example: `cargo run -p compile-clean --example for_event_item_branch` —
  the mount → per-item dispatch → remove → stale-no-op → fresh-remount
  walk printed step by step.
- Bench: `codegen/for-event-item-branch` joins `cargo test --benches` and
  the release gate; `lig budgets` enforces the new row like the other 31.

## Remaining gaps / risks

- Nested-list events (per-item × per-item) keep the `LIG-E118` discipline
  — the remaining zone of the named foundation follow-up, its own
  mechanism milestone (the inner list's reconcile state and dispatch
  addressing are per-item-square: one outer index, one inner index).
- State-reading item-branch conditions and leg binds keep the M14
  state-to-per-item fan-out boundary (the precise backstop message names
  it).
- Cross-component branch-mount fill coordination (the ADR-024 §4 named
  follow-up) keeps the M9 unmatched-fill backstop — the mechanism remains
  a real design.
- `lig-taffy-profile` stays parked and `cargo vet` stays deferred
  (carried, unchanged).
- `docs/m24/REPORT.md`'s "deliberately not the library's policy" nearest-hit
  note is stale (the promotion shipped as `resolve_nearest`, commit
  `c1d065c`) — reported, left as-is: the report is a historical record.
