# ADR-013: M0 hot-patch protocol shape (jump table, never-dlclose, manifest)

- Status: accepted (M0, S2 spike)
- Date: 2026-11-05

## Context

§2.4 / §11.3 require a subsecond-derived hot-patch path with a **< 1 s
save→frame** budget, without text/GOT patching, and with the hard-won rule
that patch dylibs are **never `dlclose`d** (stale frames may still execute
code inside them). M0 spike S2 had to prove the protocol shape on a dummy
host before M3's ThinLink machinery is built on top.

## Decision

1. **One global jump table**: `AtomicPtr<JumpTable>` (old runtime address →
   new runtime address). Safepoint calls (`lig_patch::hot_call`) look up the
   current function's address: hit → call the patched address; miss → call
   the original. The table pointer is swapped atomically (`AcqRel`); a
   patched process never mutates a table in place.
2. **Symbol-name matching** (subsecond thunk semantics): `apply_patch`
   resolves requested symbols in the fresh dylib via `libloading`, and the
   fat build's addresses via `dlsym(RTLD_DEFAULT, …)` — hosts therefore link
   with `-rdynamic` on Linux (macOS exports by default). `main`'s runtime
   address is the **ASLR reference** for rebasing.
3. **Never-dlclose, never-free**: both the loaded `Library` and every
   committed table are intentionally leaked. Generation `n`'s table stays
   valid for stale frames that unwind to it.
4. **`patch.toml` manifest** (§11.3 step 3): dylib path, build id, ASLR
   reference, symbol → file-relative address map. File-relative addresses are
   computed with `dladdr` at manifest-write time so a fresh process can
   rebase without guessing its own load bias. A minimal TOML subset is
   serialized/parsed inside `lig-patch` (no `toml` dependency at M0).
5. **The only unsafe module** (§15): `lig-patch` overrides the workspace
   `unsafe_code = "deny"` lint locally; every `unsafe` block carries a
   SAFETY comment referencing the leaked-table / same-signature invariants.
6. **Budget enforcement is a CI gate**, not a claim: `xtask patch-spike`
   rebuilds the tip cdylib, applies it on the dummy host, asserts routed
   frames and the manifest round-trip, and fails if the wall clock exceeds
   1 s (measured ~125–140 ms; see `docs/benchmarks/save-to-frame.md`).

## Rejected alternatives

- **GOT/PLT patching**: process-wide, racy with other threads, and fights
  full-relro; the jump table gives per-safepoint control with nanosecond cost.
- **`dlclose` of superseded dylibs**: the §2.4 lesson stands — stale frames
  returning into freed code is UB; leak instead.
- **Storing runtime→runtime addresses in the manifest**: useless after a
  process restart; file-relative is the portable currency.

## Consequences

- M3's patch engine (rustc replay + stub thunks, real app dylibs) implements
  against exactly this surface: `apply_patch` / `hot_call` / `PatchManifest`.
- New-symbol registration (functions that did not exist in the fat build) is
  mapped to its own address by the spike; subsecond's full thunk story lands
  with M3.
- Windows is out of scope for the M0 gate (unix `dlopen` protocol); the CI
  gate matrix is Linux + macOS.
