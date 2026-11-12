//! The position-resolver walkthrough (M24): the library story behind
//! `lig resolve` — the chained consumer map is a queryable surface, not
//! just an emitted artifact. Build the three maps for the committed counter
//! (the recorded link table doing the heavy lifting, M22), resolve real
//! generated positions back to source, and see the typed `Unmapped` failure
//! as data. No `dist/`, no file I/O, no scanning.
//!
//! Run: `cargo run -p lig-sourcemap --example resolve_position`

use lig_codegen::generate_component;
use lig_core::id::ComponentId;
use lig_parse_solid::tests_support::COUNTER_TSX;

fn main() {
    let module = lig_parse_solid::lower_module(COUNTER_TSX, "counter.tsx").expect("counter lowers");
    let unit = generate_component(&module, ComponentId::new(0)).expect("counter codegens");

    // The three maps (what `emit_units_to` writes as maps/<stem>.map.json).
    let src_map = lig_sourcemap::src_to_ir(&module, COUNTER_TSX);
    let ir_map = lig_sourcemap::ir_to_rust(&module, &unit.text, &unit.links)
        .expect("the recorded table builds the map");
    let merged = lig_sourcemap::chain(&src_map, &ir_map);
    println!(
        "merged consumer map: {} tokens over {} source(s)",
        merged.get_tokens().len(),
        merged.get_sources().len()
    );

    // Resolve a few generated positions — every token's own line:col
    // resolves to where it came from (exact when the position is unique).
    println!("\n== resolve generated positions ==");
    for token in merged.get_tokens().take(5) {
        let (line, col) = (token.get_dst_line(), token.get_dst_col());
        let got = lig_sourcemap::resolve(&merged, line, col).expect("a carried position resolves");
        let name = got.name.as_deref().unwrap_or("-");
        println!(
            "  generated {line}:{col} → {}:{}:{} (ir node: {name})",
            got.source,
            got.line + 1,
            got.col + 1
        );
    }

    // A reaction marker's recorded offset resolves to its region — the
    // emitter wrote it (M22), the map anchors it, the resolver reads it.
    let first = unit.links.links.first().expect("the counter records links");
    let (line, col) = lig_sourcemap::line_col(first.generated_offset, &unit.text);
    let got = lig_sourcemap::resolve(&merged, line, col);
    println!(
        "\nfirst recorded link (ordinal {} @ {line}:{col}): {:?}",
        first.ir_ordinal, got
    );

    // The typed failure: a position off the map is data, not a guess.
    let err = lig_sourcemap::resolve(&merged, 9_999, 0).expect_err("off the map");
    println!("\noff the map: {err}");

    // And it still resolves after a JSON round-trip — the disk-read map
    // (what the CLI loads) is the same surface.
    let json = merged.to_json();
    let loaded = oxc_sourcemap::SourceMap::from_json(json).expect("the map round-trips");
    let token = merged.get_tokens().next().expect("tokens");
    let got = lig_sourcemap::resolve(&loaded, token.get_dst_line(), token.get_dst_col())
        .expect("the loaded map resolves");
    println!(
        "from JSON: generated {}:{} → {}:{}:{}",
        token.get_dst_line(),
        token.get_dst_col(),
        got.source,
        got.line + 1,
        got.col + 1
    );
}
