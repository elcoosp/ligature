//! The link-table consumer walkthrough (M22): the committed counter fixture
//! lowers, generates with a recorded §9.4 link table, and resolves — the
//! library story behind `lig resolve`, with no text scanning anywhere.
//!
//! Sections:
//! 1. Generate the counter unit and inspect the recorded link table —
//!    every reaction/template marker's exact byte offset, recorded by the
//!    emitter post-unparse.
//! 2. Build the `src→IR` and `IR→Rust` maps from the table and chain them
//!    into the merged consumer map (what `emit_units_to` writes as
//!    `maps/<stem>.map.json`).
//! 3. Resolve generated positions back to source through the chain — the
//!    same lookup `lig resolve <map> <line> <col>` performs.
//! 4. The typed failure (M22's `SourceMapError`): a drifted offset fails
//!    closed with the ordinal and offset as data.

use lig_codegen::generate_component;
use lig_core::id::ComponentId;
use lig_parse_solid::tests_support::COUNTER_TSX;

fn main() {
    // The committed counter fixture (the D-8 reference workload).
    let module = lig_parse_solid::lower_module(COUNTER_TSX, "counter.tsx").expect("counter lowers");
    let unit = generate_component(&module, ComponentId::new(0)).expect("counter codegens");

    // --- 1. the recorded §9.4 link table -----------------------------------
    println!("== the recorded link table ==");
    println!(
        "  the emitter recorded {} links ({} reactions + {} templates), sorted by offset:",
        unit.links.links.len(),
        unit.links.reactions.len(),
        unit.links.templates.len()
    );
    for link in &unit.links.links {
        let marker = &unit.text[link.generated_offset as usize..]
            .split('\n')
            .next()
            .unwrap_or_default()
            .to_string();
        let what = if link.ir_ordinal >= lig_sourcemap::DYNAMIC_BASE {
            format!(
                "template t{}",
                link.ir_ordinal - lig_sourcemap::DYNAMIC_BASE
            )
        } else {
            format!("reaction r{}", link.ir_ordinal)
        };
        println!(
            "  {what:>14} @ offset {:>5}  {} ",
            link.generated_offset,
            marker.trim()
        );
    }

    // --- 2. the three maps --------------------------------------------------
    let src_map = lig_sourcemap::src_to_ir(&module, COUNTER_TSX);
    let ir_map = lig_sourcemap::ir_to_rust(&module, &unit.text, &unit.links)
        .expect("the recorded table builds the map");
    let merged = lig_sourcemap::chain(&src_map, &ir_map);
    println!("\n== the three maps ==");
    println!("  src→IR:  {} tokens", src_map.get_tokens().count());
    println!(
        "  IR→Rust: {} tokens (all anchored at recorded offsets)",
        ir_map.get_tokens().count()
    );
    println!(
        "  merged:  {} tokens (what maps/counter.map.json holds)",
        merged.get_tokens().count()
    );

    // --- 3. resolve a generated position back to source ---------------------
    println!("\n== resolve a generated position ==");
    // Walk the merged map: every token is (generated position → source
    // position + the IR label it came through). Print the first few.
    for token in merged.get_tokens().take(4) {
        let name = token
            .get_name_id()
            .and_then(|nid| merged.get_name(nid))
            .unwrap_or("-");
        let src_id = token.get_source_id().unwrap_or(0);
        let src_file = merged.get_source(src_id).unwrap_or("?");
        let source_line = merged
            .get_source_content(src_id)
            .and_then(|c| c.lines().nth(token.get_src_line() as usize))
            .unwrap_or("?");
        println!(
            "  {}:{}:{} ← {}:{}:{}  [{}]  {}",
            src_file,
            token.get_dst_line() + 1,
            token.get_dst_col() + 1,
            src_file,
            token.get_src_line() + 1,
            token.get_src_col() + 1,
            name,
            source_line.trim(),
        );
    }

    // --- 4. the typed failure ------------------------------------------------
    println!("\n== the typed failure (SourceMapError) ==");
    let mut drifted = unit.links.clone();
    if let Some(l0) = drifted.links.first_mut() {
        l0.generated_offset += 7; // simulate a table/text disagreement
    }
    match lig_sourcemap::ir_to_rust(&module, &unit.text, &drifted) {
        Ok(_) => panic!("a drifted table must fail closed"),
        Err(err) => {
            println!("  {err}");
            if let lig_sourcemap::SourceMapError::Anchor { ordinal, offset } = &err {
                println!("  → as data: ordinal {ordinal}, drifted offset {offset}");
            }
        }
    }

    // --- 5. the NFR-6 verifier ----------------------------------------------
    let stats = lig_sourcemap::verify(&module, COUNTER_TSX, &unit.text, &unit.links)
        .expect("the committed unit verifies");
    println!("\n== verify (NFR-6) ==");
    println!(
        "  {} expression spans resolved, {} reactions linked, {} merged tokens survive a JSON round-trip",
        stats.expr_spans_resolved, stats.reactions_linked, stats.merged_tokens
    );
}
