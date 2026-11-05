# M0 spike report — all four gates green

Date: 2026-11-05 · Toolchain: 1.99.1 · Pins: see ADR-012 · Runner: Linux x86_64

## Outcome by spike (§17 M0 acceptance)

| Spike | Gate | Result |
| --- | --- | --- |
| **S1** — TSX → IR v2 → generated Rust renders `counter` on mock host | S1 renders | ✅ `lig-parse-solid` → `lig-ir` v2 → `lig-codegen` (`quote!`→`syn`→`prettyplease`) → `adapters/mock-host`; insta snapshot + compile-clean gate green |
| **S2** — subsecond-style patch on a dummy host (fat/thin, jump table), measure save→frame | < 1 s save→frame on Linux+macOS | ✅ **~125–140 ms** on Linux (`xtask patch-spike`); protocol apply ~90–165 µs, routed frame ~1 µs; macOS gate in CI matrix (`docs/benchmarks/save-to-frame.md`) |
| **S3** — lightningcss → taffy `Style` consts for the 25-property profile v1 | const-compiles + snapshot | ✅ `lig-style` schema + `lig-taffy-profile` const-fn emitter; snapshot tests green |
| **S4** — 12 Vapor behavior fixtures → canonical-JSON parity harness | harness green | ✅ `fixtures/conformance/` (12 behaviors), `xtask parity` byte-compiles lowering vs committed `expected.ir.json` |

Plus the §2.5 gate: **Oxc vs SWC re-benchmarked on our own workload** —
Oxc 1.86 µs vs SWC 6.11 µs on `counter.tsx` (≈3.3×, confirming the keep-Oxc
decision for structural reasons as well) — `docs/benchmarks/oxc-vs-swc.md`.

## Engineering baselines held from day one

- **TDD**: 132 tests across 26 test binaries; every module landed with its
  tests (insta snapshots for IR/codegen/taffy contracts).
- **clippy pedantic**: `cargo clippy --workspace --all-targets -- -D warnings`
  green; documented allowlist in the root `Cargo.toml` (each with rationale).
- **Benches from day one**: criterion suites per crate — parse/lowering
  (18.2 µs counter lower), canonical IR, codegen (523 µs counter), runtime
  reconcile, patch protocol (apply ~22 ns @1 entry, dispatch hit ~2.1 ns);
  `cargo bench --workspace --no-run` is a CI gate.
- **CI/CD**: `.github/workflows/ci.yml` (fmt, clippy pedantic, tests, bench
  compile, parity, reproducibility, patch-spike on Linux+macOS, bench-swc)
  and `release.yml` (git-cliff changelog → GitHub release) — latest major
  actions as of 2026-11: checkout@v7, setup-rust-toolchain@v2,
  rust-cache@v2, install-action@v2, action-gh-release@v3.
- **git-cliff**: `cliff.toml` conventional-commit config; `CHANGELOG.md`
  generated; release notes automated on `v*` tags.

## Open questions resolved at M0

- **OQ-1 (`quote!` API shape)**: **text output confirmed** — `GeneratedUnit.text`
  is byte-reproducible via `prettyplease` (D-8 gate: `xtask reproducibility`).
- **Oxc vs SWC (§2.5)**: re-measured as a gate, not a citation — keep Oxc.
- **Patch protocol (§2.4)**: jump-table shape proven; see ADR-013.

## Known gaps (deferred, deliberate)

- Array-valued state initializers (`createSignal([1, 2, 3])`) are fail-closed
  (`LIG-E001` array literal in view expressions) — `<For>` fixtures therefore
  wait for M1 admission tiers; list rendering is covered by the `.map`
  fixture (`map-children`) and unit tests.
- Event-object access (`e.currentTarget.value`) is outside the spike subset;
  handlers lower as setter calls / pure builtins only.
- Vue frontend + `vue.vue` fixture legs are M2 (the fixture contract and
  harness already accept them; see `fixtures/README.md`).
- Salsa incremental, digest gating, watch mode, ThinLink patch engine: M3.
