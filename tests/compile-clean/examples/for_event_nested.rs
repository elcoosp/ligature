//! Nested-list events, walked end to end (M28 — the per-item × per-item
//! zone of the E118 foundation follow-up): the committed `for_event_nested`
//! unit — the same artifact the parity gate byte-pins on both frontend
//! legs — built on the mock host. A keyed `For` inside a keyed For body
//! mounts PER OUTER ITEM (each item is its own fragment with its own list
//! table — the per-item-square state is the fragment-per-item itself), the
//! inner binds apply with the inner item substituted, each inner handler
//! attaches per fresh inner item, and dispatch rides the index-PAIR-addressed
//! fire fn (outer index, inner index). Pre-M28 the shape denied with
//! `LIG-E118` at the handler's own span (fail-closed — per-item × per-item
//! composition was the named foundation follow-up).
//!
//! Run without touching `dist/`:
//!
//! ```sh
//! cargo run -p compile-clean --example for_event_nested
//! ```

use compile_clean::for_event_nested;
use lig_runtime::EvData;
use mock_host::{Commit, MockHost};

fn main() {
    let mut host = MockHost::new();

    // 1. Build — the initial flush runs the outer keyed reconcile: one
    //    group mounts, and the per-item replay calls the inner reconcile on
    //    the ITEM's own fragment (its list table hangs off the item), whose
    //    keyed plan mounts alpha and beta — each inner handler attaches per
    //    fresh inner item.
    let mut app = for_event_nested::generated::Groups::build(
        &mut host,
        for_event_nested::generated::GroupsProps::default(),
    );
    let outer = app.frag.list_item(0, 0).expect("the outer item mounted");
    assert_eq!(
        outer.list_len(0),
        2,
        "alpha/beta mounted on the item's own table"
    );
    let attaches = host
        .commits
        .iter()
        .filter(|c| matches!(c, Commit::Attach { .. }))
        .count();
    assert_eq!(attaches, 2, "one attach per fresh inner item");
    println!(
        "mount: one group → one outer item; its own fragment carries the \
         inner table (alpha, beta), 2 attaches (one per inner item)"
    );

    // 2. Dispatch — the inner fire fn addresses the LIVE pair through both
    //    sources: the outer source re-evaluated by outer index, the inner
    //    source re-evaluated through the live outer item. A stale pair
    //    returns as a no-op, never a panic.
    app.fire_f2_d0(&mut host, 0, 1, &EvData::new())
        .expect("the dispatch converges");
    assert_eq!(app.picked, "beta", "the pair (0, 1) dispatched beta");
    app.fire_f2_d0(&mut host, 0, 0, &EvData::new())
        .expect("the dispatch converges");
    assert_eq!(app.picked, "alpha", "the pair (0, 0) dispatched alpha");
    app.fire_f2_d0(&mut host, 3, 0, &EvData::new())
        .expect("the dispatch converges");
    assert_eq!(app.picked, "alpha", "a stale outer index is a no-op");
    println!("dispatch: fire_f2(0, 1) → beta, fire_f2(0, 0) → alpha, stale pair → no-op");

    // 3. Square fan-out — a fresh outer item mounts its own inner list with
    //    its own attach records (one reaction, N×M live items — the M14
    //    flush-scope discipline squared); each item's handler dispatches the
    //    live pair through the same fire fn.
    app.set_groups(vec![
        vec!["alpha".into(), "beta".into()],
        vec!["gamma".into()],
    ]);
    lig_runtime::flush(&mut app, &mut host);
    let second = app
        .frag
        .list_item(0, 1)
        .expect("the fresh outer item mounted");
    assert_eq!(
        second.list_len(0),
        1,
        "gamma mounted on the fresh item's table"
    );
    app.fire_f2_d0(&mut host, 1, 0, &EvData::new())
        .expect("the dispatch converges");
    assert_eq!(app.picked, "gamma", "the fresh item's handler dispatched");
    println!("fan-out: second group mounts its own inner list, fire_f2(1, 0) → gamma");

    // 4. Keyed inner move — the inner plan repositions the SAME instances
    //    (LIS-stable: no remount), and the outer removal disposes the whole
    //    inner table with the item (the item teardown is recursive).
    let beta_node = app
        .frag
        .list_item(0, 0)
        .and_then(|f| f.list_item(0, 1))
        .map(|f| f.hole(1))
        .expect("beta's node");
    app.set_groups(vec![
        vec!["beta".into(), "alpha".into()],
        vec!["gamma".into()],
    ]);
    lig_runtime::flush(&mut app, &mut host);
    let moved = app
        .frag
        .list_item(0, 0)
        .and_then(|f| f.list_item(0, 0))
        .map(|f| f.hole(1))
        .expect("the moved item's fragment");
    assert_eq!(
        moved, beta_node,
        "the keyed inner move repositions the same fragment"
    );
    app.set_groups(vec![]);
    lig_runtime::flush(&mut app, &mut host);
    assert_eq!(
        app.frag.list_len(0),
        0,
        "the removal disposed the inner tables"
    );
    println!("move + teardown: beta repositioned 1 → 0 (same fragment), removal disposes all");

    // 5. Fresh remount — a new outer item re-mounts its inner list and
    //    re-attaches exactly once (never the disposed nodes), and dispatch
    //    rides the same index pair.
    app.set_groups(vec![vec!["again".into()]]);
    lig_runtime::flush(&mut app, &mut host);
    app.fire_f2_d0(&mut host, 0, 0, &EvData::new())
        .expect("the dispatch converges");
    assert_eq!(
        app.picked, "again",
        "the fresh item's handler dispatches the live pair"
    );
    println!("remount: fresh inner list mounts, fire_f2(0, 0) → again");
}
