# M3 milestone report — incremental pipeline, digest gating, watch mode, ThinLink

Date: 2026-11-06 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (§17.1 M3)

Salsa incremental, digest gating, watch mode, ThinLink patch engine — the
four deferred M2 items, plus every in-repo "M3 item" note.

| Exit criterion | Result |
| --- | --- |
| **Salsa incremental** | ✅ `lig-incremental` — a dependency-free salsa-style session engine (ADR-015): source inputs + per-path memo entries validated by recorded dependency digests, revision counters (no-op edits never bump), and the pipeline queries `lowered` / `generated` keyed on (source digest, admission profile). The engine is the shared substrate of `lig check`, `lig build`, and `lig watch`. |
| **Digest gating (D-9)** | ✅ digest-gated re-emission in `emit`: a pass writes only units whose source/generated/IR/map digests changed against `dist/build-state.json` (a missing artifact forces a re-emit — the state is a cache, never a lie). `lig build` reports `N kept, M re-emitted`; the `lig incremental` gate pins the contract end to end (write → memo-keep → cold-session state-keep → edit → byte-identical revert → LIG-E020 deny with `dist` untouched). |
| **Watch mode** | ✅ `lig watch` — a digest-polled shell over the session (300 ms default interval, `--interval-ms`, `--once` for CI): scans the source set, feeds edits to the session, runs one gated pass per change, prunes artifacts of removed sources, and never touches `dist` on a denied pass (BR-1: the last good output stays live). |
| **ThinLink patch engine** | ✅ `lig-patch::session` (ADR-016) — `PatchSession` chains generations (build ids 1, 2, …), records per-symbol classification (`Existing` / `StubThunk` / `New`), and proves the never-dlclose invariant in-process: a generation-1 committed address captured before the generation-2 apply still executes the generation-1 body after it. **Stub thunks** land: the fat host exports `patchable_frame_extra` as a placeholder and generation 2 fills it. The spike gate is now two generations against one running host. |
| **LIG-E020 active** | ✅ `lig-reactivity::component_build_order` toposorts the component-use graph (§11.1); cycles surface as the LIG-E020 Deny with the deterministic cycle path (`A → B → A`) and a code frame on the first use site — never a hang or a runtime stack overflow. M2 silently admitted `A → B → A` and emitted a build that would recurse forever at first mount; now `lig check`/`build` deny with exit 1 and zero output. |
| **Per-arm source-map attribution** | ✅ `lig-sourcemap` maps every expression to the generated region of the reaction that owns it (exact body match, else the smallest containing body span); unowned expressions keep the first-reaction fallback. Pinned by a new test: two arms, two distinct regions. |
| **Spill + per-bit dispatch refinement** | ✅ the §7.2 spill path is exact: `Schedule.writes_hi` carries per-reaction spill write masks (ids 64..128, bits are id − 64), setters/memos emit `mark_spill_word` alongside the low-word `mark_mask`, build-time init masks are exact per bit (no more mark-everything `u64::MAX`), and `flush` dispatches spill reactions per bit. The reaction cap rises 64 → 128 with a fail-closed error past it. |

## Engineering baselines held

- **TDD**: 224 tests across 39 test binaries (M2: 199/37). Every new module
  landed with its tests: session memo/invalidation/cycle unit tests, spill
  dispatch tests in the runtime, exact-mask codegen tests, per-arm
  attribution, the two-generation host assertions, and six new CLI
  integration tests (`watch --once`, D-9 keeps, LIG-E020 denial,
  `incremental` gate, `doctor`, embedded-catalog `explain`).
- **clippy pedantic**: `cargo clippy --workspace --all-targets -- -D
  warnings` green, benches included; no new allowlist rows.
- **Benches from day one**: `cargo bench --workspace --no-run` stays a CI
  gate; the runtime bench compiles against the extended `Schedule`.
- **CI/CD**: `.github/workflows/ci.yml` gains the `lig incremental` gate;
  the patch-spike job keeps its Linux+macOS matrix (now two generations).
- **git-cliff**: changelog continues on conventional commits (`v0.1.0-m3`).

## Full gate run at delivery

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ |
| `cargo test --workspace` | ✅ 224 tests / 39 binaries |
| `cargo bench --workspace --no-run` | ✅ |
| `lig fixtures verify` | ✅ 18/18 fixtures, both legs |
| `lig fixtures admission` | ✅ 22/22 cases (NFR-5) |
| `lig fixtures rules-sync` | ✅ 11 rules 1:1 with fixtures |
| `lig reproducibility` | ✅ byte-identical (7 146 B counter unit — +52 B for the `writes_hi` row) |
| `lig incremental` | ✅ 6 passes green (D-9 + LIG-E020) |
| `lig errors-gen` | ✅ 15 codes + 15 embedded docs, regeneration is a no-op |
| `lig patch-spike` | ✅ two generations, **276 ms** total, < 1 s per save (gen1 apply 159 µs, routed frame ~1 µs) |

## Notes and decisions

- **Why not the `salsa` crate** (ADR-015): the runtime would invert the
  M0–M2 pipeline APIs around an interning database and add a dependency
  subtree; the engine implements the memo/revision/cycle-recovery semantics
  over the project's existing FNV digest contract in ~400 dependency-free
  lines, pinned by tests and the `lig incremental` gate.
- **Embedded catalog** (the `lig doctor` hardening): `lig errors-gen` now
  also generates `lig-core::error_code::catalog_md` — the 15 catalog
  Markdown bodies ship inside the binary, so `lig explain` works far from
  the workspace (verified from `/tmp` in the CLI tests). `lig doctor`
  reports catalog coverage, frontends, patch-protocol availability, and
  out-dir writability.
- **Reproducibility size moved** 7 094 → 7 146 B: the generated `Schedule`
  literal now carries `writes_hi: &[]`. Contract change, reviewed like code
  (snapshot diff is the one field).
- **The compile-clean crate** regenerated for the same reason (18 gen
  files, one added line each).

## Known gaps (deferred, deliberate)

- Keyed `For` (`key` expression → LIS diff): the schema exists, both
  frontends emit index-diff lists (unchanged from M2).
- Named slots and `<slot>` fallback content; `v-model`; multi-statement
  effect bodies: the IR models the default outlet only (v2 schema).
- A `lig patch` developer driver over real app dylibs (app-crate scaffold +
  dev channel): the engine surface (`PatchSession` / `apply_patch` /
  `hot_call` / `PatchManifest`) is what that work wires into; the M3 gate
  proves the protocol on the reference host.
- Budgets + `no_std` proof + Appendix-A docs: M4.
