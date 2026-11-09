//! The consumer walkthrough (M16): the embeddable pipeline, end to end.
//!
//! 1. Register a source in a [`Session`] and run one digest-gated compile
//!    pass (`lower → admit → codegen`, the D-9 engine behind `lig build`).
//! 2. Run the pass again — every memo is reused, nothing recomputes.
//! 3. Emit `dist/` twice — artifacts are written once, then **kept**
//!    byte-for-byte while the digests hold.
//! 4. Print the generated unit: plain Rust on the `lig_runtime` prelude.
//!
//! Run: `cargo run -p lig-incremental --example compile_and_emit`

use lig_admission::Profile;
use lig_incremental::Session;
use lig_incremental::emit::emit_units;
use std::path::Path;

fn main() {
    // The M0 reference workload — the smallest admitted program (a signal,
    // a memo, a delegated click, a `Show`-shaped conditional).
    let source = include_str!("../../../fixtures/conformance/counter/input.solid.tsx");

    // 1. Inputs in, one cold pass out. Every stage runs; the report says so.
    let mut session = Session::new();
    session.set_source("counter.tsx", source);
    let report = session.compile(Profile::Layout);
    println!(
        "pass 1: revision {} — {} recompiled, {} reused in {:.1} ms",
        report.revision,
        report.recompiled.len(),
        report.reused.len(),
        report.elapsed.as_secs_f64() * 1000.0
    );

    // 2. The warm pass: identical digests validate every memo (D-9), so the
    //    second compile recomputes nothing — this is what `lig watch` ticks
    //    feel like for free.
    let report = session.compile(Profile::Layout);
    assert!(report.all_reused(), "no-op pass keeps every memo");
    println!(
        "pass 2: all reused ({} memo(s) served from digests)",
        report.reused.len()
    );

    // 3. Emit dist/ (fail-closed BR-1 upstream: nothing here runs unless
    //    every unit was admitted). The second emission keeps the files.
    let out = std::env::temp_dir().join("lig-example-dist");
    let _ = std::fs::remove_dir_all(&out);
    let units = session.units();
    let first = emit_units(&out, &units, "taffy", false).expect("dist/ is writable");
    println!(
        "emit 1: wrote {} artifact set(s) → {}",
        first.written.len(),
        out.display()
    );
    let second = emit_units(&out, &units, "taffy", false).expect("dist/ is writable");
    assert!(second.written.is_empty(), "digest-gated: nothing re-wrote");
    println!(
        "emit 2: kept {} artifact set(s) byte-for-byte (digest unchanged)",
        second.kept.len()
    );

    // 4. The generated unit: a component struct with state fields, generated
    //    setters as the only write path, a const Schedule, static template
    //    tables, and build/fire entry points (docs/appendix-a.md walks it).
    let unit = session
        .unit(Path::new("counter.tsx"))
        .expect("the registered source has a unit");
    let generated = unit
        .generated
        .as_ref()
        .expect("an admitted unit carries its codegen output");
    println!(
        "\ncounter.rs: {} bytes, component digest {:#018x}",
        generated.text.len(),
        generated.digest
    );
    println!("---- head of the generated unit ----");
    for line in generated.text.lines().take(14) {
        println!("{line}");
    }
    println!("-----------------------------------");
    println!(
        "artifacts left in {} for inspection — delete freely",
        out.display()
    );
}
