//! The watch-sync seam: what a `lig watch` tick does, as library calls —
//! one [`Session::sync`] diffs a fresh source snapshot into
//! added/edited/removed (with the D-9 no-op discipline), the digest gate
//! recompiles exactly the touched units, and the emission-target seam
//! writes only what changed. No filesystem watching here: the tick's
//! snapshot is simulated in memory, and the artifacts land in a
//! [`MemSink`](lig_incremental::emit::MemSink) so the walkthrough touches
//! no `dist/`.
//!
//! Run: `cargo run -p lig-incremental --example watch_sync`

use lig_admission::Profile;
use lig_incremental::Session;
use lig_incremental::emit::{MemSink, emit_units_to};
use lig_parse_solid::tests_support::COUNTER_TSX;
use std::path::Path;

fn main() {
    // The snapshot of a small project: two copies of the counter under
    // different names (the text is the same; the paths are not).
    let snapshot = |b_text: &str| -> Vec<(std::path::PathBuf, String)> {
        vec![
            (Path::new("a.tsx").to_path_buf(), COUNTER_TSX.to_string()),
            (Path::new("b.tsx").to_path_buf(), b_text.to_string()),
        ]
    };

    // Tick 1 — an empty session syncs the initial snapshot: both files are
    // additions. One compile: both recompute. One emission: both written.
    let mut session = Session::new();
    let report = session.sync(snapshot(COUNTER_TSX), &[]);
    println!(
        "tick 1: {} added, {} edited, {} removed",
        report.added.len(),
        report.edited.len(),
        report.removed.len()
    );
    session.compile(Profile::Layout);
    let mut sink = MemSink::new();
    let units = session.units();
    let emission = emit_units_to(&mut sink, &units, "taffy", false).expect("the map sink writes");
    println!(
        "tick 1: {} recompiled, {} artifact set(s) written",
        2,
        emission.written.len()
    );

    // Tick 2 — an identical snapshot: the sync is a no-op (identical texts
    // are not edits, D-9), so the tick skips the pass entirely — what the
    // watch loop does between saves.
    let report = session.sync(snapshot(COUNTER_TSX), &[]);
    assert!(report.is_empty(), "identical snapshot: {report:?}");
    println!("tick 2: nothing changed — the pass is skipped");

    // Tick 3 — one file edited: sync reports exactly that edit, the compile
    // recompiles exactly that unit (the other memo is reused), and the
    // digest-gated emission rewrites only its artifact set.
    let edited = COUNTER_TSX.replace("Add", "Add2");
    let report = session.sync(snapshot(&edited), &[]);
    assert_eq!(report.edited, vec![Path::new("b.tsx").to_path_buf()]);
    println!(
        "tick 3: {} edited ({:?})",
        report.edited.len(),
        report.edited.first().map(|p| p.display().to_string())
    );
    let compile = session.compile(Profile::Layout);
    println!(
        "tick 3: {} recompiled, {} reused (revision {})",
        compile.recompiled.len(),
        compile.reused.len(),
        compile.revision
    );
    let units = session.units();
    let emission = emit_units_to(&mut sink, &units, "taffy", false).expect("the map sink writes");
    println!(
        "tick 3: re-emitted {:?}, kept {:?}",
        emission.written, emission.kept
    );

    // Tick 4 — the file is gone: sync drops it (and the emission prunes its
    // artifacts from the sink, the watch-sync contract).
    let report = session.sync(
        vec![(Path::new("a.tsx").to_path_buf(), edited.clone())],
        &[],
    );
    assert_eq!(report.removed, vec![Path::new("b.tsx").to_path_buf()]);
    let _ = session.compile(Profile::Layout);
    let units = session.units();
    let emission = emit_units_to(&mut sink, &units, "taffy", true).expect("the map sink writes");
    println!(
        "tick 4: {} removed, pruned artifact stems {:?}",
        report.removed.len(),
        emission.pruned
    );

    println!("no dist/ was touched — the whole watch story ran against the MemSink");
}
