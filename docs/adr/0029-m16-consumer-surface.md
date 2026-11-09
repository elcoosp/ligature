# ADR-0029: M16 — the consumer surface (examples, session observability, typed emission errors)

- Status: accepted (M16)
- Deciders: compiler team
- Date: 2026-11-09

## Context

Five straight milestones (M11–M15) shipped compiler behavior — the else
leg, template refs, per-item instances, per-item slot assembly, per-item
event dispatch — and the keyed-interaction story is coherent. But the
*consumer* surface of the same stack stopped growing at M3:

- **No runnable consumer example exists.** The repo demonstrates the
  pipeline through the CLI (`lig check/build/…`), the CLI's own
  integration tests, and one committed generated unit
  (`fixtures/generated/counter.rs`). A team embedding
  `lig-incremental` — the README's stated Rust path — copies from a
  README snippet or reads the CLI's internals. Nothing runs with
  `cargo run --example` and nothing shows the digest-gated loop
  (compile → reuse, emit → keep) from the outside.
- **Session passes are unobservable.** `Session::compile` is a black
  box: no spans, no stage timing. An embedded consumer profiling a slow
  unit has nothing to subscribe to; the CLI prints pass-level summaries
  only. `tracing` is the de-facto Rust seam for exactly this and was
  already in the dependency tree transitively.
- **Emission fails as a bare `String`.** `emit_units` returned
  `Result<_, String>` — no stable code, no subject, no fix, and the
  CLI's `eprintln!` could not print a help line. The repo's own D-7
  discipline (typed codes, miette rendering, help-text) stopped at the
  emission seam consumers hit first.
- **Docs drifted while the compiler moved.** The CHANGELOG's newest
  entry was v1.0.0 while tags reached v1.2.1 and the tree sat at an
  untagged 1.3.0; `fixtures/README.md` still named the M2-era "View IR
  v2" (the shipped schema is v5); one session rustdoc carried a grammar
  slip ("A incremental"). Small, but exactly the surfaces a consumer
  reads first.
- **The gate paths panicked on odd paths.** `lig errors-gen` and the
  NFR-5 admission harness `unwrap()`ed path components — weak errors in
  the very code that audits the repo's health.

## Decision

### 1. M16 is a consumer-surface milestone; compiler behavior does not move

No IR change, no admission change, no codegen change — the counter unit
stays byte-identical (D-8 pins it) and every existing gate stays green.
The milestone's exit criteria live in the embed API, the examples
directory, and the docs.

### 2. `crates/lig-incremental/examples/` is the executable contract

Two examples, both fixture-fed so they cannot lie about the pipeline:

- `compile_and_emit` — the full loop: set_source, cold pass (recompiled
  counts), warm pass (`all_reused` asserted), `emit_units` twice
  (written then kept, asserted), and the generated unit's digest + head.
- `render_denial` — a `LIG-E114` denial: fail-closed BR-1 asserted on
  the unit, then the diagnostic rendered through
  `lig_core::diagnostic::render` (miette graphical: code frame, help,
  catalog URL).

`lig-incremental` is the right home: it is the crate the README's Rust
usage names, it already depends on both frontends, admission, codegen,
sourcemap, and core — an example there needs no new workspace edges.

### 3. Stage spans on the session engine (the observability seam)

`Session::compile` wraps each unit's pass in a `session_compile` span;
the queries walk `lower` (parse+frontend), `admit` (tiering + cycle
recovery), and `codegen` spans; `emit_units` walks an `emit` span with
recorded `written`/`kept` counts. Plain `tracing` spans — no new
subscriber dependency, zero effect when no subscriber is installed
(the no-op cost is one span allocation per stage per unit, noise
against a pass that runs codegen). The span capture is pinned by a test
with a hand-rolled subscriber, so the seam cannot silently disappear.

### 4. `EmitError`: the emission seam fails like a diagnostic

`emit_units` returns `Result<EmissionReport, EmitError>`:

- `code` — the stable string `ligature::emit` (not a `LIG-E###`: the
  catalog numbers admission/compiler denials with a user-facing fix;
  emission failures are embed-API errors, and the URL footer would
  promise a catalog page that does not exist);
- `subject` — the unit stem or artifact path;
- `message` — what failed (renders as the error line);
- `help` — the fix, in words (renders as miette `help:`; the CLI prints
  it as an indented `help:` line);
- `source()` — the underlying `std::io::Error` when I/O caused it.

Two construction shapes cover the seam: `io` (create_dir_all, the
atomic write) and `contract` (an admitted unit without IR, a failed
NFR-6 map verification — internal invariants, help-text says "report
it", never "retry").

### 5. The budget ledger covers the session engine

The `session/compile-counter` criterion bench (a full cold pass over the
M0 reference workload) joins the bench suite, and the
`session_compile_counter` ledger row joins `docs/benchmarks/budgets.toml`
with the same ≥5× headroom rule — the session engine is budgeted like
every other stage, and the registry sync check keeps bench ↔ ledger 1:1
(18 rows).

### 6. Docs reconcile with the tree

CHANGELOG backfills v1.1.0/v1.2.0/v1.2.1 from the tags' actual commit
ranges plus an Unreleased section (M14/M15 land there with M16 — v1.3.0
is the version the next release tags); `fixtures/README.md` names the
shipped schema version (v5); the session rustdoc's article is fixed;
the gate paths' unwraps degrade to display-path fallbacks with a
comment at each site.

## Consequences

- Consumers get a runnable starting point, a traceable pipeline, and
  typed failures — the next compiler milestone (nested/composed list
  events, the `LIG-E118` boundary's next step) will be judged by people
  who can now run the thing.
- `emit_units`' signature changes: any out-of-tree caller moves from
  `String` to `EmitError` (a mechanical `.map_err`). In-tree, only the
  CLI's `emit_and_report` — updated in the same change.
- `tracing` becomes a direct dependency of `lig-incremental` (already
  in the lock transitively; zero new packages).
- The bench suite and ledger grow by one row each; the budgets gate's
  sync check enforces the pairing.
