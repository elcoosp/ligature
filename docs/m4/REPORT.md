# M4 milestone report — budget ledger, no_std proof, Appendix A

Date: 2026-11-06 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (§17.1 M4)

Budgets, the `no_std` proof, and the Appendix-A docs — the three items every
prior report deferred. No new frontend, IR, or protocol surface: this
milestone hardens contracts the project already had, in the house style —
gated, not claimed (ADR-017).

| Exit criterion | Result |
| --- | --- |
| **Budgets (§14.2)** | ✅ `lig budgets` + the committed ledger `docs/benchmarks/budgets.toml` — one absolute ceiling per bench workload (parse both frontends, canonical IR, codegen, the three runtime benches, the two patch-protocol benches, the generated-unit size), each ≥ 5× its reference median with the provenance in the row's note. The gate re-measures the bench workloads in-process (medians over 60–800 samples, sub-second total), keeps the ledger 1:1 with its measurement registry (rules-sync discipline: unknown/missing/duplicate/non-positive rows fail before measuring), and fails CI on any violation. **Profile contract:** time budgets are release numbers — CI enforces via `cargo run -p lig-cli --release -- budgets`; a dev run prints measurements and enforces only the profile-independent bytes rows, and says so on every line. The M4 release medians land on the M0 numbers (lower 18.5 µs vs 18.2, codegen 577 µs vs 523) — the ledger encodes what `docs/benchmarks/` already published. |
| **`no_std` proof (§10)** | ✅ `lig no-std` — three legs: (1) the structural contract over the committed crate (`#![no_std]`, `#![forbid(unsafe_code)]`, **zero `[dependencies]`** parsed from the manifest, `std` only behind `#[cfg(test)]`); (2) bare-metal builds — `cargo build -p lig-runtime --target thumbv7em-none-eabihf` and `--target aarch64-unknown-none-softfloat`, targets where std and a host allocator do not exist; (3) generated code is host-agnostic — a fresh counter unit compiles inside a `#![no_std]` scratch crate (temp dir, `lig-runtime` by absolute path) for the thumb target. Missing targets are a hard failure with the `rustup target add` fix — the gate never passes where it did not run. |
| **Appendix-A docs** | ✅ `docs/appendix-a.md` — the annotated generated unit: the committed `fixtures/generated/counter.rs` walked top to bottom (hygiene, state slots, template consts, anchor tables, the `Schedule` literal, reaction ids, setter + dispatch glue, run-once `build`, the `Reactions` match, style consts), quoting the byte-pinned text (elisions marked), cross-referencing the section each shape implements (§7.2, §9, §10, §11.3) and the M4 additions (its budget row, the no_std proof it passes). D-8 keeps the quotes honest: a codegen change fails `lig reproducibility` first and the appendix diff is reviewed in the same change. |
| **Doctor hardening** | ✅ `lig doctor` gains two environment checks: the budget ledger (parse + row count — a broken ledger is a doctor failure, like a broken catalog) and the no_std proof targets (installed count + the fix hint; informational — the gate enforces). |

## Engineering baselines held

- **TDD**: 235 tests across 39 test binaries (M3: 224/39). The ledger gate
  landed with unit tests (sync contract, enforcement decision incl. the
  dev/release profile split, median sanity, jump-table workload shape, the
  pinned byte band) and CLI integration tests (the budgets table, and the
  no-std gate's both-environments contract: prove when targets are
  installed, demand `rustup target add` when they are not).
- **clippy pedantic**: `cargo clippy --workspace --all-targets -- -D
  warnings` green, benches included; no new allowlist rows.
- **Benches from day one**: `cargo bench --workspace --no-run` stays a CI
  gate; the budget workloads deliberately mirror the bench suite (§14.2's
  rows), so a bench added without a budget row — or a ledger row without a
  bench — fails the sync check.
- **CI/CD**: `.github/workflows/ci.yml` gains two jobs — `budgets` (the
  release-profile ledger enforcement) and `no-std` (installs
  `thumbv7em-none-eabihf` + `aarch64-unknown-none-softfloat`, runs the
  proof gate). The patch-spike matrix is unchanged.
- **git-cliff**: changelog continues on conventional commits (`v0.1.0-m4`).
- **Zero new external dependencies**: the ledger rides the `toml` pin
  already in `lig-cli`; the gate measurements ride workspace path deps
  (`lig-runtime`, `lig-patch` join `lig-cli`'s dependency list). `cargo vet`
  onboarding (§14.2/ADR-012) remains a separate, deferred decision — the
  pin table is still the reviewed supply-chain surface.

## Full gate run at delivery

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ |
| `cargo test --workspace` | ✅ 235 tests / 39 binaries |
| `cargo bench --workspace --no-run` | ✅ |
| `lig fixtures verify` | ✅ 18/18 fixtures, both legs |
| `lig fixtures admission` | ✅ 22/22 cases (NFR-5) |
| `lig fixtures rules-sync` | ✅ 11 rules 1:1 with fixtures |
| `lig reproducibility` | ✅ byte-identical (7 146 B counter unit, unchanged) |
| `lig incremental` | ✅ 6 passes green (D-9 + LIG-E020) |
| `lig errors-gen` | ✅ 15 codes + 15 embedded docs, regeneration is a no-op |
| `lig patch-spike` | ✅ two generations, **256 ms** total, < 1 s per save |
| `lig budgets` (release) | ✅ all 10 rows within budget — see the table |
| `lig no-std` | ✅ proof green (2 bare-metal targets + the generated unit) |

The M4 budget table at delivery (release medians, this runner):

| Workload | Measured | Ceiling | Headroom |
| --- | --- | --- | --- |
| `lower_solid_counter` | 18.49 µs | 100 µs | 5.4× |
| `lower_vue_counter` | 22.13 µs | 120 µs | 5.4× |
| `canonical_json_counter` | 5.96 µs | 60 µs | 10.1× |
| `codegen_counter` | 576.56 µs | 3 000 µs | 5.2× |
| `runtime_instantiate` | 0.28 µs | 5 µs | 17.5× |
| `runtime_update_mask` | 0.28 µs | 5 µs | 17.5× |
| `runtime_list_diff_keyed` | 2.61 µs | 25 µs | 9.6× |
| `patch_apply_1` | 0.06 µs | 1 µs | 15.9× |
| `patch_dispatch_hit` | 0.03 µs | 1 µs | 40.0× |
| `gen_counter_bytes` | 7 146 B | 8 192 B | 1.1× |

## Notes and decisions

- **Why a ledger + in-process medians instead of gating criterion**: the
  gate must never be the slow part (the M2 gate-migration rationale).
  Criterion stays the precision instrument; the gate answers the contract
  question — "did a change blow through the ceiling?" — in under a second,
  on the same workloads. Ceilings at ≥ 5× the reference median absorb
  runner variance while still catching order-of-magnitude regressions
  (an accidental `format!` on the hot path, a `String` in the flush loop,
  a lost equality guard).
- **Why the dev profile measures but does not enforce time rows**: dev and
  release timings are different currencies (the same gate is ~14× slower
  on `lower_solid_counter` in dev); enforcing release ceilings in a dev
  run would make the gate lie-by-default-fail for every contributor who
  has not read the calibration. The rule the gate prints with its results:
  release enforces; dev reports; bytes (D-8's currency) enforce everywhere.
- **Why the scratch crate for the generated-unit proof**: a `#![no_std]`
  lib that `include!`s a fresh unit is the cheapest honest form — no
  workspace surgery (it lives in the temp dir, `lig-runtime` by absolute
  path), no committed scaffolding, and it regenerates per run like every
  other gate artifact. The empty `[workspace]` table detaches it; the
  include path is escaped into a Rust string literal.
- **Appendix A is an artifact, not prose**: it quotes the byte-pinned unit
  and cross-references the gates that pin it, so it participates in the
  same review flow as code. The initial draft surfaced two real
  misunderstandings (the `READS_*` constants are the per-state reader
  masks the `writes` cascade derives from — not flush inputs; the init
  mask marks every non-event-dispatched reaction) — exactly the failure
  mode the quotes-are-pinned discipline exists to catch.
- **The lig-cli dependency growth is reviewed surface**: `lig-runtime` and
  `lig-patch` join as workspace path deps so the budget gate can measure
  the workloads it polices. Both are already gate-covered crates; neither
  adds a third-party edge (ADR-012's pin table is untouched).

## Known gaps (deferred, deliberate)

- `cargo vet` (§14.2 supply-chain onboarding, called out in ADR-012):
  still deferred — the pin table remains the reviewed surface.
- Keyed `For` (`key` expression → LIS diff): the schema exists, both
  frontends emit index-diff lists (unchanged from M2).
- Named slots and `<slot>` fallback content; `v-model`; multi-statement
  effect bodies: the IR models the default outlet only (v2 schema).
- A `lig patch` developer driver over real app dylibs (app-crate scaffold +
  dev channel): the engine surface (`PatchSession` / `apply_patch` /
  `hot_call` / `PatchManifest`) is what that work wires into.
- The budget ledger's patch rows measure the protocol's *runtime* half;
  the end-to-end save→frame budget stays with `lig patch-spike` (its own
  gate, its own 1 s ceiling).
