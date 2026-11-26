# M26 milestone report — the mount walk's recursive capture walk

Date: 2026-11-26 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the M25 gap list closes)

The per-mount fill refs (M25) routed the TOP-LEVEL fill's ref captures into
the branch mounting arm's leg walk and left one named follow-up:
**refs inside nested slot content of a branch-mounted use** kept a precise
fail-closed codegen denial "until the mount walk grows a recursive capture
walk (the same recursion the M9 nested-outlet mounts gained)". Probing the
boundary proved the gap was wider than the capture: the M18 leg walk
mounted only the top-level fill, so a nested outlet's fill inside a
branch-mounted use's fill NEVER mounted at all — the composed bind run arms
addressed content that was never inserted (a silent fail-open, the M6-era
shape in a new location) — and the child's root-outlet fallback content,
mounted per mount, had no ref capture either (its `ref_` fields stayed at
the sentinel forever). M26 grows the mount walk the M9 recursion and closes
all three:

- **The leg walk recurses.** The M18 per-mount fill mount (and its fallback
  twin) calls the shared `walk_content_mounts` on the freshly mounted
  content, per mount: nested outlets mount their use-site fills or own
  fallbacks, grandchild roots insert, nested events attach, nested refs
  capture. The walk's receivers are the caller-composed `Option`
  expressions of the walked content, so the per-mount entry passes the
  guarded instance chains
  (`self.<field>.as_ref().and_then(|inst| inst.frag.slot_fill(<outlet>))`)
  and every deeper level chains one more `and_then` — the M18 guard
  discipline rides the whole recursion.
- **The author spelling is a walk parameter.** Build-body statements bind
  `s`, mounting-arm statements address `self`, per-mount child-authored
  statements guard on the `Option<Child>` field and write the CHILD
  instance's field through `inst` (the M7 fallback-ref ownership, per
  mount). This also fixes the branch-time assembly's ref captures, which
  spelled `s.` in run-arm context where no `s` exists — a shape no
  committed unit exercised (it would not compile), now spelled correctly.
- **Nested fills join the mount mask.** Nested fill contents are children
  of the use site's owner fragment, not of the content they mount into, so
  the M18 widening missed their reactions. The walk reports every
  fill-origin content it mounts; the caller widens the mount mask by each
  (scoped to per-mount contexts so every unit without per-mount fills is
  byte-identical — D-8 untouched: the committed counter unit and all 37
  pre-existing compile-clean gen modules re-verified unchanged by
  `lig fixtures gen` + `lig reproducibility`).
- **The denial lifts for the covered chains.** `per_mount_capture_covered`
  admits exactly the base chains the mounting arms' walks reach: per-mount
  fill, branch, and fallback links bottoming at the per-mount top-level
  fill. Chains the walk cannot reach keep the fail-closed denial — the
  boundary moves, it does not disappear.

## Design decisions (ADR-0040)

- One shared recursion, not a parallel per-mount walker: the M9 walk gains
  two receiver expressions and an author parameter instead of the leg walk
  growing its own second copy — the three call sites (build phase, branch
  assembly, per-mount leg walk) cannot drift.
- Mount-time, guarded: the nested fills mount inside the branch mounting
  arm, guarded on the `Option<Child>` field at every level, so a remount
  cycle re-mounts and re-captures into fresh nodes — never the disposed
  ones (the `NodeId` discipline keeps stale ids inert).
- The top-level fallback ref capture (the build path's M7 shape,
  1:1 per-mount-guarded) rides the same leg walk block the M25 fill
  captures live in.
- Bench, one new ledger row: `codegen/per-mount-nested-fill-ref` joins the
  codegen bench family (the M11/M13/M14/M15/M25 precedent — a new language
  surface measures `generate_module` over its fixture) with the
  `codegen_per_mount_nested_fill_ref` row (31 rows; release median ≈ 1111 µs,
  ceiling 6 ms, ≈ 5.4× headroom).

## Evidence

- Tests (`cargo test -p lig-codegen --test codegen`): the new
  `per_mount_nested_fill_refs_walk_at_mount_time` pins the plan's
  acceptance — the shape compiles (the M25 denial lifts), the nested fill
  mounts per mount through the guarded chain (two `SlotOrigin::Fill`
  mounts), and the nested ref captures through the composed read chain
  into the PARENT's field. 36 tests in the file, all green; the counter
  snapshot byte-stable.
- Fixture: `per-mount-nested-fill-ref` (both frontend legs, 38 × 2 legs
  now) — a branch-mounted `Panel` use whose `header` fill carries a nested
  `<slot name="inner">` outlet and whose `#inner` fill carries
  `ref={mark}` / `ref="mark"`; `lig fixtures verify` matches the committed
  canonical IR on both legs, and `lig fixtures gen` regenerates only the
  new gen module (every other module byte-identical).
- Demo (`cargo test -p compile-clean --test demo`): 25 tests — the new
  `per_mount_nested_fill_refs_walk_per_mount` drives the whole cycle on
  the mock host: the nested fill mounted at branch mount (pre-M26 it never
  mounted), the ref naming the freshly mounted NESTED fill element, the
  nested bind live and updating through the composed chain, dispose on the
  flip off, a fresh capture on the remount (never the disposed node).
- Example: `cargo run -p compile-clean --example per_mount_nested_fill_refs`
  — the mount → nested capture → nested bind → dispose → remount walk
  printed step by step.
- Bench: `codegen/per-mount-nested-fill-ref` joins `cargo test --benches`
  and the release gate; `lig budgets` enforces the new row like the other
  30.

## Remaining gaps / risks

- Cross-component branch-mount fill coordination (the ADR-024 §4 named
  follow-up: fills for outlets inside the CHILD's own branches) keeps the
  M9 unmatched-fill backstop — the mechanism remains a real design (the
  child cannot evaluate parent-authored binds or reach parent instance
  state).
- Events inside list-item branches and nested lists keep the `LIG-E118`
  discipline (the named foundation follow-up — per-item × per-mount and
  per-item × per-item composition, each its own mechanism milestone).
- `lig-taffy-profile` stays parked and `cargo vet` stays deferred
  (carried, unchanged).
- `docs/m24/REPORT.md`'s "deliberately not the library's policy" nearest-hit
  note is stale (the promotion shipped as `resolve_nearest`, commit
  `c1d065c`) — reported, left as-is: the report is a historical record.
