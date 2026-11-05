# Ligature

A dual-frontend (Solid TSX / Vue SFC → shared canonical View IR → const-scheduled Rust) UI compiler with a subsecond-derived hot-patch protocol. This repository implements the architecture in `ligature-architecture-v1.0.md`.

**Status: M3 milestone complete** — the incremental pipeline: a salsa-style session engine (`lig-incremental`) with digest-gated re-emission (D-9), `lig watch`, build-order cycle recovery (LIG-E020), per-arm source-map attribution, the 128-reaction per-bit dispatch refinement, and the two-generation ThinLink patch engine with stub thunks (see `docs/m0/REPORT.md`, `docs/m1/REPORT.md`, `docs/m2/REPORT.md`, `docs/m3/REPORT.md`).

## Workspace

| Crate | Role |
| --- | --- |
| `lig-core` | Spans, u32 ids, string interning, FNV fingerprints, error-code registry |
| `lig-ir` | View IR v2: templates, fragments, anchors, dynamics, expr forest, canonical JSON |
| `lig-expr` | oxc AST → ExprIR lowering, `isDynamic` port, pure-builtin whitelist, small-lattice typer |
| `lig-parse-solid` | Solid TSX frontend (dom-expressions passes ported to Rust) |
| `lig-parse-vue` | Vue SFC frontend (`<script setup>` + `<template>` → the same canonical IR, ADR-014) |
| `lig-reactivity` | Const topological schedule, reader/writer masks, static cycle deny |
| `lig-style` + `lig-taffy-profile` | 25-property style schema; lightningcss → taffy const-fn emitter |
| `lig-codegen` | `quote!` → `syn::File` → `prettyplease`, byte-reproducible (D-8) |
| `lig-runtime` | `no_std` + alloc slab runtime, `Host` commit stream, keyed LIS reconcile |
| `lig-patch` | S2 spike: jump-table hot-patch protocol, manifest, save→frame budget |
| `lig-sourcemap` | Chained standard-VLQ source maps, IR-node names in `names[]` (§9.4) |
| `lig-admission` | Deny/Lint/Allow ladder, `model!`/`native!` escapes, rule table ↔ fixtures |
| `lig-incremental` | Salsa-style sessions: digest-gated memos (D-9), LIG-E020 cycle recovery, digest-gated emission, watch passes |
| `lig-cli` | The `lig` binary: `check`, `build`, `watch`, `explain`, `resolve`, `inspect`, `doctor` **+ the CI gates** (§12.1, §13) |

## Gates (all enforced in CI — they ship inside the `lig` CLI since M2)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings   # pedantic, day one
cargo test --workspace                                   # TDD, day one
cargo bench --workspace --no-run                         # benched, day one
cargo run -p lig-cli -- fixtures verify                  # 18 fixtures × both frontend legs (§4)
cargo run -p lig-cli -- fixtures admission               # NFR-5: 22 diagnostic cases
cargo run -p lig-cli -- fixtures rules-sync              # rule table ↔ fixtures 1:1
cargo run -p lig-cli -- reproducibility                  # D-8: byte-identical codegen
cargo run -p lig-cli -- incremental                      # D-9: digest-gated re-emit + LIG-E020 (M3)
cargo run -p lig-cli -- patch-spike                      # S2/M3: two generations, < 1 s per save
```

Fixture regenerators (review diffs like code): `lig fixtures gen` and
`lig fixtures parity-gen`; the error-catalog regenerator is `lig errors-gen`
(registry **and** the embedded catalog `lig explain` serves offline).

Incremental development: `lig build` re-emits only digest-changed units
(D-9, `dist/build-state.json`), `lig watch` re-runs digest-gated passes on
save, and `lig doctor` checks the environment (embedded catalog, patch
protocol, out-dir).

Benchmarks (`cargo bench`): parse/lowering (both frontends), canonical IR, codegen, runtime reconcile, patch protocol. The Oxc-vs-SWC re-benchmark is feature-gated: `cargo bench -p lig-parse-solid --features bench-swc --bench swc_compare`.

## Releases

Conventional Commits → [git-cliff](https://git-cliff.org) (`cliff.toml`) → `CHANGELOG.md` + release notes, automated by `.github/workflows/release.yml` on `v*` tags.

## License

MIT OR Apache-2.0 (see `LICENSE-MIT` / `LICENSE-APACHE`).
