//! The emission-target seam (the `ArtifactSink` trait): digest-gated
//! emission is not bound to the filesystem. The same `emit_units_to` body
//! writes the §9.5 artifact set into any sink — the [`FsSink`] `dist/`
//! layout `lig build` uses, or the in-memory [`MemSink`] this walkthrough
//! drives: artifacts land in a caller-owned map, ready for a host that has
//! no filesystem (an embedded target, a test, an asset pipeline).
//!
//! 1. Compile the M0 reference workload in a [`Session`].
//! 2. Emit into a `MemSink` — the full artifact set, no `dist/` touched.
//! 3. Run the emission again — the digest gate keeps it byte-for-byte.
//! 4. Walk the artifacts out of the sink and print the generated head.
//!
//! Run: `cargo run -p lig-incremental --example mem_sink`

use lig_admission::Profile;
use lig_incremental::Session;
use lig_incremental::emit::{MemSink, emit_units_to};
use std::path::Path;

fn main() {
    // 1. One cold pass over the counter (a signal, a memo, a delegated
    //    click, a `Show`-shaped conditional).
    let source = include_str!("../../../fixtures/conformance/counter/input.solid.tsx");
    let mut session = Session::new();
    session.set_source("counter.tsx", source);
    let report = session.compile(Profile::Layout);
    println!(
        "pass: revision {} — {} recompiled in {:.1} ms",
        report.revision,
        report.recompiled.len(),
        report.elapsed.as_secs_f64() * 1000.0
    );

    // 2. Emit into memory: the artifact set is exactly what `dist/` gets,
    //    byte-for-byte — the seam moves where artifacts land, never what
    //    they contain. Fail-closed upstream (BR-1): nothing here runs
    //    unless every unit was admitted.
    let units = session.units();
    let mut sink = MemSink::new();
    let first = emit_units_to(&mut sink, &units, "taffy", false).expect("the map sink is writable");
    println!(
        "emit 1: wrote {} artifact set(s) into memory ({} paths)",
        first.written.len(),
        sink.paths().len()
    );

    // 3. The second emission keeps: the recorded digests still validate.
    let second =
        emit_units_to(&mut sink, &units, "taffy", false).expect("the map sink is writable");
    assert!(second.written.is_empty(), "digest-gated: nothing re-wrote");
    println!(
        "emit 2: kept {} artifact set(s) byte-for-byte (digest unchanged)",
        second.kept.len()
    );

    // 4. The artifacts are plain bytes in the sink — hand them to any host.
    let unit = session
        .unit(Path::new("counter.tsx"))
        .expect("the registered source has a unit");
    let generated = unit
        .generated
        .as_ref()
        .expect("an admitted unit carries its codegen output");
    println!(
        "\nmem://counter.rs: {} bytes, component digest {:#018x}",
        sink.get("counter.rs").map_or(0, <[u8]>::len),
        generated.digest
    );
    for path in sink.paths() {
        println!("  {path:<28} {} B", sink.get(path).map_or(0, <[u8]>::len));
    }
    println!("---- head of the generated unit (mem://counter.rs) ----");
    let text = String::from_utf8(sink.get("counter.rs").expect("counter.rs emitted").to_vec())
        .expect("generated units are UTF-8");
    for line in text.lines().take(10) {
        println!("{line}");
    }
    println!("-------------------------------------------------------");
    println!("no dist/ was touched — the artifacts live in the MemSink");
}
