# ADR-015: M3 incremental pipeline — a dependency-free salsa-style session engine

- Status: accepted (M3)
- Deciders: compiler team
- Date: 2026-11-06

## Context

§17.1 M3 calls for **Salsa incremental compilation, digest gating (D-9), and
watch mode**. The architecture's language is salsa's: inputs, queries,
memoized recomputation guarded by input digests, revision counters, and
cycle recovery (LIG-E020 — "Salsa cycle recovery surfaces this as a Deny
diagnostic, never a hang", §11.1). The pipeline to make incremental is the
M0–M2 chain: lower → admit → build-order → codegen → emit, with FNV-1a 64
digests already the project's hashing contract (§5.6, D-8/D-9).

## Decision

1. **A dedicated engine crate, `lig-incremental`**, implements the
   salsa *semantics* without the salsa runtime: a `Session` holds source
   inputs (path → text + FNV digest), a revision counter, and per-path memo
   entries recording the dependency digests each query ran against. A query
   re-executes **iff** a recorded digest (or the admission profile) changed;
   otherwise the memo serves the result untouched. No-op edits
   (`set_source` with identical bytes) never bump the revision.
2. **Digest-gated emission (D-9)** lives beside the engine (`emit`): each
   pass compares per-unit content digests (source, generated Rust, IR JSON,
   merged map) against `dist/build-state.json` and writes only what changed,
   so both `lig build` and every `lig watch` tick keep byte-identical
   artifacts on disk. The state file is a cache, never a lie: a missing
   artifact forces a re-emit.
3. **Watch mode is a poll shell over the engine** (`lig watch`): a 300 ms
   scan hashes the source set, feeds edits to `set_source`, and runs one
   digest-gated pass per change. Polling with content digests (not mtime
   events) keeps the dependency surface empty (ADR-012's supply-chain
   minimalism) and is correct across filesystems and editors.
4. **Cycle recovery**: `lig-reactivity::component_build_order` toposorts the
   component-use graph (§11.1); a cycle returns a deterministic path that
   the engine lowers to the **LIG-E020** Deny diagnostic (now active — the
   code frame labels the first use site on the cycle). `lig check`, `lig
   build`, and the watch loop all deny before codegen; a denied watch pass
   never touches `dist` (the last good output stays live).
5. **Why not the `salsa` crate**: the runtime would invert the M0–M2
   pipeline APIs around an interning database, add a dependency subtree to a
   workspace whose pin table is reviewed like code (ADR-012), and buy
   durability/weaving features the compiler does not use at M3. The engine
   is ~400 lines, dependency-free, and its memo/revision/cycle-recovery
   semantics are pinned by unit tests and the `lig incremental` gate.

## Rejected alternatives

- **The `salsa` crate as-is**: dependency + API-inversion cost against
  benefits (durable caching, parallel query weaving) outside M3 scope.
- **mtime-only watching**: editors and containers produce mtime jitter and
  equal-mtime writes; content digests are the contract the rest of the
  project already speaks.
- **Emit-always with content-equal writes**: correct but wasteful — every
  pass would rewrite `dist` and invalidate downstream build systems; D-9
  exists to make "nothing changed" provably free.

## Consequences

- `lig build` reports `kept` vs `re-emitted` per pass; the second build of
  an unchanged tree performs no I/O on unit artifacts (state file aside).
- The `lig incremental` CI gate pins the D-9 contract end to end (write,
  memo-keep, state-keep, edit, byte-identical revert, LIG-E020 deny).
- Cross-process reuse rides `build-state.json`; in-process reuse rides the
  memo table. Both are digest-keyed and re-verified by the gate.
- Per-arm source-map attribution (M3) landed with the same re-emit work:
  expressions map to their own reaction's generated region.
- Spill + per-bit dispatch refinement (§7.2) raised the reaction cap from 64
  to 128 with exact per-bit init masks (`Schedule.writes_hi`).
