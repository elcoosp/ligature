# M28 milestone report — nested-list events (per-item × per-item)

Date: 2026-11-27 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the E118 foundation follow-up, zone two)

The E118 boundary (ADR-0030 §4, narrowed by M15/M17/M18/M27) carried two
named denial zones; the M27 report carried the second unchanged: **events
inside nested list bodies** (per-item × per-item) — "each needs per-item
control flow … the named foundation follow-up, judged an increment away
from one mechanism milestone, not a codegen flag". M28 grows the per-item
control flow for the nested-list zone and closes it:

- **The inner list mounts per item.** The item replay grows the For arm:
  the inner reconcile is ONE commit in the outer apply (`reconcile_item_f{body}`
  — the three replay call sites — keyed, unkeyed, composed — cannot
  drift), and it addresses the ITEM's own fragment list table (the inner
  For's branch anchor slot; each item is its own `Fragment`). The
  per-item-square state IS the fragment-per-item: one outer index, one
  inner index, no parallel store — the inner keys, items and anchors live
  on the item fragment's `lists` vec and the recursive teardown disposes
  them with the item. No new schedule, no run arms, no build-time
  addressing: the inner body's reactions join the bodies', the per-item
  fallbacks' and the legs' in the flush-scope exclusion (one reaction,
  N×M live items — the M14 discipline squared).
- **The inner surface is the inner replay's reach.** The inner body's OWN
  scalar binds (item-bound; state-reading binds keep the M14
  state→per-item fan-out boundary) re-apply per replay pass through the
  inner apply; the inner body's OWN events attach ONCE at the inner
  insert arm (the M15 attach discipline one level deeper — the record
  disposes with the inner item; a stale pair stays inert). Structural
  content inside the inner body (nested If/For, components, slots, refs,
  html) and any deeper fragment keeps the fail-closed denial — the
  boundary moves, it does not disappear. The inner source reads the outer
  item binding directly (`each={g}`; the backend lattice types the item's
  element list), and inner sources/keys never read state.
- **Dispatch rides the M15 fire fns, squared.** One index-PAIR-addressed
  fire fn per inner event dynamic (`fire_f{body}_d{di}(h, outer_index,
  index, ev)` — the outer source through `self`, the inner source through
  the live outer item, the checked `CycleError` front; a stale pair is a
  no-op). The mounts and the fire fns agree on the same canonical fragment
  ids.
- **Admission walks each For chain exactly once.** The E118 walk rooted at
  a For body recurses into its nested bodies with `owner_direct` one level
  deep, and a For hanging on a For body never fires its own walk — the
  pre-M28 walks double-fired on nested chains (two identical diagnostics
  for one event), which the one-precise-reason contract cannot tolerate.
  The nested body's own events admit; its own branch legs and its own
  nested lists stay denied. The composed content contract lifts the same
  surface on both frontends (`ItemSurface::NestedBody` — the M18
  precedent: one milestone moves own-tree and composed together).
- **Nested literal state initializers lower.** The shape's data is a list
  of lists; `init_is_literal` accepts nested literal arrays recursively,
  so `createSignal([["alpha", "beta"]])` lowers — additive to every
  pre-existing shape.
- **The Vue leg needed no lowerer change.** Nested `v-for` on a real
  element (`<ul v-for="g in groups"><li v-for="n in g">`) already lowered
  to the same IR shape the Solid leg produces — the parity harness pins it
  byte-for-byte.

## Design decisions (ADR-0042)

- The inner replay IS the per-item-square control flow: the reconcile is
  one commit in the outer apply, so the keyed, unkeyed and composed
  replay call sites cannot drift (the M27 single-mechanism discipline).
- Replay-time, idempotent: the inner keyed diff re-runs per outer item per
  pass — unchanged keys produce no ops, changed keys ride the same
  LIS-stable plan; a fresh outer item mounts its inner table through the
  same path (never the disposed nodes).
- Bench, one new ledger row: `codegen/for-event-nested` joins the codegen
  bench family (the M11/M13/M14/M15/M25/M26/M27 precedent — a new language
  surface measures `generate_module` over its fixture) with the
  `codegen_for_event_nested` row (33 rows; release median 1044 µs, ceiling
  5 500 µs, ≈5.3× headroom).

## Evidence

- Tests (`cargo test -p lig-codegen --test codegen`): the new
  `for_event_nested_list_compiles_per_item_square` pins the plan's
  acceptance — the shape compiles (the E118 zone lifts), the inner
  reconcile is called from the per-item replay, the inner list mounts on
  the item fragment's own table with the attach per fresh inner item, the
  inner apply carries the outer item, and dispatch stays index-pair-addressed.
  38 tests in the file, all green; the counter snapshot byte-stable.
- Admission: the boundary tests move — `for_handler_in_nested_list_admits`
  (the nested body's own events admit),
  `for_handler_in_nested_list_leg_is_e118` and
  `for_handler_in_doubly_nested_list_is_e118` (the nested body's legs and
  deeper nesting keep one precise denial), 22 admission tests green.
  Frontends: `nested_list_events_admit_in_composed_content` /
  `nested_list_events_leg_in_composed_content_denies` (parse-solid, 27
  green) lift the composed content contract. Fixture: `e118-for-handler` —
  the allow case moves to the nested shape, the deny case to a
  nested-body-leg shape (line 11), 22/22 admission cases green.
- Fixture: `for-event-nested` (both frontend legs, 40 × 2 legs now) — a
  keyed `For` whose body root is a `ul` carrying a keyed inner `For` with a
  handler on the inner body; `lig fixtures verify` matches the committed
  canonical IR on both legs, and `lig fixtures gen` regenerates only the
  new gen module (plus the one stale M27 module, see below).
- Demo (`cargo test -p compile-clean --test demo`): 27 tests — the new
  `nested_list_events_mount_and_dispatch_per_item_square` drives the whole
  cycle on the mock host: the inner table on the item fragment (2 attaches,
  one per inner item), per-pair dispatch ((0,1) → beta, (0,0) → alpha),
  stale pairs inert, a fresh outer item mounting its own inner list
  ((1,0) → gamma), the keyed inner move repositioning the SAME fragment
  (beta 1 → 0, no remount), removal disposing the inner tables with the
  items, and a fresh remount re-attaching exactly once.
- Example: `cargo run -p compile-clean --example for_event_nested` — the
  mount → per-pair dispatch → square fan-out → keyed move + teardown →
  fresh-remount walk printed step by step.
- Bench: `codegen/for-event-nested` joins `cargo test --benches` and the
  release gate; `lig budgets` enforces the new row like the other 32.

## Remaining gaps / risks

- Cross-component branch-mount fill coordination (the ADR-024 §4 named
  follow-up: fills for outlets inside the CHILD's own branches) keeps the
  M9 unmatched-fill backstop — the mechanism remains a real design (the
  child cannot evaluate parent-authored binds or reach parent instance
  state).
- Doubly-nested lists (per-item × per-item × per-item) and state-reading
  per-item conditions/binds/sources keep their boundaries (the
  state→per-item fan-out; the inner source must read the outer item
  binding directly — the backend lattice types the item's element list).
- `lig-taffy-profile` stays parked and `cargo vet` stays deferred
  (carried, unchanged).
- `docs/m24/REPORT.md`'s "deliberately not the library's policy" nearest-hit
  note is stale (the promotion shipped as `resolve_nearest`, commit
  `c1d065c`) — reported, left as-is: the report is a historical record.
- The M27-era committed gen module `for_event_item_branch` was stale
  against its own codegen (found by `lig fixtures gen` while landing this
  milestone: the tracker param, the index conversion and the
  identical-leg bind collapse had moved after the last regen); the
  refreshed module ships with this milestone.
