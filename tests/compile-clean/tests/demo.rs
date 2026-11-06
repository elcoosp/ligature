//! The KR-1 demo (§17.1): Solid TSX → generated Rust renders on the mock
//! host and updates through the const schedule. Every assertion here is a
//! compile-clean fixture run end-to-end.

use compile_clean::{component_use, counter, dynamic_style, event_object, for_keyed, for_list};
use lig_runtime::{EvData, key_hash};
use mock_host::MockHost;

#[test]
fn counter_renders_and_updates_through_the_schedule() {
    let mut h = MockHost::new();
    let mut c = counter::Counter::build(&mut h, counter::CounterProps::default());
    // initial render: count 0
    let span = *c.frag.roots().first().expect("root");
    let _ = span;
    assert_eq!(c.count, 0, "initial signal value");
    // click the Add button twice: count 0 → 2
    c.fire_1(&mut h, &EvData::new());
    c.fire_1(&mut h, &EvData::new());
    assert_eq!(c.count, 2, "setter path incremented twice");
    assert_eq!(c.doubled, 4, "memo tracks the signal");
}

#[test]
fn for_list_mounts_and_reconciles_items() {
    let mut h = MockHost::new();
    let mut app = for_list::Names::build(&mut h, for_list::NamesProps::default());
    assert_eq!(app.frag.list_len(0), 3, "alpha/beta/gamma mounted");
    app.set_names(vec![
        "alpha".into(),
        "delta".into(),
        "gamma".into(),
        "epsilon".into(),
    ]);
    lig_runtime::flush(&mut app, &mut h);
    assert_eq!(app.frag.list_len(0), 4, "unkeyed index diff after set");
    app.set_names(vec!["alpha".into()]);
    lig_runtime::flush(&mut app, &mut h);
    assert_eq!(app.frag.list_len(0), 1, "tails removed");
}

/// The §7.5 keyed contract, end to end: a reorder moves the existing item
/// fragments (LIS-stable shape — one host `Move`, no `Create`/`Destroy`
/// churn), keys track the new sequence, and the text binds replay.
#[test]
fn for_keyed_reorders_without_recreating_items() {
    let mut h = MockHost::new();
    let mut app = for_keyed::Tags::build(&mut h, for_keyed::TagsProps::default());
    assert_eq!(app.frag.list_len(0), 3, "alpha/beta/gamma mounted");
    let creates_before = h.count_kind("Create");
    // Rotate the tail to the front: [gamma, alpha, beta].
    app.set_tags(vec!["gamma".into(), "alpha".into(), "beta".into()]);
    lig_runtime::flush(&mut app, &mut h);
    assert_eq!(
        h.count_kind("Create"),
        creates_before,
        "keyed reorder creates nothing"
    );
    assert_eq!(h.count_kind("Destroy"), 0, "keyed reorder destroys nothing");
    assert_eq!(h.count_kind("Move"), 1, "rotation is one LIS-stable move");
    let expected = ["gamma", "alpha", "beta"]
        .iter()
        .map(|s| key_hash(s))
        .collect::<Vec<_>>();
    for (i, k) in expected.iter().enumerate() {
        assert_eq!(
            app.frag.list_key(0, i),
            Some(*k),
            "key order tracks position {i}"
        );
    }
    // Removal + insertion still route through the keyed diff.
    app.set_tags(vec!["alpha".into(), "delta".into()]);
    lig_runtime::flush(&mut app, &mut h);
    assert_eq!(app.frag.list_len(0), 2, "one kept, one inserted");
    let expected = ["alpha", "delta"]
        .iter()
        .map(|s| key_hash(s))
        .collect::<Vec<_>>();
    for (i, k) in expected.iter().enumerate() {
        assert_eq!(
            app.frag.list_key(0, i),
            Some(*k),
            "key order after diff at {i}"
        );
    }
}

#[test]
fn event_object_flows_through_the_payload_slot() {
    let mut h = MockHost::new();
    let mut s = event_object::Search::build(&mut h, event_object::SearchProps::default());
    assert_eq!(s.query, "", "initial query");
    let ev = EvData::new().current_target_value("ligature");
    s.fire_1(&mut h, &ev);
    assert_eq!(s.query, "ligature", "handler read e.currentTarget.value");
}

#[test]
fn component_use_builds_child_and_passes_props() {
    let mut h = MockHost::new();
    let app = component_use::App::build(&mut h, component_use::AppProps::default());
    assert_eq!(app.panel.title, "Status", "build-time prop pass");
    assert!(
        app.panel.frag.slot_is(1),
        "children slot filled (dynamic 1)"
    );
}

#[test]
fn dynamic_style_prev_diffs_property_updates() {
    let mut h = MockHost::new();
    let mut s = dynamic_style::Swatch::build(&mut h, dynamic_style::SwatchProps::default());
    assert_eq!(s.color, "crimson", "initial color");
    assert_eq!(h.count_kind("SetStyleDyn"), 1, "initial flush commits once");
    // Same value twice: the generated prev diff (p_0) must suppress the
    // second commit entirely — no SetStyleDyn reaches the host.
    s.set_color("crimson".into());
    lig_runtime::flush(&mut s, &mut h);
    assert_eq!(
        h.count_kind("SetStyleDyn"),
        1,
        "unchanged value suppressed by the prev diff"
    );
    // A changed value commits exactly once more.
    s.set_color("navy".into());
    lig_runtime::flush(&mut s, &mut h);
    assert_eq!(
        h.count_kind("SetStyleDyn"),
        2,
        "changed value commits exactly once"
    );
}
