# Ligature

A dual-frontend (Solid TSX / Vue SFC → shared canonical View IR → const-scheduled Rust) UI compiler with a subsecond-derived hot-patch protocol. This repository implements the architecture in `ligature-architecture-v1.0.md`.

**Status: M1 milestone complete** — Solid frontend + admission tiers v1 + codegen + source maps + `lig check/build` (see `docs/m0/REPORT.md` and `docs/m1/REPORT.md`).

## Workspace

| Crate | Role |
| --- | --- |
| `lig-core` | Spans, u32 ids, string interning, FNV fingerprints, error-code registry |
| `lig-ir` | View IR v2: templates, fragments, anchors, dynamics, expr forest, canonical JSON |
| `lig-expr` | oxc AST → ExprIR lowering, `isDynamic` port, pure-builtin whitelist |
| `lig-parse-solid` | Solid TSX frontend (dom-expressions passes ported to Rust) |
| `lig-reactivity` | Const topological schedule, reader/writer masks, static cycle deny |
| `lig-style` + `lig-taffy-profile` | 25-property style schema; lightningcss → taffy const-fn emitter |
| `lig-codegen` | `quote!` → `syn::File` → `prettyplease`, byte-reproducible (D-8) |
| `lig-runtime` | `no_std` + alloc slab runtime, `Host` commit stream, keyed LIS reconcile |
| `lig-patch` | S2 spike: jump-table hot-patch protocol, manifest, save→frame budget |
| `lig-sourcemap` | Chained standard-VLQ source maps, IR-node names in `names[]` (§9.4) |
| `lig-admission` | Deny/Lint/Allow ladder, `model!`/`native!` escapes, rule table ↔ fixtures |
| `lig-cli` | The `lig` binary: `check`, `build`, `explain`, `resolve`, `inspect` (§12.1) |

## Gates (all enforced in CI)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings   # pedantic, day one
cargo test --workspace                                   # TDD, day one
cargo bench --workspace --no-run                         # benched, day one
cargo run -p xtask -- parity                             # 16 canonical-IR fixtures
cargo run -p xtask -- admission                          # NFR-5: 22 diagnostic cases
cargo run -p xtask -- rules-sync                         # rule table ↔ fixtures 1:1
cargo run -p xtask -- reproducibility                    # D-8: byte-identical codegen
cargo run -p xtask -- patch-spike                        # S2: save→frame < 1 s
```

Benchmarks (`cargo bench`): parse/lowering, canonical IR, codegen, runtime reconcile, patch protocol. The Oxc-vs-SWC re-benchmark is feature-gated: `cargo bench -p lig-parse-solid --features bench-swc --bench swc_compare`.

## Releases

Conventional Commits → [git-cliff](https://git-cliff.org) (`cliff.toml`) → `CHANGELOG.md` + release notes, automated by `.github/workflows/release.yml` on `v*` tags.

## License

MIT OR Apache-2.0 (see `LICENSE-MIT` / `LICENSE-APACHE`).
