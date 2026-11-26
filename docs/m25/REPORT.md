# M25 milestone report — the per-mount fill refs (the ref-composed guard)

Date: 2026-11-26 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the M18 gap list closes)

The per-mount fill assembly (M18) fixed the fills carried into
branch-mounted instances — the `Option<Child>` read/unwrap guards on binds
and events, the mounts riding the branch mounting arm's leg walk. One
named follow-up stayed: **ref captures inside those fills**. The build-body
emission assumed a plain instance field, so the generated unit for a ref
inside the fill of a branch-mounted use refused to compile — fail-loud, no
silent mis-addressing, but the shape could not ship. M25 makes the shape
work end to end:

- **The capture rides the mount walk.** The `Ctx::Fill` ref-capture split:
  direct uses keep the build-body emission byte-for-byte (their fills
  mount during the build's slot assembly); a branch-mounted use's fill
  refs route into the mounting arm's leg walk, emitted after the per-outlet
  fill mount and event attaches, guarded on the instance field — the M18
  spelling, now carrying the capture front. The ownership rule is the M7
  fill-ref discipline, per mount: the parent's `ref_<name>` field takes
  the freshly mounted fill hole; a remount cycle re-captures into the
  fresh node, never the disposed one (the `NodeId` discipline keeps the
  stale id inert).
- **The pre-collect.** The If mounting arm is emitted inside the reaction
  loop before the ref reactions may have been visited, so the per-mount
  refs pre-collect into a map keyed by the fill fragment before the loop
  starts; the leg walk emits nothing when its content has no entry — every
  unit without per-mount fill refs is byte-identical (D-8 untouched: the
  committed counter unit and every compile-clean gen module re-verified
  unchanged by `lig fixtures gen` + `lig reproducibility`).
- **Deeper nesting stays fail-closed.** A ref inside nested slot content
  of a branch-mounted use (the ctx base beyond `Direct`) denies with a
  precise codegen error — the leg walk has no capture walk for it, and a
  silent sentinel would be the M6-era fail-open shape.

## Design decisions (ADR-0039)

- Mount-time, not build-time-even-guarded: the instance is built in the
  mounting arm AFTER the build body runs, so a guarded build capture would
  always read `None` — the capture must live where the fill mounts.
- The `per_mount == false` path is untouched code: the split is by the
  flag the M18 guards already carry, so the direct-use emission (and every
  committed unit) does not move a byte.
- Bench, one new ledger row: `codegen/per-mount-fill-ref` joins the
  codegen bench family (the M11/M13/M14/M15 precedent — a new language
  surface measures `generate_module` over its fixture) with the
  `codegen_per_mount_fill_ref` row (30 rows; release median ≈ 787 µs,
  ceiling 4 ms, ≈ 5.1× headroom).

## Evidence

- Tests (`cargo test -p lig-codegen --test codegen`): the new
  `per_mount_fill_refs_capture_at_mount_time` pins the plan's acceptance —
  the build body never reads a per-mount instance's fill (the pre-M18
  spelling would not compile), and the mounting arm captures through the
  guarded spelling, writing the parent's field. 35 tests in the file, all
  green; the counter snapshot byte-stable.
- Fixture: `per-mount-fill-ref` (both frontend legs, 37 × 2 legs now) — a
  branch-mounted `Panel` use whose `header` fill carries `ref={mark}` /
  `ref="mark"`; `lig fixtures verify` matches the committed canonical IR on
  both legs, and `lig fixtures gen` regenerates only the new gen module
  (every other module byte-identical).
- Demo (`cargo test -p compile-clean --test demo`): 24 tests — the new
  `per_mount_fill_refs_capture_per_mount` drives the whole cycle on the
  mock host: capture at mount (the ref names the freshly mounted fill
  element), dispose on the flip off, a fresh capture on the remount (never
  the disposed node).
- Example: `cargo run -p compile-clean --example per_mount_fill_refs` —
  the mount → dispose → remount walk printed step by step.
- Bench: `codegen/per-mount-fill-ref` joins `cargo test --benches` and the
  release gate; `lig budgets` enforces the new row like the other 29.

## Remaining gaps / risks

- Cross-component branch-mount fill coordination (the ADR-024 §4 named
  follow-up: fills for outlets inside the CHILD's own branches) keeps the
  M9 unmatched-fill backstop — the mechanism remains a real design (the
  child cannot evaluate parent-authored binds or reach parent instance
  state).
- Refs inside nested slot content of a branch-mounted use keep the new
  fail-closed codegen denial until the mount walk grows a recursive
  capture walk (the same recursion the M9 nested-outlet mounts gained).
- `lig-taffy-profile` stays parked and `cargo vet` stays deferred
  (carried, unchanged).
