//! What a denial looks like from Rust (M16): a construct outside the
//! enforced subset lowers into a Deny-tier diagnostic — fail-closed (BR-1),
//! zero output downstream — and renders through miette's graphical handler
//! with the code frame, the fix, and the catalog URL.
//!
//! Run: `cargo run -p lig-incremental --example render_denial`

use lig_admission::Profile;
use lig_core::diagnostic::render;
use lig_incremental::Session;
use std::path::Path;

fn main() {
    // `.filter(...)` inside a view expression is a Deny-tier construct
    // (LIG-E114 — calls with callbacks are outside the enforced subset);
    // move the filtering into a memo in the model section instead.
    const DENIED: &str = concat!(
        "import { createSignal } from \"solid-js\";\n",
        "export function App() {\n",
        "  const [items] = createSignal([1, 2, 3]);\n",
        "  return <ul>{items().filter(i => i > 1)}</ul>;\n",
        "}\n",
    );

    let mut session = Session::new();
    session.set_source("denied.tsx", DENIED);
    let _report = session.compile(Profile::Layout);

    let unit = session
        .unit(Path::new("denied.tsx"))
        .expect("the registered source has a unit");
    assert!(!unit.admitted, "the unit is denied (fail-closed, BR-1)");
    assert!(
        unit.generated.is_none(),
        "a denied unit carries no generated output"
    );

    println!(
        "denied.tsx: {} diagnostic(s), nothing was emitted — `lig build` refuses \
         to touch dist/ while any unit is denied\n",
        unit.diagnostics.len()
    );
    for d in &unit.diagnostics {
        // miette graphical render: file header, code frame with a caret, the
        // `help:` suggestion, and the ligature.dev/e/<code> catalog URL.
        println!("{}", render("denied.tsx", DENIED, d));
    }
}
