# ADR-012: MSRV policy and dependency pins

- Status: accepted (M0)
- Deciders: compiler team
- Date: 2026-11-05

## Context

`ligature-architecture-v1.0.md` §D-12 requires a stated MSRV policy and §14.2
requires verified dependency pins ("re-verify at M0"). The draft's pins
(Oxc `0.155`, syn `2.x`, prettyplease `0.2.x`) were document-time estimates.

## Decision

1. **MSRV policy N-2**: support the stable toolchain and the two releases
   before it. At M0 the developer/CI toolchain is **1.99.1**
   (`rust-toolchain.toml`); the declared **`rust-version` is 1.97** — forced
   from below by `oxc 0.157.0` (`rust-version = 1.97.0`), not chosen.
2. **Pins verified on crates.io at M0 bootstrap (2026-11-05)**, recorded in
   the root `Cargo.toml` and `Cargo.lock`:

   | Crate | Pin | Role |
   | --- | --- | --- |
   | oxc_* | 0.157.0 | parse substrate |
   | syn / quote / prettyplease | 3.0.9 / 1.0.47 / 0.3.0 | codegen |
   | serde / serde_json | 1.0.229 / 1.0.154 | canonical IR serialization |
   | miette | 7.6.0 | diagnostics rendering |
   | lightningcss | =1.0.0-alpha.72 (exact) | style lowering (MPL-2.0 boundary) |
   | taffy | 0.15.1 | layout consts |
   | libloading / libc | 0.9.0 / 0.2.191 | patch protocol |
   | swc_common / swc_ecma_ast / swc_ecma_parser | 27.0.0 / 30.0.1 / 47.0.0 | M0 re-bench only, feature `bench-swc` |
   | criterion / insta | 0.8.2 / 1.49.0 | benches / snapshots |

3. **lightningcss is pinned exactly** (`=`): it is an alpha with breaking
   minor bumps; it sits behind an MPL-2.0 dependency boundary (§15) and must
   not float.
4. **SWC is never a compiler dependency**: feature-gated `bench-swc` in
   `lig-parse-solid`, used only by the M0 re-benchmark (§2.5).

## Consequences

- Bumping any pin is an intentional change: update the table, run the full
  gate suite (`cargo test --workspace`, clippy pedantic, benches compile,
  xtask gates) and re-record `docs/benchmarks/*` numbers.
- The MSRV may only rise with a new `oxc` minor; CI installs the toolchain
  from `rust-toolchain.toml` so local and CI builds are identical.
- `cargo vet` onboarding is M1 scope (§14.2); until then the pin table above
  is the reviewed supply-chain surface.
