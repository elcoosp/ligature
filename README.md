# Ligature

A dual-frontend (Solid TSX / Vue SFC → shared canonical View IR → const-scheduled Rust) UI compiler with a subsecond-derived hot-patch protocol. This repository implements the architecture in `ligature-architecture-v1.0.md`.

**Status: M2 milestone complete** — dual frontends (Solid TSX + Vue SFC) with two-leg fixture parity, list/array tier completion, and the CI gates folded into the `lig` CLI (see `docs/m0/REPORT.md`, `docs/m1/REPORT.md`, `docs/m2/REPORT.md`).

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
| `lig-cli` | The `lig` binary: `check`, `build`, `explain`, `resolve`, `inspect` **+ the CI gates** (§12.1, §13) |

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
cargo run -p lig-cli -- patch-spike                      # S2: save→frame < 1 s
```

Fixture regenerators (review diffs like code): `lig fixtures gen` and
`lig fixtures parity-gen`; the error-catalog regenerator is `lig errors-gen`.

Benchmarks (`cargo bench`): parse/lowering (both frontends), canonical IR, codegen, runtime reconcile, patch protocol. The Oxc-vs-SWC re-benchmark is feature-gated: `cargo bench -p lig-parse-solid --features bench-swc --bench swc_compare`.

## Releases

Conventional Commits → [git-cliff](https://git-cliff.org) (`cliff.toml`) → `CHANGELOG.md` + release notes, automated by `.github/workflows/release.yml` on `v*` tags.

## License

MIT OR Apache-2.0 (see `LICENSE-MIT` / `LICENSE-APACHE`).
