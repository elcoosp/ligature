//! The watch-tick seam (M23): `lig watch`'s per-save loop as ONE session
//! call. [`Session::tick`] diffs the fresh snapshot (the watch-sync seam),
//! compiles what the digest gate invalidated (D-9), and emits the artifact
//! set into any sink — fail-closed (BR-1): a denied pass never lets the
//! sink see a byte. The CLI's `lig watch` loop drives exactly this call
//! per tick; here the snapshots are simulated in memory and the artifacts
//! land in a [`MemSink`](lig_incremental::emit::MemSink) so the walkthrough
//! touches no `dist/`.
//!
//! Run: `cargo run -p lig-incremental --example watch_tick`

/// The LIG-E117 denial workload: lowers fine, then the admit stage denies
/// on the layout-only profile — the tick's BR-1 refusal, as data.
const DENIED: &str = concat!(
    "import { createSignal } from \"solid-js\";\n",
    "export function A() {\n",
    "  const [h, setH] = createSignal(\"<b>x</b>\");\n",
    "  return <div innerHTML={h()} />;\n",
    "}\n",
);

use lig_admission::Profile;
use lig_incremental::Session;
use lig_incremental::emit::MemSink;
use lig_parse_solid::tests_support::COUNTER_TSX;
use std::path::Path;

fn main() {
    // A small project: two counter units under different names.
    let mut session = Session::new();
    let mut sink = MemSink::new();

    // Tick 1 — the cold tick: both files are additions, both recompute,
    // both artifact sets write. One call, one report per stage of the loop
    // body (sync → pass → emission).
    let tick = session
        .tick(
            vec![
                (Path::new("a.tsx").to_path_buf(), COUNTER_TSX),
                (Path::new("b.tsx").to_path_buf(), COUNTER_TSX),
            ],
            &[],
            &mut sink,
            Profile::Layout,
            false,
        )
        .expect("the cold tick passes");
    println!(
        "tick 1: {} added → {} recompiled → {} written",
        tick.sync.added.len(),
        tick.pass.recompiled.len(),
        tick.emission.written.len()
    );

    // Tick 2 — an identical snapshot: the sync is a no-op (D-9), every memo
    // is reused, every artifact is kept — the tick is a no-op against the
    // sink, what the watch loop does between saves.
    let tick = session
        .tick(
            vec![
                (Path::new("a.tsx").to_path_buf(), COUNTER_TSX),
                (Path::new("b.tsx").to_path_buf(), COUNTER_TSX),
            ],
            &[],
            &mut sink,
            Profile::Layout,
            false,
        )
        .expect("the warm tick passes");
    println!(
        "tick 2: nothing changed → all reused → kept {:?}",
        tick.emission.kept
    );

    // Tick 3 — one file edited: the tick recompiles exactly that unit and
    // rewrites exactly that stem (the other unit's digests still hold).
    let edited = COUNTER_TSX.replace("Add", "Add2");
    let tick = session
        .tick(
            vec![
                (Path::new("a.tsx").to_path_buf(), COUNTER_TSX),
                (Path::new("b.tsx").to_path_buf(), edited.as_str()),
            ],
            &[],
            &mut sink,
            Profile::Layout,
            false,
        )
        .expect("the edit tick passes");
    println!(
        "tick 3: edited {:?} → recompiled {:?} → written {:?}, kept {:?}",
        tick.sync
            .edited
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>(),
        tick.pass
            .recompiled
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>(),
        tick.emission.written,
        tick.emission.kept
    );

    // Tick 4 — a denial rides the same call: one LIG-E117 unit joins the
    // snapshot and the WHOLE tick refuses (BR-1) — the error names the
    // blocking stem, and the sink still holds exactly what tick 3 left.
    let err = session
        .tick(
            vec![
                (Path::new("a.tsx").to_path_buf(), COUNTER_TSX),
                (Path::new("b.tsx").to_path_buf(), edited.as_str()),
                (Path::new("denied.tsx").to_path_buf(), DENIED),
            ],
            &[],
            &mut sink,
            Profile::Layout,
            false,
        )
        .expect_err("a denied pass refuses the tick");
    println!("tick 4: refused — {err}");
    println!(
        "tick 4: the sink still holds {} artifacts (nothing wrote)",
        sink.paths().len()
    );
    let denied_unit = session
        .unit(Path::new("denied.tsx"))
        .expect("the denied unit");
    println!(
        "tick 4: the denial rides the session — {} diagnostic(s), first: {}",
        denied_unit.diagnostics.len(),
        denied_unit
            .diagnostics
            .first()
            .map_or("?", |d| d.code.as_str())
    );

    // Tick 5 — drop the denied file and remove `b` entirely: the session
    // prunes b's artifacts from the sink (best-effort, the sink contract).
    let tick = session
        .tick(
            vec![(Path::new("a.tsx").to_path_buf(), COUNTER_TSX)],
            &[],
            &mut sink,
            Profile::Layout,
            true,
        )
        .expect("the prune tick passes");
    println!(
        "tick 5: removed {:?} → pruned {:?}, a.rs still held: {}",
        tick.sync.removed.len(),
        tick.emission.pruned,
        sink.get("a.rs").is_some()
    );

    println!("no dist/ was touched — the whole watch story ran against the MemSink");
}
