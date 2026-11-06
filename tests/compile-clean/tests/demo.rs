//! The KR-1 demo (§17.1): Solid TSX → generated Rust renders on the mock
//! host and updates through the const schedule. Every assertion here is a
//! compile-clean fixture run end-to-end.

use compile_clean::{
    component_use, counter, dynamic_style, effect_multi, event_object, for_keyed, for_list,
    slots_named, v_model,
};
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

/// The §7.4 slot contract, end to end (M6): the parent's build mounts each
/// use site's fill into the child's outlet holes by name — and a use site
/// that does not fill a slot mounts the outlet's own fallback content.
/// Items are filled, not recreated: both Panel instances render in place.
#[test]
fn slots_named_mount_fills_and_fallbacks_per_use_site() {
    let mut h = MockHost::new();
    let app = slots_named::App::build(&mut h, slots_named::AppProps::default());
    // Use 1 fills the header (dynamic 1) and the default outlet (dynamic 2).
    assert!(
        app.panel.frag.slot_is(1),
        "use 1: header slot filled from the element-valued prop"
    );
    assert!(app.panel.frag.slot_is(2), "use 1: default slot filled");
    // Use 2 fills only the default outlet; the header outlet shows the
    // child's own fallback fragment (mounted from the child's statics).
    assert!(
        app.panel_2.frag.slot_is(1),
        "use 2: header slot shows the outlet's fallback content"
    );
    assert!(app.panel_2.frag.slot_is(2), "use 2: default slot filled");
    assert_eq!(app.panel.title, "Status", "props ride the use site");
    assert_eq!(app.panel_2.title, "Bare");
}

/// The §7.6 v-model sugar, end to end: the input event writes the
/// synthesized setter through the payload, and the value attr replays.
#[test]
fn v_model_writes_through_the_event_payload() {
    let mut h = MockHost::new();
    let mut s = v_model::Signup::build(&mut h, v_model::SignupProps::default());
    assert_eq!(s.email, "", "initial email");
    let ev = EvData::new().current_target_value("me@ligature.dev");
    s.fire_1(&mut h, &ev);
    assert_eq!(s.email, "me@ligature.dev", "v-model wrote the setter");
}

/// The §7.1 multi-statement effect contract (M6): each statement of a block
/// body lowers to its own effect reaction, and a single setTick flush runs
/// both in schedule order.
#[test]
fn effect_multi_runs_every_statement_as_its_own_reaction() {
    let mut h = MockHost::new();
    let mut s = effect_multi::Sync::build(&mut h, effect_multi::SyncProps::default());
    // The build-time init flush runs every effect once: log = "0", count = 0.
    assert_eq!(s.log, "0", "init flush ran both statements");
    assert_eq!(s.count, 0);
    s.set_tick(7);
    lig_runtime::flush(&mut s, &mut h);
    assert_eq!(s.log, "7", "statement 1: log tracks the tick");
    assert_eq!(s.count, 14, "statement 2: count ran in the same flush");
    s.set_tick(9);
    lig_runtime::flush(&mut s, &mut h);
    assert_eq!(s.log, "9");
    assert_eq!(s.count, 18, "both reactions re-run per flush");
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
