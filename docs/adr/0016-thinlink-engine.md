# ADR-016: M3 ThinLink patch engine — chained generations, stub thunks, never-dlclose proof

- Status: accepted (M3)
- Deciders: compiler team
- Date: 2026-11-06

## Context

ADR-013 froze the M0 hot-patch protocol shape (jump table, symbol-name
matching, never-dlclose, `patch.toml` manifest) and deferred the "dx
ThinLink machinery" — rustc replay, stub thunks, generation chaining over
real app dylibs — to M3 (§2.4, §11.3). The M0 spike proved one
save→frame hop (~125–140 ms) against a dummy host with two patchable
symbols.

## Decision

1. **`PatchSession` (in `lig-patch`, safe code)** owns the engine surface:
   every `apply` loads the fresh dylib, resolves the requested symbols,
   commits the next jump table atomically, bumps the **generation** (build
   id), and chains a `PatchManifest`. Sessions expose the manifest history
   and per-symbol classification (`Existing` / `StubThunk` / `New`).
2. **Stub thunks** (subsecond's new-symbol story): a fat build that wants to
   receive functions it does not have yet exports a placeholder under the
   future name; a later patch generation routes it exactly like an existing
   symbol and the stub body never runs again. The spike's third symbol
   (`patchable_frame_extra`) is the fat host's stub, filled by generation 2.
   Symbols absent from the fat build remain **new**: they resolve to their
   patch address directly and stay callable from the patch dylib.
3. **Never-dlclose is now a proven invariant, not a comment**: before
   applying generation 2 the host captures the committed runtime address of
   a generation-1 routed symbol (`PatchSession::captured_address`); after
   the generation-2 commit it calls that stale address directly
   (`call_captured`) and gets the generation-1 body — the superseded dylib
   stayed mapped and executable, which is exactly what a stale unwinding
   frame relies on (§2.4).
4. **Two-generation spike gate**: `lig patch-spike` builds the tip crate
   twice (v1, then v2 — each into a staged, generation-specific artifact),
   runs one dummy-host process through both applications, asserts routed
   bodies (v1/42, v2/43, stub→7), the chained manifests (build ids 1, 2),
   the stub classification, the stale-frame proof, and the budget: **<
   1 s per save** (each generation's build + apply + first frame) and < 1 s
   total. The M0 numbers carried over (measured at M3 delivery: ~270 ms
   total on the Linux runner, apply ~140 µs, routed frame ~1 µs).
5. **Rustc replay** remains exactly the spike's build step: "save" is a tip
   edit, and the replay is the per-generation cdylib rebuild of the changed
   code. At M3 the replay is multi-generation and digest-disciplined on the
   compiler side (the `lig build` state file); driving a *real* app dylib
   end to end (app crate scaffold + `lig patch` driver command) stays out
   of scope — the engine surface (`PatchSession` over `apply_patch` /
   `hot_call` / `PatchManifest`) is what the dx work wires into.

## Rejected alternatives

- **dlclose of superseded generations**: §2.4's UB lesson — stale frames
  return into freed code; the session leaks every generation instead.
- **In-place jump-table mutation**: ADR-013 already rejects it; sessions
  only ever swap the table pointer atomically (`AcqRel`).
- **Manifest-per-process side channels**: the chained `PatchManifest`
  history in the session is the single source of truth for tooling (dev
  channel, pid/build-id filtering come later).

## Consequences

- The CI gate matrix (Linux + macOS) now proves the *full* M3 protocol
  story, not just one hop: chaining, stubs, stale-frame safety, budgets.
- `dummy-host` is the reference host integration: three exported symbols
  (one a stub thunk), session-driven applies, and the machine-readable
  timing report the gate parses.
- Windows remains out of scope for the dlopen protocol (unchanged from M0).
