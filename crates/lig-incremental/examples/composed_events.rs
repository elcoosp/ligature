//! The composed-event walkthrough (M17): per-item event dispatch on
//! COMPOSED list bodies — a For/v-for body that hangs inside a branch or a
//! slot fill still admits its own direct handler.
//!
//! 1. Compile a composed-event unit in a [`Session`] (the stages walk the
//!    M16 `tracing` spans: `session_compile` → `lower` → `admit` →
//!    `codegen` — attach any subscriber to see them).
//! 2. Emit `dist/` — the typed miette `EmitError` seam (M16) is the
//!    fail path; here it succeeds and writes the artifact set.
//! 3. Print the generated surface: the composed `reconcile` with its
//!    receiver-parameterized attach arms, and the index-addressed
//!    `fire_f*_d*` dispatch glue.
//! 4. Show what STAYS denied: a handler deeper in the item subtree (a
//!    branch inside the item) — the LIG-E118 boundary rendered through
//!    miette with the fix and the catalog URL.
//!
//! Run: `cargo run -p lig-incremental --example composed_events`

use lig_admission::Profile;
use lig_core::diagnostic::render;
use lig_incremental::Session;
use lig_incremental::emit::emit_units;
use std::path::Path;

fn main() {
    // What STAYS denied (the M17 boundary): a handler DEEPER in the item
    // subtree — a branch inside the item here. Per-item × per-mount
    // composition needs per-item control flow (the foundation
    // follow-up); fail-closed, one precise reason at the event's span.
    const DENIED: &str = concat!(
        "import { For, Show } from \"solid-js\";\n",
        "export function Rows() {\n",
        "  const [on, setOn] = createSignal(true);\n",
        "  const [items, setItems] = createSignal([1, 2]);\n",
        "  return (\n",
        "    <Show when={on()}>\n",
        "      <For each={items()}>\n",
        "        {(n) => (\n",
        "          <Show when={n > 0}>\n",
        "            <li onClick={() => setItems([n])}>{n}</li>\n",
        "          </Show>\n",
        "        )}\n",
        "      </For>\n",
        "    </Show>\n",
        "  );\n",
        "}\n",
    );

    // The composed-event shape: the For body hangs inside a Show branch,
    // and the handler sits DIRECTLY on the body — M17 admits it. The
    // attach rides the composed reconcile's insert arms (one per fresh
    // item, through the branch receiver), and dispatch goes through the
    // same source-addressed fire fn the direct list uses (M15).
    const ADMITTED: &str = concat!(
        "import { For, Show } from \"solid-js\";\n",
        "export function PickPanel() {\n",
        "  const [on, setOn] = createSignal(true);\n",
        "  const [rows, setRows] = createSignal([\"alpha\", \"beta\"]);\n",
        "  const [picked, setPicked] = createSignal(\"\");\n",
        "  return (\n",
        "    <Show when={on()}>\n",
        "      <For each={rows()} key={(r) => r}>\n",
        "        {(r) => <li onClick={() => setPicked(r)}>{r}</li>}\n",
        "      </For>\n",
        "    </Show>\n",
        "  );\n",
        "}\n",
    );

    // 1. The cold pass: lower → admit → codegen, each stage inside its
    //    named tracing span (M16) — the observability seam is unchanged
    //    for the composed path.
    let mut session = Session::new();
    session.set_source("pick_panel.tsx", ADMITTED);
    let report = session.compile(Profile::Layout);
    let unit = session
        .unit(Path::new("pick_panel.tsx"))
        .expect("the registered source has a unit");
    assert!(
        unit.admitted,
        "composed body events admit: {:?}",
        unit.diagnostics
    );
    println!(
        "pick_panel.tsx: admitted in revision {} ({} recompiled) — a For body inside \
         a Show branch with its own handler\n",
        report.revision,
        report.recompiled.len()
    );

    // 2. Emit dist/ (fail-closed BR-1 upstream). The typed EmitError seam
    //    (M16) is what an I/O or contract failure looks like here: a
    //    miette Diagnostic with stable code `ligature::emit`, the offending
    //    unit as subject, and help-text naming the fix.
    let out = std::env::temp_dir().join("lig-example-composed");
    let _ = std::fs::remove_dir_all(&out);
    let units = session.units();
    let first = emit_units(&out, &units, "taffy", false).expect("dist/ is writable");
    println!(
        "emit: wrote {} artifact set(s) → {}\n",
        first.written.len(),
        out.display()
    );

    // 3. The generated surface: the composed reconcile (an associated fn —
    //    it addresses the list through the branch receiver), the per-item
    //    attach arms in its insert path, and the per-item fire fn.
    let generated = &unit
        .generated
        .as_ref()
        .expect("an admitted unit generates")
        .text;
    for line in generated.lines() {
        let t = line.trim_start();
        if t.starts_with("pub fn fire_") || t.starts_with("fn reconcile_") || t.contains(".attach(")
        {
            println!("  {t}");
        }
    }
    println!(
        "\n  dispatch: `app.fire_f2_d0(&mut h, index, &ev)` substitutes the live \
         item — a stale index is a no-op\n"
    );

    session.set_source("rows.tsx", DENIED);
    let _report = session.compile(Profile::Layout);
    let denied = session
        .unit(Path::new("rows.tsx"))
        .expect("the registered source has a unit");
    assert!(!denied.admitted, "branch-in-item events stay denied (BR-1)");
    println!(
        "rows.tsx: {} diagnostic(s), nothing was emitted — a handler inside a \
         list-item branch keeps the denial\n",
        denied.diagnostics.len()
    );
    for d in &denied.diagnostics {
        // miette graphical render: file header, code frame with a caret, the
        // `help:` suggestion, and the ligature.dev/e/<code> catalog URL.
        println!("{}", render("rows.tsx", DENIED, d));
    }
}
