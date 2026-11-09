# M17 — Per-item event dispatch on composed list bodies

The third zone of the `LIG-E118` boundary falls: a handler DIRECTLY on a
For/v-for body now admits when the body is COMPOSED — the For hangs inside
a branch or inside slot content. The attach rides the composed reconcile's
receiver-parameterized insert arms (in place since M15), the dispatch rides
the same source-addressed per-item fire fn (context-free by construction),
and the admission walk drops the owner-based split entirely. The demo test
exposed and fixed a latent M8-era gap along the way: a list-carrying
branch remounts through the mount mark, so its fresh list table fills
again.

## Why composed bodies, and why now

ADR-0030 records the full argument; the short form: the composed denial
was the last zone whose mechanism was already receiver-neutral. M8 shipped
the composed reconcile (an associated fn addressing the list through the
ctx chain's receiver), M13 mirrored per-item ops into both plans, and M15
parameterized the per-item attach emission by receiver — the composed
insert arms have carried attach calls since M15, and the fire fns never
touch the owner fragment. Branch-in-item and nested-list events stay
denied: both need per-item control flow (the codegen backstop "control
flow inside list item bodies is not admitted in v1" is a foundation layer,
not an increment).

## What shipped

| Exit criterion | Result |
| --- | --- |
| **Composed body events admit and dispatch** | ✅ The admission walk admits events on the For body fragment itself regardless of ownership; `owned_body_fragments` and the `owner_direct` flag are gone (the walk keeps one boolean: body-own events vs subtree). The codegen `emit_item_method` drops its `direct` parameter and the composed backstop — the per-item replay and the fire fns emit identically for both classes. Pinned twice: two admission unit tests (composed admits; branch-in-composed-body still denies) and the `for-event-composed` fixture both legs. |
| **The composed runtime story, end to end** | ✅ `tests/compile-clean/tests/demo.rs::composed_list_events_dispatch_per_item`: two attaches on the branch mount (one per item, each at its own button node inside the branch's list slot), index-addressed dispatch substitutes the live item, a keyed rotation re-locates the records without re-attaching, the branch cycle (unmount → remount) disposes and re-attaches per mount, and a stale index stays a no-op. |
| **List-carrying branches remount (the latent M8 gap)** | ✅ The plain (no-assembly) If arm emitted no mount mark, so a remounted branch's list table — reconcile state, not template state — stayed empty until its source next changed. The arm now marks the subtree init mask when the branch subtree carries a For dynamic (the M8 init discipline, scoped to list-carrying branches so every list-free unit's bytes stay pinned); the reconcile re-inserts into the fresh table on the next flush pass. The demo test pins the remount (fresh slabs, fresh attaches). |
| **The conformance surface** | ✅ `for-event-composed` (35th fixture): a keyed `<For>`/`v-for` with a direct `@click`/`onClick` handler inside a `v-if`/`<Show>` branch, both legs byte-identical against the committed canonical IR. The admission ladder's `e118-for-handler/allow` case redrawn to the composed shape (22 cases green, rules-sync 1:1); the deny case stays branch-in-item. |
| **Budgeted from day one** | ✅ The `codegen/for-event-composed` criterion workload joins the bench suite; the `codegen_for_event_composed` ledger row joins `docs/benchmarks/budgets.toml`: release median 816 µs against a 4 500 µs ceiling (≈5.5× headroom). 19 ledger rows, all enforced in release; the registry sync check and the CLI's row assertions updated 1:1 (18 → 19). |
| **The consumer surface grows a third example** | ✅ `crates/lig-incremental/examples/composed_events.rs`: the admitted composed unit through the Session (the M16 `tracing` spans fire unchanged), the typed-emit path, the generated surface (the associated reconcile, the attach in its insert arms, the fire fn), and the still-denied zone rendered through miette with the fix and the catalog URL. |
| **Docs reconcile with the tree** | ✅ ADR-0030 records the decision; `LIG-E118`'s doc and the rule row narrow to the two remaining zones (branch-in-item, nested lists) with the help-text naming the M13 escape hatch; the README carries M17 in Features/Status/Learn-more and loses a duplicated examples block; `fixtures/README.md` counts 35 and pins the new fixture's behavior; the CHANGELOG's Unreleased section records the milestone. |
| **Drift and micro-debt cleared** | ✅ `tests/dummy-host` carried a stale, duplicated doc comment (the pre-edition `#[no_mangle]` spelling beside the real `#[unsafe(no_mangle)]` rationale): the stale copy is gone. The README's Usage section carried a duplicated examples block (the same two bullets and `cargo run` commands twice back to back): deduplicated, now naming all three examples. The dead `owned_body_fragments` machinery died with the M17 admission lift. |

## Fixtures

One added: `for-event-composed` (35 × 2 legs, byte-identical on both
frontends). The admission ladder stays 22 cases — the e118 allow case
redraws to the composed shape, the deny case stays branch-in-item.

## Engineering baselines held

- **TDD**: 327 tests across 39 binaries (M16: 324/39). The M17 shapes
  landed red-first: the composed-admits / branch-in-composed-still-denies
  admission pair watched the walk lift (the composed probe denied through
  the CLI before the change), and the composed demo test failed on the
  remount assertion before the mount-mark fix existed — it caught the
  M8-era gap the same day it shipped.
- **clippy pedantic**: `cargo clippy --workspace --all-targets
  --all-features -- -D warnings` green — zero warnings, benches, tests,
  and examples included, the `bench-swc` feature path compiled too.
  Pedantic rides the `[lints.clippy]` table (`pedantic = warn`,
  `priority = -1`) and CI's `-D warnings` escalates it — passing
  `-W pedantic` after `--` is wrong on modern rustc (the bare group name
  is an unknown lint there and `-D warnings` turns it into an error).
  Zero new suppressions: the tree still carries exactly the two
  pre-existing, rationale-commented `#![expect(unsafe_code)]` attributes
  (lig-patch's single-unsafe-module policy §15, ADR-013; the dummy-host
  spike binary) — both self-auditing (`expect` warns when the unsafe ever
  dries up).
- **Benches from day one**: `cargo test --benches` green (the stable
  criterion suite runs; `cargo bench --workspace --no-run` stays the CI
  compile gate). M17 adds the `codegen/for-event-composed` workload with
  the `codegen_for_event_composed` ledger row (§14.2): release median
  816 µs against a 4 500 µs ceiling (≈5.5× headroom). 19 ledger rows,
  all enforced in release.
- **Byte-stability**: every pre-existing generated unit is unchanged —
  `lig reproducibility` diffs the committed counter unit (7 385 B) against
  a fresh generation, byte-identical; `lig fixtures gen` after the
  codegen change rewrote nothing but the new fixture's module. The
  list-carrying-branch mount mark is scoped so no list-free unit moves.
- **Gates, all locally green on this tree**: fmt, clippy (above), the
  full test suite (327/39), `lig fixtures verify` (35 × 2 legs), `lig
  fixtures admission` (22 cases), `lig fixtures rules-sync` (1:1, 11
  rules), `lig reproducibility`, `lig incremental` (6 passes), `lig
  patch-spike` (two generations, PASS), `lig budgets` (release, 19/19
  rows).

## Known boundaries (redrawn once, still fail-closed)

The `LIG-E118` boundary keeps TWO zones after M17: events inside
list-item BRANCHES and events inside NESTED list bodies — both need
per-item control flow (per-item × per-mount and per-item × per-item
composition), the foundation follow-up. Composed lists with events beyond
scalar binds (fills for list-body outlets, state-reading fallback binds)
keep their precise codegen backstops; per-item slot content admits scalar
binds only (M14).
