//! The composed-content walkthrough (M18): per-item events AND per-item
//! slot assembly on COMPOSED CONTENT list bodies — a For/v-for body that
//! hangs inside a fill or a fallback still admits its own direct handler
//! and its own direct outlet.
//!
//! 1. Compile a composed-content unit in a [`Session`] (the M16 `tracing`
//!    spans walk the stages: `session_compile` → `lower` → `admit` →
//!    `codegen` — attach any subscriber to see them).
//! 2. Emit `dist/` — the typed miette `EmitError` seam (M16) is the fail
//!    path; here it succeeds and writes the artifact set.
//! 3. Print the generated surface: the fill-carried composed reconcile
//!    (M17's receiver-parameterized machinery, the content leg), the
//!    per-item fallback mounts in the shared replay (M14's, through any
//!    receiver), and the index-addressed `fire_f*_d*` dispatch glue.
//! 4. Show what STAYS denied: an outlet inside a branch hanging on a
//!    content list body — the sticky branch-in-item boundary, rendered
//!    through miette with the fix and the catalog URL.
//!
//! Run: `cargo run -p lig-incremental --example composed_content`

use lig_admission::Profile;
use lig_core::diagnostic::render;
use lig_incremental::Session;
use lig_incremental::emit::emit_units;
use std::path::Path;

fn main() {
    // What STAYS denied (the M18 boundary): an outlet inside a BRANCH
    // hanging on a composed content list body. The lift covers the body's
    // OWN surface only — per-item × per-mount composition (an outlet that
    // mounts with a branch inside every live item) needs per-item control
    // flow, the named foundation follow-up.
    const DENIED: &str = concat!(
        "import { For, Show } from \"solid-js\";\n",
        "export function Cell() {\n",
        "  return <Slot name=\"cell\" />;\n",
        "}\n",
        "export function PickList() {\n",
        "  const [rows, setRows] = createSignal([\"alpha\", \"beta\"]);\n",
        "  return (\n",
        "    <Cell cell={<For each={rows()} key={(r) => r}>\n",
        "      {(r) => (\n",
        "        <Show when={r.length > 0}>\n",
        "          <Slot name=\"row\"><i>{r}</i></Slot>\n",
        "        </Show>\n",
        "      )}\n",
        "    </For>} />\n",
        "  );\n",
        "}\n",
    );

    // The composed-content shape: the For hangs inside a fill carried into
    // a branch-mounted child, and the body carries BOTH the handler and the
    // outlet directly — M18 admits it. The fill mounts into the fresh
    // instance at branch mount, the attach rides the composed reconcile's
    // insert arms (one per fresh item, through the content receiver), the
    // fallback mounts per item through the shared replay, and dispatch goes
    // through the same source-addressed fire fn the direct list uses (M15).
    const ADMITTED: &str = concat!(
        "import { For, Show } from \"solid-js\";\n",
        "export function Cell() {\n",
        "  return <Slot name=\"cell\" />;\n",
        "}\n",
        "export function PickList() {\n",
        "  const [on, setOn] = createSignal(true);\n",
        "  const [rows, setRows] = createSignal([\"alpha\", \"beta\"]);\n",
        "  const [picked, setPicked] = createSignal(\"\");\n",
        "  return (\n",
        "    <Show when={on()}>\n",
        "      <Cell cell={<For each={rows()} key={(r) => r}>\n",
        "        {(r) => (\n",
        "          <li onClick={() => setPicked(r)}>\n",
        "            <Slot name=\"row\"><i class=\"pick\">{r}</i></Slot>\n",
        "          </li>\n",
        "        )}\n",
        "      </For>} />\n",
        "    </Show>\n",
        "  );\n",
        "}\n",
    );

    // 1. The cold pass: lower → admit → codegen, each stage inside its
    //    named tracing span (M16) — the observability seam is unchanged
    //    for the composed-content path.
    let mut session = Session::new();
    session.set_source("pick_list.tsx", ADMITTED);
    let report = session.compile(Profile::Layout);
    let unit = session
        .unit(Path::new("pick_list.tsx"))
        .expect("the registered source has a unit");
    assert!(
        unit.admitted,
        "composed-content body events + outlets admit: {:?}",
        unit.diagnostics
    );
    println!(
        "pick_list.tsx: admitted in revision {} ({} recompiled) — a For body inside \
         a fill carried into a branch-mounted child, with its own handler and \
         its own outlet\n",
        report.revision,
        report.recompiled.len()
    );

    // 2. Emit dist/ (fail-closed BR-1 upstream). The typed EmitError seam
    //    (M16) is what an I/O or contract failure looks like here: a
    //    miette Diagnostic with stable code `ligature::emit`, the offending
    //    unit as subject, and help-text naming the fix.
    let out = std::env::temp_dir().join("lig-example-composed-content");
    let _ = std::fs::remove_dir_all(&out);
    let units = session.units();
    let first = emit_units(&out, &units, "taffy", false).expect("dist/ is writable");
    println!(
        "emit: wrote {} artifact set(s) → {}\n",
        first.written.len(),
        out.display()
    );

    // 3. The generated surface: the content-owned composed reconcile (a
    //    receiver-parameterized associated fn), the per-item attach arms in
    //    its insert path, the per-item fallback mounts in the shared replay,
    //    and the per-item fire fn.
    let generated = &unit
        .generated
        .as_ref()
        .expect("an admitted unit generates")
        .text;
    for line in generated.lines() {
        let t = line.trim_start();
        if t.starts_with("fn reconcile_f")
            || t.contains(".attach(")
            || t.contains(".mount_slot(")
            || t.starts_with("pub fn fire_")
        {
            println!("  {t}");
        }
    }
    println!(
        "\n  dispatch: `app.fire_f4_d0(&mut h, index, &ev)` substitutes the live \
         item — a stale index is a no-op; the fallback re-binds per replay\n"
    );

    session.set_source("branch_outlet.tsx", DENIED);
    let _report = session.compile(Profile::Layout);
    let denied = session
        .unit(Path::new("branch_outlet.tsx"))
        .expect("the registered source has a unit");
    assert!(
        !denied.admitted,
        "branch-in-item outlets on composed content stay denied (BR-1)"
    );
    println!(
        "branch_outlet.tsx: {} diagnostic(s), nothing was emitted — an outlet \
         inside a branch hanging on a composed content list body keeps the denial\n",
        denied.diagnostics.len()
    );
    for d in &denied.diagnostics {
        // miette graphical render: file header, code frame with a caret, the
        // `help:` suggestion, and the ligature.dev/e/<code> catalog URL.
        println!("{}", render("branch_outlet.tsx", DENIED, d));
    }
}
