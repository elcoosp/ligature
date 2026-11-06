# ADR-017: M4 — the budget ledger, the no_std proof, and Appendix A

- Status: accepted (M4)
- Deciders: compiler team
- Date: 2026-11-06

## Context

§17.1 M4 closes the three items every prior report deferred: **Budgets**
(§14.2 — the perf budgets were measured from day one but never *enforced*),
the **`no_std` proof** (§10 — `lig-runtime` declares `#![no_std]`, but
nothing compiles it for a target where std does not exist to lean on), and
**Appendix-A docs** (the annotated generated-code walkthrough the
architecture's appendix shape calls for). The milestone adds no new
frontend, IR, or protocol surface — it hardens contracts that already exist.

## Decision

### 1. Budgets are a ledger + a gate, not a claim

- `docs/benchmarks/budgets.toml` is the committed ledger: one `[[budget]]`
  row per workload, `metric` = `time` (µs medians) or `bytes`, an absolute
  `budget` ceiling, and a `note` carrying the reference median the ceiling
  was derived from.
- The ceiling policy is **≥ 5× headroom over the reference median** (the
  patch-protocol rows sit further out — nanosecond operations get 1 µs
  ceilings). The gate is a regression tripwire, not a benchmark: criterion
  remains the precision instrument; the gate answers "did a change blow
  through the contract?" in under a second.
- The gate (`lig budgets`) measures **the same workloads the criterion
  benches drive** — in-process, median over fixed samples (60–800 per row).
  The ledger's names are 1:1 with the gate's measurement registry (the
  rules-sync discipline applied to budgets): an unknown row, a missing row,
  a duplicate, or a non-positive ceiling fails the gate before anything is
  measured.
- **Profile contract**: time budgets are release numbers. CI enforces them
  through `cargo run -p lig-cli --release -- budgets` (a dedicated job); a
  dev-profile run prints its measurements but enforces only the
  profile-independent rows (bytes) and says so on every line it prints —
  the gate never reports a contract as checked when it ran a different
  currency. Bytes rows (the D-8-pinned generated-unit size, 8 192 B ceiling
  against the 7 146 B counter unit) are enforced in every profile.
- The measured workloads: `lower_solid_counter`, `lower_vue_counter`,
  `canonical_json_counter`, `codegen_counter`, `runtime_instantiate`,
  `runtime_update_mask`, `runtime_list_diff_keyed`, `patch_apply_1`,
  `patch_dispatch_hit`, `gen_counter_bytes` — parse (both frontends),
  canonical IR, codegen, the three runtime benches, the two patch-protocol
  benches, and the generated-code size budget. The M4 release medians land
  on the M0 numbers (18.4 µs vs 18.2 µs lower, 563 µs vs 523 µs codegen),
  which is the point: the ledger encodes the numbers the project already
  published in `docs/benchmarks/`.
- `lig doctor` reports the ledger as an environment check (parse + row
  count); a broken ledger is a doctor failure like a broken catalog.

### 2. The no_std proof is three legs, all gated

`lig no-std` proves the §10 claim instead of asserting it:

1. **Structural contract** (no toolchain work): `lig-runtime` declares
   `#![no_std]` and `#![forbid(unsafe_code)]`, declares **zero
   `[dependencies]`** (the gate parses the manifest; dev-dependencies are
   exempt — test harnesses run on the host), and touches `std` only behind
   `#[cfg(test)]`.
2. **Bare-metal builds**: `cargo build -p lig-runtime --target` for
   `thumbv7em-none-eabihf` (Cortex-M4F, the canonical embedded-UI target)
   and `aarch64-unknown-none-softfloat` — targets where neither std nor a
   host allocator exists to hide behind. An rlib build needs no linker or
   `#[panic_handler]`; compiling at all is the proof.
3. **Generated code is host-agnostic**: the gate generates a fresh counter
   unit (the D-8-pinned workload), writes it into a throwaway `#![no_std]`
   scratch crate in the temp dir (outside this workspace; `lig-runtime`
   linked by absolute path), and compiles it for the thumb target. The
   published runtime *and* the code the compiler emits for it both stand on
   `core` + `alloc` alone.

Missing targets are a hard failure with the one-line fix
(`rustup target add …`) — the gate never silently passes where it did not
run. CI gains a dedicated `no-std` job that installs both targets first.
`lig doctor` reports installed targets informationally (the gate is the
enforcement).

### 3. Appendix A is the annotated generated unit

`docs/appendix-a.md` walks the committed `fixtures/generated/counter.rs`
end to end — module hygiene, state slots, template consts, anchor tables,
the `Schedule` literal (`order`/`writes`/`writes_hi`), reaction constants
and the per-arm `run` match, the setter + dispatch glue, the run-once
`build`, and the style consts — quoting the byte-pinned text (D-8 keeps the
quotes honest) and cross-referencing the sections each shape implements
(§7.2, §9, §10, §11.3) plus the M4 additions (the budget row for its size,
the no_std proof it passes). It is the missing "what does the compiler
actually emit and why" document, written against the one artifact every
gate already pins.

### 4. What M4 deliberately does not do

- No new crates, no new external dependencies: the ledger rides the `toml`
  pin already in `lig-cli`; the gate measurements ride workspace path deps
  (`lig-runtime`, `lig-patch` join `lig-cli`'s dependency list — both
  reviewed, zero new third-party surface).
- No budget rows for the SWC comparison (that bench measures a
  *decision*, not a contract — its gate is the ratio, recorded at M0).
- No `cargo vet` in this milestone: the pin table (ADR-012) remains the
  reviewed supply-chain surface; vet onboarding stays a separate decision.

## Consequences

- A perf regression > 5× on any pinned workload now fails CI with the row,
  the measured number, and the provenance note — the same review path as a
  parity or reproducibility drift.
- `lig-runtime` losing its dependency-free / no_std discipline fails CI on
  three independent legs.
- The appendix quotes byte-pinned text: a codegen change that moves the
  unit fails `lig reproducibility` first, and the appendix diff is reviewed
  in the same change (the quotes cannot silently rot).
- The gate suite count rises to 13 (`fixtures gen/verify/parity-gen/
  admission/rules-sync`, `reproducibility`, `patch-spike`, `incremental`,
  `errors-gen`, `budgets`, `no-std`, plus `doctor`'s environment checks).
