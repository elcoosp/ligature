# S2 hot-patch spike — measured numbers (§11.3, §17)

The S2 acceptance criterion is **save→frame < 1 s** on Linux and macOS:
the wall-clock time from "the tip source was saved" to "the next frame runs
patched code on the dummy host".

## What the clock covers (`xtask patch-spike`)

1. rebuild `tests/patch-tip` as a **cdylib** with v1 bodies
   (`LIG_TIP_LABEL=v1`, `LIG_TIP_COUNT=42`) — a fresh `target/` per run so
   the `option_env!`-selected bodies really recompile;
2. run `tests/dummy-host`, which applies the dylib through `lig-patch`
   (`dlopen` → symbol resolve → rebase → **atomic jump-table commit**) and
   asserts the routed frames.

The host binary itself is the "fat build" and is compiled **outside** the
clock — in the real flow it is the already-running app.

## Result (2026-11-05, Linux x86_64, dev-profile dylib)

| Stage | Measured |
| --- | --- |
| save→frame total (rebuild + relink + load + apply + first frame) | **~125–140 ms** |
| in-process `apply_patch` (load + resolve + rebase + commit) | ~90–165 µs |
| first routed frame (`hot_call` through the table) | ~1 µs |

Budget: **1000 ms** — PASS with ~7× headroom, and the dominant term is the
tip-crate rustc invocation, not the patch protocol.

## Protocol cost decomposition (`cargo bench -p lig-patch`)

| Bench | Median |
| --- | --- |
| `patch-apply/1` (jump-table build + atomic commit, 1 entry) | ~22 ns |
| `patch-apply/16` | ~33 ns |
| `patch-dispatch/miss` (safepoint, un-patched) | ~0.4 ns |
| `patch-dispatch/hit` (safepoint routed through table) | ~2.1 ns |

The runtime protocol is nanoseconds; the budget is owned by compilation.
This validates the §2.4 design bet: the jump table keeps the fat/thin
boundary cheap and the safepoint overhead negligible.

## Runner notes

- Numbers are from the M0 CI-reference runner (dev-profile tip dylib,
  debug host); re-run locally for your hardware.
- The gate itself lives in `xtask patch-spike` and runs on Linux + macOS in
  CI (`.github/workflows/ci.yml`). Windows is skipped at M0 (unix-only
  `dlopen` protocol).
- The dx ThinLink machinery (rustc replay + stub thunks, real app dylibs) is
  M3 scope; this spike proves the protocol shape and the budget.
