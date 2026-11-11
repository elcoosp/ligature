# M20 milestone report — the component-mount seam

Date: 2026-11-12 · Toolchain: 1.99.1 (MSRV 1.97, ADR-012) · Runner: Linux x86_64

## Scope delivered (the runtime half's lifecycle front)

M19 closed the compile half's seams (the stage observer, the one-call
fail-closed emission) and named the runtime half as its open gap: a
consumer implements the 15-method `Host` trait and then drives the
generated unit through free functions and unowned conventions — call
`build` yourself, import the free `flush` after every setter batch, and
discover `Fragment::teardown` on your own or leak the surface. M20 makes
the runtime half the same one-call story:

- **The component-unit seam** (`lig_runtime::mount::ComponentUnit`):
  one object-safe trait, `Reactions` supertrait (what generated code
  already implements) plus one accessor naming the root fragment as the
  teardown surface — the three-line consumer impl lives next to the unit
  it names (orphan rule enforced; `compile-clean` carries the demo impl
  for `Counter`).
- **The mount front** (`Mounted<C: ComponentUnit>`): `mount(host, build)`
  pairs the generated build with the handle; `state()` keeps setters and
  the generated `fire_N` dispatch directly reachable; `update(host, f)`
  is the save-tick (setter batch, then the const-schedule flush, one
  call); `flush(host)` is the bare tick; `dispose(host)` is the total
  recursive LIFO teardown — one `destroy` commit per node the build
  created, branches/list items/slot content included — returning the
  state (fields intact, slab handles spent).
- **The flush loop widened to trait objects** (`R: Reactions + ?Sized`)
  — the one signature change the object-safe seam needs; generated code
  passes concrete state and is byte-identical (D-8 untouched, `lig
  reproducibility` green).
- **The walkthrough example** (`tests/compile-clean/examples/
  host_mount.rs`): the committed counter unit mounted on `MockHost` end
  to end — the style-table bind (`Counter_STYLE_CSS`), mount, one
  save-tick, one event dispatch (`fire_1`), dispose, and the balanced
  commit stream asserted — runnable with `cargo run -p compile-clean
  --example host_mount` without touching `dist/`.

## Design decisions (ADR-0033)

- The trait adds only the teardown accessor: `Reactions` already carries
  the schedule contract, so the consumer impl is one field reference —
  no new generated code, no codegen churn, zero dependency change for
  the `no_std` crate (the zero-deps no_std proof stays green).
- `Mounted` is generic over the concrete component (setters and `fire_N`
  stay first-class); the trait is object-safe for registries that need
  `&mut dyn ComponentUnit`.
- The style-table binding stays a host convention, now pinned by the
  walkthrough example instead of a new abstraction.

## Evidence

- **TDD**: one red→green cycle. Red: the `mount.rs` tests failed to
  compile (`unresolved imports super::ComponentUnit, super::Mounted`)
  against the unwired API; green: 7 mount tests + the `?Sized` flush
  widening, full `cargo test --workspace` green.
- **Bench + budget**: the `runtime/mount_lifecycle` criterion workload
  (mount, one `Mounted::update` save-tick, dispose), ledger row
  `runtime_mount_lifecycle` — release median 0.29 µs (bench) / 0.32 µs
  (in-process gate run), ceiling 1.5 µs (≈5× headroom), 25 rows total,
  all within budget under `cargo run -p lig-cli --release -- budgets`.
- **Example**: `host_mount` — `count = 3 (doubled = 6)` after the
  save-tick, `count = 4 (doubled = 8)` after the click dispatch,
  `6 create commits, 6 destroy commits — balanced: true` on dispose.
- **Gates**: `cargo fmt --all -- --check`; `cargo clippy --workspace
  --all-targets --all-features -- -D warnings` (pedantic active from the
  workspace ladder — see the gate note below); the zero-suppression grep
  (only the two pre-existing `#![expect(unsafe_code)]` of the
  single-unsafe-module policy); `cargo test --workspace --all-features`;
  `cargo test --workspace --benches`.

## Known gaps (deferred, deliberate)

- The mount front does not hide the `Host` implementation — writing the
  15 methods stays the embedding work (D-5's contract boundary).
- A `dyn` component registry (host stores mounted components behind
  `&mut dyn ComponentUnit`) has the trait ready but no shipped
  container; it is an embedding milestone of its own if wanted.
- The patch engine's host bridge (`lig-patch` driving a mounted surface)
  stays untouched.
