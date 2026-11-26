//! Branch-in-item events, walked end to end (M27 — the per-item × per-mount
//! zone of the E118 foundation follow-up): the committed
//! `for_event_item_branch` unit — the same artifact the parity gate
//! byte-pins on both frontend legs — built on the mock host. A `Show`
//! inside a keyed For body mounts PER ITEM (the item's own fragment carries
//! the branch slot; one item's then leg and one item's else leg mount side
//! by side), each leg's handler attaches at the mount through the item's
//! slab, and dispatch rides the index-addressed leg fire fns with the live
//! item substituted. Pre-M27 the shape denied with `LIG-E118` at the
//! handler's own span (fail-closed — per-item control flow was the named
//! foundation follow-up).
//!
//! Run without touching `dist/`:
//!
//! ```sh
//! cargo run -p compile-clean --example for_event_item_branch
//! ```

use compile_clean::for_event_item_branch;
use lig_runtime::EvData;
use mock_host::{Commit, MockHost};

fn main() {
    let mut host = MockHost::new();

    // 1. Build — the initial flush runs the keyed reconcile: two items
    //    mount, and the per-item replay evaluates each item's condition
    //    (`alpha` is 5 chars > 4, `beta` is 4): item 0's then leg mounts,
    //    item 1's else leg mounts — one branch slot, two item-scoped leg
    //    instances — and each leg's handler attaches at its mount.
    let mut app = for_event_item_branch::generated::Rows::build(
        &mut host,
        for_event_item_branch::generated::RowsProps::default(),
    );
    let then_node = app
        .frag
        .list_item(0, 0)
        .and_then(|f| f.branch(0))
        .map(|b| b.hole(0))
        .expect("item 0's then leg mounted");
    let _else_node = app
        .frag
        .list_item(0, 1)
        .and_then(|f| f.branch(0))
        .map(|b| b.hole(0))
        .expect("item 1's else leg mounted");
    let attaches = host
        .commits
        .iter()
        .filter(|c| matches!(c, Commit::Attach { .. }))
        .count();
    assert_eq!(attaches, 2, "one attach per item, at the branch mount");
    println!(
        "mount: alpha → then leg, beta → else leg, 2 attaches (one per item) \
         — canonical ids: then leg = f3, else leg = f2"
    );

    // 2. Dispatch — the leg fire fns address the LIVE item; the handler
    //    body substitutes item/index exactly like the M15 body fire fns.
    app.fire_f3_d0(&mut host, 0, &EvData::new())
        .expect("the dispatch converges");
    assert_eq!(app.picked, "alpha", "the then leg's handler picked alpha");
    app.fire_f2_d0(&mut host, 1, &EvData::new())
        .expect("the dispatch converges");
    assert_eq!(app.picked, "short", "the else leg's handler picked short");
    println!("dispatch: fire_f3(0) → alpha, fire_f2(1) → short");

    // 3. Remove — the keyed plan's remove tears the item's slab down (its
    //    leg and its handler record go with it) and mirrors the per-item leg
    //    tracker; a stale index stays a no-op.
    app.set_items(vec!["alpha".into()]);
    lig_runtime::flush(&mut app, &mut host);
    assert_eq!(app.frag.list_len(0), 1, "beta removed with its else leg");
    app.fire_f2_d0(&mut host, 1, &EvData::new())
        .expect("the dispatch converges");
    assert_eq!(app.picked, "short", "a stale index is a no-op, not a panic");
    println!("remove: beta's else leg disposed, stale dispatch is a no-op");

    // 4. Fresh mount — a new item mounts a FRESH leg with a fresh attach
    //    (never the disposed node), and its handler dispatches the live
    //    item through the same fire fn.
    app.set_items(vec!["alpha".into(), "longest".into()]);
    lig_runtime::flush(&mut app, &mut host);
    let fresh = app
        .frag
        .list_item(0, 1)
        .and_then(|f| f.branch(0))
        .map(|b| b.hole(0))
        .expect("the fresh item's then leg mounted");
    assert_ne!(fresh, then_node, "the fresh leg is a fresh mount");
    app.fire_f3_d0(&mut host, 1, &EvData::new())
        .expect("the dispatch converges");
    assert_eq!(
        app.picked, "longest",
        "the fresh leg dispatches the live item"
    );
    println!("remount: longest → fresh then leg, fire_f3(1) → longest");
}
