# M16 milestone report — the consumer surface (examples, session observability, typed emission errors)

Date: 2026-11-09 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered

M11–M15 shipped compiler behavior; the consumer surface stopped growing at
M3. M16 turns the milestone inward — the embed API, the examples directory,
and the docs — with zero compiler-behavior change (the counter unit is
byte-identical; D-8 holds):

| Exit criterion | Result |
| --- | --- |
| **Runnable consumer examples** | ✅ `crates/lig-incremental/examples/` — `compile_and_emit` (the Session end to end: cold pass → `all_reused` warm pass asserted → `emit_units` twice, written then digest-gated **kept**, the generated unit's digest + head) and `render_denial` (a `LIG-E114` denial: BR-1 asserted on the unit, the diagnostic rendered through miette's graphical handler — code frame, caret, `help:`, the `ligature.dev/e/LIG-E114` URL). Both fixture-fed, both run green: `cargo run -p lig-incremental --example …`. |
| **Session observability (tracing spans)** | ✅ The session engine's stages walk named spans: `session_compile` (per unit, in the pass loop), `lower` (the lowered-query compute), `admit` (tiering + LIG-E020 cycle check), `codegen` (D-8 emission of Rust), and `emit` (with recorded `written`/`kept` counts). Plain `tracing` — no subscriber dependency; the seam is pinned by a capture test with a hand-rolled subscriber, so it cannot silently disappear. |
| **Typed emission errors (miette help-text)** | ✅ `emit_units` fails as `lig_incremental::emit::EmitError` — a `miette::Diagnostic` with stable code `ligature::emit`, the offending unit/artifact as `subject`, `help` naming the fix (I/O: the atomic write→rename contract and what blocks it; contract violations: "report it", never "retry"), and the underlying `std::io::Error` as `source()`. The CLI's `emit_and_report` prints the help line alongside the message. The bare-`String` era on this seam is over. |
| **The session engine is budgeted** | ✅ The `session/compile-counter` criterion bench (a full cold pass: set_source + compile over the M0 reference workload — lower, admission, the build-order check, codegen) joins the bench suite; the `session_compile_counter` ledger row joins `docs/benchmarks/budgets.toml`: release median 691 µs against a 3 500 µs ceiling (≈5.1× headroom). 18 ledger rows, all enforced in release; the registry sync check and the CLI's row assertions updated 1:1 (17 → 18). |
| **Docs reconcile with the tree** | ✅ The CHANGELOG backfills the releases git carried but the file skipped (v1.1.0, v1.2.0 = M11+M12, v1.2.1 = M13) plus an Unreleased section (M14/M15/M16 — v1.3.0 is what the next release tags); `fixtures/README.md` names the shipped schema version (View IR v5, not the M2-era v2); the session rustdoc's article fixed; the README points consumers at the examples and records M16 in Features/Status/Learn-more. |
| **Micro-debt cleared** | ✅ The gate paths (`errors-gen` ×2, the NFR-5 harness naming, `expect_clean` ×2) no longer `unwrap()` path components — stemless names and root paths degrade to the display path with a comment at each site (a gate must fail with a message, never an unwind). |

## Fixtures

None added — deliberately. The fixture contract is untouched: 34 fixtures ×
both legs byte-compare green, the admission ladder stays 22 cases, and
`lig reproducibility` pins the counter unit at 7 385 B, byte-identical.

## Engineering baselines held

- **TDD**: 324 tests across 39 binaries (M15: 321/39). The M16 shapes
  landed red-first: the span-capture test (with a hand-rolled
  subscriber) and the typed-emission-error test failed before the
  implementation existed — the observability seam and the `EmitError`
  contract are pinned by tests that watched them be born.
- **clippy pedantic**: `cargo clippy --workspace --all-targets
  --all-features -- -D warnings` green — zero warnings, benches and
  examples included, the `bench-swc` feature path compiled too. The
  enforcement is empirical: two pedantic violations introduced during
  development (`used_underscore_binding`, `match_same_arms`) errored
  under exactly this command and were fixed, not suppressed. The
  `[lints.clippy]` table carries `pedantic = warn` and CI's `-D
  warnings` escalates it — passing `-W pedantic` after `--` is wrong
  on modern rustc (the bare group name is an unknown lint there and
  `-D warnings` turns it into an error). Zero new suppressions: the
  tree still carries exactly the two pre-existing, rationale-commented
  `#![expect(unsafe_code)]` attributes (lig-patch's single-unsafe-module
  policy §15, ADR-013; the dummy-host spike binary) — both self-auditing
  (`expect` warns when the unsafe ever dries up).
- **Benches from day one**: `cargo test --benches` green (the stable
  criterion suite runs; `cargo bench --workspace --no-run` stays the CI
  compile gate). M16 adds the `session/compile-counter` workload with
  the `session_compile_counter` ledger row (§14.2): release median
  691 µs against a 3 500 µs ceiling (≈5.1× headroom). 18 ledger rows,
  all enforced in release.
- **Byte-stability**: the counter component digest is unchanged —
  `lig reproducibility` diffs the committed unit (7 385 B) against a
  fresh generation, byte-identical. No compiler crate's output moved.
- **Gates, all locally green on this tree**: fmt, clippy (above), the
  full test suite (324/39), `lig fixtures verify` (34 × 2 legs), `lig
  fixtures admission` (22 cases), `lig fixtures rules-sync` (1:1, 11
  rules), `lig reproducibility`, `lig incremental` (6 passes), `lig
  patch-spike` (two generations, 270 ms total — PASS), `lig budgets`
  (release, 18/18 rows), `lig no-std` (runtime + generated unit across
  thumbv7em-none-eabihf and aarch64-unknown-none-softfloat).

## Known boundaries (unchanged, redrawn nowhere)

The `LIG-E118` boundary stays where M15 drew it: events inside
list-item BRANCHES, on nested list bodies, and on composed lists still
deny — that is the next compiler milestone, judged (for the first time)
by consumers who can run `examples/` first.
