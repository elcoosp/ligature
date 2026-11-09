//! The KR-1 demo (§17.1): Solid TSX → generated Rust renders on the mock
//! host and updates through the const schedule. Every assertion here is a
//! compile-clean fixture run end-to-end.

use compile_clean::{
    component_use, counter, dynamic_style, effect_multi, event_object, for_composed_content,
    for_event, for_event_composed, for_keyed, for_list, list_component, list_slot, show_else,
    slots_branch_component, slots_branch_outlet, slots_component, slots_dynamic, slots_named,
    slots_nested, slots_structural, template_ref, v_model,
};
use lig_runtime::{EvData, SlotOrigin, key_hash};
use mock_host::{Commit, MockHost};

/// The M8 structural-slot-content contract, end to end: a fill carries an If
/// branch (its bind initializes at build AND re-initializes on every late
/// mount through the mount-time marking) and a keyed For (the reconcile
/// addresses the content's own list table through the composed walk); the
/// unfilled outlet mounts the child's structural fallback, whose branch
/// initializes through the parent-driven init-flush.
#[test]
fn slots_structural_branches_and_lists_address_their_content() {
    let mut h = MockHost::new();
    let mut app = slots_structural::App::build(&mut h, slots_structural::AppProps::default());
    // The fill's branch mounted at build (on = true), its bind initialized —
    // the branch lives on the CONTENT's branch table.
    let fill = app
        .panel
        .frag
        .slot_fill(1)
        .expect("fill mounted by the parent's build");
    assert!(fill.branch_is(0), "content branch mounted at build");
    assert_eq!(
        h.text_of(fill.branch(0).expect("mounted").hole(0)),
        Some("live"),
        "branch bind initialized at build"
    );
    // The keyed list lives on the content's own list table.
    assert_eq!(fill.list_len(1), 2, "alpha/beta mounted on the content");
    // Late mount: unmount (on = false), rewrite the label while absent, and
    // flip back — the fresh branch's bind re-initializes through the
    // mount-time marking (the flush re-enters via the union).
    app.set_on(false);
    lig_runtime::flush(&mut app, &mut h);
    assert!(
        !app.panel.frag.slot_fill(1).expect("fill").branch_is(0),
        "branch unmounted"
    );
    app.set_label("next".into());
    lig_runtime::flush(&mut app, &mut h);
    app.set_on(true);
    lig_runtime::flush(&mut app, &mut h);
    let fill = app.panel.frag.slot_fill(1).expect("fill");
    assert!(fill.branch_is(0), "branch remounted");
    assert_eq!(
        h.text_of(fill.branch(0).expect("mounted").hole(0)),
        Some("next"),
        "remounted branch re-initializes through the mount marking"
    );
    // Keyed reconcile inside the content: rotate one in, one out.
    app.set_rows(vec!["beta".into(), "gamma".into()]);
    lig_runtime::flush(&mut app, &mut h);
    let fill = app.panel.frag.slot_fill(1).expect("fill");
    assert_eq!(fill.list_len(1), 2, "keyed diff ran on the content's table");
    let expected = ["beta", "gamma"]
        .iter()
        .map(|s| key_hash(s))
        .collect::<Vec<_>>();
    for (i, k) in expected.iter().enumerate() {
        assert_eq!(
            fill.list_key(1, i),
            Some(*k),
            "content list key order tracks the diff at {i}"
        );
    }
    // The child's structural fallback: its branch mounted and its bind
    // initialized by the parent-driven init-flush (title == "Status" → on).
    let fb = app
        .panel
        .frag
        .slot_fallback(2)
        .expect("fallback mounted by the parent's build");
    assert!(fb.branch_is(0), "fallback branch mounted by the init-flush");
    assert_eq!(
        h.text_of(fb.branch(0).expect("mounted").hole(0)),
        Some("Status"),
        "fallback branch bind initialized by the init-flush"
    );
}

/// The M7 dynamic-slot-content contract, end to end: a fill carries a live
/// text bind + a delegated event (the handler record lands on the parent's
/// fragment across slabs), and the fallback mounts with its bind
/// initialized by the parent-driven init-flush. One addressing discipline:
/// every slot-content write goes through the mounted content.
#[test]
fn slots_dynamic_binds_and_events_address_their_content() {
    let mut h = MockHost::new();
    let mut app = slots_dynamic::App::build(&mut h, slots_dynamic::AppProps::default());
    // Origins per outlet: use 1 fills the header; the foot outlet shows the
    // child's own fallback.
    assert_eq!(
        app.panel.frag.slot_origin(1),
        Some(SlotOrigin::Fill),
        "header outlet carries the use-site fill"
    );
    assert_eq!(
        app.panel.frag.slot_origin(2),
        Some(SlotOrigin::Fallback),
        "foot outlet carries the outlet fallback"
    );
    // The fallback bind ran at mount time (the parent's init-flush), not at
    // the child's build — its text shows the prop through the content.
    let fb = app
        .panel
        .frag
        .slot_fallback(2)
        .expect("fallback mounted by the parent's build");
    assert_eq!(
        h.text_of(fb.hole(0)),
        Some("Status"),
        "fallback bind initialized by the init-flush"
    );
    // The fill's text bind ran at the parent's build: content live, "live".
    let fill = app
        .panel
        .frag
        .slot_fill(1)
        .expect("fill mounted by the parent's build");
    assert_eq!(
        h.text_of(fill.hole(1)),
        Some("live"),
        "fill text bind initialized at build"
    );
    // Dispatch the fill's click: the record lives on the parent's fragment
    // (attach_foreign across slabs); the flush rewrites the content text.
    let fill_text = fill.hole(1);
    app.fire_1(&mut h, &EvData::new());
    assert_eq!(app.label, "next", "handler wrote the setter");
    assert_eq!(
        h.text_of(fill_text),
        Some("next"),
        "slot-scoped bind rewrote the content after the event"
    );
}

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

/// The M9 grandchild prop-pass contract, end to end: a component use inside
/// slot content builds as the content author's struct field, its DOM root
/// inserts into the mounted content's component hole at mount time, and the
/// prop-pass run arm (the content author's bind reaction) writes the
/// grandchild's setters and flushes the instance — a signal update flows
/// through the mounted content into the grandchild's own template.
#[test]
fn content_component_inserts_and_passes_props_through_the_mount() {
    let mut h = MockHost::new();
    let mut app = slots_component::App::build(&mut h, slots_component::AppProps::default());
    // The grandchild inserted into the header fill's component hole at
    // mount time; its prop initialized by the initial flush (which runs
    // after slot assembly).
    assert!(
        app.panel.frag.slot_fill(1).is_some(),
        "header fill mounted by the parent's build"
    );
    assert_eq!(
        h.text_of(app.badge.frag.hole(0)),
        Some("two"),
        "grandchild prop initialized through the initial flush"
    );
    // The signal update flows: App flush → prop-pass arm → grandchild flush
    // → the grandchild's own text bind.
    app.set_n("three".into());
    lig_runtime::flush(&mut app, &mut h);
    assert_eq!(app.badge.count, "three", "prop-pass wrote the instance");
    assert_eq!(
        h.text_of(app.badge.frag.hole(0)),
        Some("three"),
        "the grandchild's bind re-rendered the new prop"
    );
    // The default fill mounted on the child's default outlet.
    assert_eq!(
        app.panel.frag.slot_origin(2),
        Some(SlotOrigin::Fill),
        "default outlet carries the element-children fill"
    );
}

/// The M9 nested-outlet contract, end to end: a nested outlet on slot
/// content mounts its use-site fill (composed `slot_fill_mut` →
/// `slot_fill_mut` walk) or — when the use site does not fill it — its own
/// fallback (composed `slot_fill_mut` → `slot_fallback_mut`); binds on both
/// initialize at build through the initial flush and track updates through
/// the composed walks.
#[test]
fn nested_outlets_mount_fills_and_fallbacks_through_the_composed_walk() {
    let mut h = MockHost::new();
    let mut app = slots_nested::App::build(&mut h, slots_nested::AppProps::default());
    // Use 1 fills the nested outlet: the fill mounts ON the header fill's
    // own slot table, and its bind initialized ("in").
    let fill = app
        .panel
        .frag
        .slot_fill(1)
        .expect("header fill mounted (use 1)");
    assert!(
        fill.slot_is(0),
        "the nested outlet carries the use-site fill"
    );
    let inner = fill.slot_fill(0).expect("nested fill mounted");
    assert_eq!(
        h.text_of(inner.hole(0)),
        Some("in"),
        "nested fill bind initialized through the initial flush"
    );
    // Use 2 does not fill `inner`: the outlet's own fallback mounts.
    let fill2 = app
        .panel_2
        .frag
        .slot_fill(1)
        .expect("header fill mounted (use 2)");
    assert!(
        fill2.slot_is(0),
        "the nested outlet carries its own fallback"
    );
    let fb2 = fill2.slot_fallback(0).expect("nested fallback mounted");
    assert_eq!(
        h.text_of(fb2.hole(0)),
        Some("in"),
        "nested fallback bind initialized through the initial flush"
    );
    // Updates flow through the composed walks: fill bind (content → nested
    // fill) and fallback bind (content → nested fallback).
    app.set_tag("next".into());
    lig_runtime::flush(&mut app, &mut h);
    let fill = app.panel.frag.slot_fill(1).expect("fill");
    assert_eq!(
        h.text_of(fill.slot_fill(0).expect("nested fill").hole(0)),
        Some("next"),
        "the nested fill bind walked the composed chain"
    );
    let fill2 = app.panel_2.frag.slot_fill(1).expect("fill2");
    assert_eq!(
        h.text_of(fill2.slot_fallback(0).expect("nested fallback").hole(0)),
        Some("next"),
        "the nested fallback bind walked the composed chain"
    );
    // The origin gates hold: use 1's outlet is FILLED, so the fallback
    // accessor sees nothing — a fallback write can never cross into a fill.
    assert!(
        app.panel
            .frag
            .slot_fill(1)
            .unwrap()
            .slot_fallback(0)
            .is_none()
    );
}

/// The M10 per-mount instance identity, end to end: a component use inside a
/// branch body of slot content builds as an Option field; the mounting arm
/// rebuilds the instance, inserts its root into the branch's component hole,
/// and the mount marking initializes the fresh instance's prop-pass — a
/// signal written while the branch was absent lands on the NEXT mount. The
/// unmount arm disposes the instance (teardown + None), so a remount cycle
/// never reuses a stale instance or leaks its slab.
#[test]
fn branch_components_rebuild_and_dispose_per_mount() {
    let mut h = MockHost::new();
    let mut app =
        slots_branch_component::App::build(&mut h, slots_branch_component::AppProps::default());
    // The badge built inside the initial flush's mount arm, its root
    // inserted into the branch's component hole, its prop initialized
    // through the mount marking (initial flush, second pass).
    let badge = app.badge.as_ref().expect("instance built at mount");
    assert_eq!(badge.count, "two", "prop initialized on the fresh instance");
    assert_eq!(
        h.text_of(badge.frag.hole(0)),
        Some("two"),
        "the grandchild's bind re-rendered the prop"
    );
    // The toggle unmounts the branch AND disposes the instance.
    app.set_on(false);
    lig_runtime::flush(&mut app, &mut h);
    assert!(
        app.badge.is_none(),
        "the unmount arm disposed the per-mount instance"
    );
    // A signal write while the branch is absent must not touch anything:
    // the prop-pass arm guards on the Option field.
    app.set_n("three".into());
    lig_runtime::flush(&mut app, &mut h);
    assert!(app.badge.is_none(), "no instance to write while absent");
    // The remount builds a FRESH instance and the mount marking re-runs the
    // prop-pass — the write made while absent lands on it.
    app.set_on(true);
    lig_runtime::flush(&mut app, &mut h);
    let badge = app.badge.as_ref().expect("fresh instance at remount");
    assert_eq!(
        badge.count, "three",
        "the late write landed on the fresh instance"
    );
    assert_eq!(
        h.text_of(badge.frag.hole(0)),
        Some("three"),
        "the fresh instance's bind re-rendered"
    );
}

/// The M13 per-item instance identity, end to end: a component use inside a
/// keyed For body builds one instance per live item — the keyed plan's
/// inserts build + attach into the item's component hole, removes take +
/// dispose the instance's slab, keyed moves reposition the SAME instance
/// (no rebuild — identity survives reorders), and the per-item replay drives
/// each instance's props.
#[test]
fn list_components_carry_per_item_identity() {
    let mut h = MockHost::new();
    let mut app = list_component::App::build(&mut h, list_component::AppProps::default());
    // Two items mounted on the initial flush → two instances, each attached
    // in its item's component hole with its prop bound by the replay.
    assert_eq!(app.badge.len(), 2, "one instance per live item");
    let alpha = app.badge[0].as_ref().expect("row 0 instance");
    let beta = app.badge[1].as_ref().expect("row 1 instance");
    assert_eq!(alpha.count, "alpha", "props initialized through the replay");
    assert_eq!(beta.count, "beta");
    let alpha_node = alpha.frag.hole(0);
    let beta_node = beta.frag.hole(0);
    assert_eq!(h.text_of(alpha_node), Some("alpha"), "the child renders");
    assert_eq!(h.text_of(beta_node), Some("beta"));

    // A keyed rotate ([beta, alpha]) MOVES the instances without rebuilding
    // them: the same node handles reappear at the new positions (a rebuild
    // would carry fresh instance-seq NodeIds).
    app.set_rows(vec!["beta".into(), "alpha".into()]);
    lig_runtime::flush(&mut app, &mut h);
    assert_eq!(app.badge.len(), 2, "a keyed move rebuilds nothing");
    assert_eq!(
        app.badge[0].as_ref().expect("moved beta").frag.hole(0),
        beta_node,
        "the moved item kept its instance"
    );
    assert_eq!(
        app.badge[1].as_ref().expect("stable alpha").frag.hole(0),
        alpha_node,
        "the LIS-stable item kept its instance"
    );
    assert_eq!(
        h.text_of(beta_node),
        Some("beta"),
        "the moved instance still renders at its new position"
    );

    // A removal takes + disposes the instance (the store shrinks with the
    // list, and the removed instance's node is gone from the host).
    app.set_rows(vec!["beta".into()]);
    lig_runtime::flush(&mut app, &mut h);
    assert_eq!(app.badge.len(), 1, "the removed item's instance is gone");
    assert_eq!(
        app.badge[0].as_ref().expect("beta survives").frag.hole(0),
        beta_node,
        "the surviving item kept its instance"
    );

    // A fresh key builds a FRESH instance (new slab, new node handle) with
    // its prop bound by the replay.
    app.set_rows(vec!["beta".into(), "gamma".into()]);
    lig_runtime::flush(&mut app, &mut h);
    let gamma = app.badge[1].as_ref().expect("fresh gamma instance");
    assert_eq!(gamma.count, "gamma", "the fresh instance's prop is bound");
    assert_ne!(
        gamma.frag.hole(0),
        alpha_node,
        "a new key builds a new instance, not the disposed one"
    );
    assert_eq!(h.text_of(gamma.frag.hole(0)), Some("gamma"));
}

/// The M14 per-item slot assembly, end to end: a slot outlet DIRECTLY on a
/// keyed For body mounts its own fallback into EVERY live item's outlet
/// hole (the per-item replay mounts idempotently and drives the fallback's
/// item-bound bind) — keyed rotations move the SAME mounted content without
/// rebuilding it (the item's `Fragment` carries its slot table), removals
/// dispose the content with the item (teardown recurses into slots), and
/// fresh keys build fresh content.
#[test]
fn list_slots_mount_their_fallback_per_item() {
    let mut h = MockHost::new();
    let mut app = list_slot::App::build(&mut h, list_slot::AppProps::default());
    // Two items → two mounted fallbacks, each bound to its item by the replay.
    assert_eq!(app.frag.list_len(0), 2, "alpha/beta mounted");
    let alpha_fb = app
        .frag
        .list_item(0, 0)
        .and_then(|f| f.slot_fallback(0))
        .expect("row 0 fallback mounted");
    let beta_fb = app
        .frag
        .list_item(0, 1)
        .and_then(|f| f.slot_fallback(0))
        .expect("row 1 fallback mounted");
    let alpha_node = alpha_fb.hole(0);
    let beta_node = beta_fb.hole(0);
    assert_eq!(
        h.text_of(alpha_node),
        Some("alpha"),
        "the fallback bind initialized through the replay"
    );
    assert_eq!(h.text_of(beta_node), Some("beta"));

    // A keyed rotate MOVES the mounted content without rebuilding it: the
    // same node handles reappear at the new positions (a rebuild would
    // carry fresh instance-seq NodeIds).
    app.set_rows(vec!["beta".into(), "alpha".into()]);
    lig_runtime::flush(&mut app, &mut h);
    assert_eq!(app.frag.list_len(0), 2, "a keyed move rebuilds nothing");
    let moved_beta = app
        .frag
        .list_item(0, 0)
        .and_then(|f| f.slot_fallback(0))
        .expect("beta at 0");
    let stable_alpha = app
        .frag
        .list_item(0, 1)
        .and_then(|f| f.slot_fallback(0))
        .expect("alpha at 1");
    assert_eq!(
        moved_beta.hole(0),
        beta_node,
        "the moved item kept its mounted content"
    );
    assert_eq!(
        stable_alpha.hole(0),
        alpha_node,
        "the LIS-stable item kept its content"
    );
    assert_eq!(
        h.text_of(beta_node),
        Some("beta"),
        "the moved content still renders at its new position"
    );

    // A removal disposes the item AND its mounted content: teardown
    // recurses into the item's slot table, so the removed content's nodes
    // leave the host.
    app.set_rows(vec!["beta".into()]);
    lig_runtime::flush(&mut app, &mut h);
    assert_eq!(app.frag.list_len(0), 1, "the removed item is gone");
    assert!(
        h.commits
            .iter()
            .any(|c| matches!(c, Commit::Destroy { node } if *node == alpha_node)),
        "the removed item's mounted content was torn down"
    );

    // A fresh key builds FRESH content (a new node handle) with its bind
    // driven by the replay.
    app.set_rows(vec!["beta".into(), "gamma".into()]);
    lig_runtime::flush(&mut app, &mut h);
    let fresh = app
        .frag
        .list_item(0, 1)
        .and_then(|f| f.slot_fallback(0))
        .expect("fresh gamma content");
    assert_ne!(
        fresh.hole(0),
        alpha_node,
        "a new key builds new content, not the disposed one"
    );
    assert_eq!(h.text_of(fresh.hole(0)), Some("gamma"));
}

/// The M10 branch-time slot assembly, end to end: an outlet inside a branch
/// body of slot content mounts its use-site fill (composed
/// `slot_fill_mut` → `branch_mut` → `slot_fill_mut`) or — when the use site
/// does not fill it — its own fallback (`… → slot_fallback_mut`) at
/// BRANCH-MOUNT time, and the mounted content's binds initialize through the
/// mount marking and track updates through the composed walks.
#[test]
fn branch_outlets_mount_fills_and_fallbacks_with_the_branch() {
    let mut h = MockHost::new();
    let mut app = slots_branch_outlet::App::build(&mut h, slots_branch_outlet::AppProps::default());
    // Use 1 fills the branch-internal outlet: the fill mounts ON the mounted
    // branch, its bind initialized through the mount marking.
    let fill1 = app
        .panel
        .frag
        .slot_fill(1)
        .expect("header fill mounted (use 1)");
    let branch1 = fill1.branch(0).expect("the content branch mounted");
    assert!(
        branch1.slot_is(0),
        "the branch-internal outlet carries the use-site fill"
    );
    let inner = branch1.slot_fill(0).expect("branch fill mounted");
    assert_eq!(
        h.text_of(inner.hole(0)),
        Some("in"),
        "the branch fill bind initialized at branch mount"
    );
    // Use 2 does not fill `inner`: the outlet's own fallback mounts with the
    // branch.
    let fill2 = app
        .panel_2
        .frag
        .slot_fill(1)
        .expect("header fill mounted (use 2)");
    let branch2 = fill2.branch(0).expect("the content branch mounted");
    let fb2 = branch2.slot_fallback(0).expect("branch fallback mounted");
    assert_eq!(
        h.text_of(fb2.hole(0)),
        Some("in"),
        "the branch fallback bind initialized at branch mount"
    );
    // Updates flow through the composed walks (content → branch → fill /
    // fallback).
    app.set_tag("next".into());
    lig_runtime::flush(&mut app, &mut h);
    let branch1 = app
        .panel
        .frag
        .slot_fill(1)
        .and_then(|f| f.branch(0))
        .expect("branch 1");
    assert_eq!(
        h.text_of(branch1.slot_fill(0).expect("branch fill").hole(0)),
        Some("next"),
        "the branch fill bind walked the composed chain"
    );
    let branch2 = app
        .panel_2
        .frag
        .slot_fill(1)
        .and_then(|f| f.branch(0))
        .expect("branch 2");
    assert_eq!(
        h.text_of(branch2.slot_fallback(0).expect("branch fb").hole(0)),
        Some("next"),
        "the branch fallback bind walked the composed chain"
    );
    // Unmounting the branch takes the mounted content with it.
    app.set_on(false);
    lig_runtime::flush(&mut app, &mut h);
    let fill1 = app.panel.frag.slot_fill(1).expect("header fill stays");
    assert!(
        fill1.branch(0).is_none(),
        "the branch unmounted with its fills"
    );
}

/// The M12 template-ref contract, end to end: the Vue-leg spelling (the
/// fixture is the two-leg `template-ref` pair) rides the same Ref machinery
/// as the Solid leg — the placeholder declares no state, and the build body
/// captures the element's hole `NodeId` into the `ref_<name>` field.
#[test]
fn template_ref_captures_the_element_node() {
    let mut h = MockHost::new();
    let app = template_ref::Marked::build(&mut h, template_ref::MarkedProps::default());
    // The capture ran in the build body (Ctx::Direct): the field names the
    // mounted <input> node after build — not the `NodeId(u32::MAX)`
    // sentinel the struct initializes with.
    assert_ne!(
        app.ref_box,
        lig_runtime::NodeId(u32::MAX),
        "the build body captured the element hole"
    );
    assert!(
        h.text_of(app.ref_box).is_none(),
        "the input element carries no text — the capture addresses the element itself"
    );
}

/// The M11 else-leg contract, end to end: the else branch mounts when the
/// condition is false (the M2-era fail-open rendered nothing — the emitted
/// else template was dead), its bind initializes through the mount marking
/// and updates live, the per-mount Badge rebuilds on the else mount with its
/// prop initialized, and the whole leg disposes on the flip back.
#[test]
fn show_else_mounts_binds_and_disposes_per_mount() {
    let mut h = MockHost::new();
    let mut app = show_else::App::build(&mut h, show_else::AppProps::default());
    // on = true: the then legs are up, the else component is absent.
    assert!(
        app.badge.is_none(),
        "the else leg's component is unmounted while then is up"
    );
    let then = app.frag.branch(0).expect("then leg mounted at build");
    assert_eq!(h.text_of(then.hole(0)), Some("in"), "then bind initialized");
    assert!(
        app.frag.branch(1).is_some(),
        "the second Show's then leg mounted"
    );
    // Flip off: the else legs mount — nothing rendered here before M11.
    app.set_on(false);
    lig_runtime::flush(&mut app, &mut h);
    let else_leg = app.frag.branch(0).expect("else leg mounted");
    assert_eq!(
        h.text_of(else_leg.hole(0)),
        Some("in"),
        "else bind initialized through the mount marking"
    );
    let badge = app.badge.as_ref().expect("else Badge rebuilt per mount");
    assert_eq!(
        h.text_of(badge.frag.hole(0)),
        Some("off"),
        "the fresh instance's prop initialized"
    );
    // The else bind is live: a late write lands while the else is mounted.
    app.set_tag("late".into());
    lig_runtime::flush(&mut app, &mut h);
    let else_leg = app.frag.branch(0).expect("else leg");
    assert_eq!(
        h.text_of(else_leg.hole(0)),
        Some("late"),
        "the else bind updates through its branch ctx"
    );
    // Flip on: the else leg (and its instance) disposes; the then leg
    // remounts fresh and re-initializes through the mount marking.
    app.set_on(true);
    lig_runtime::flush(&mut app, &mut h);
    assert!(
        app.badge.is_none(),
        "the else instance disposed with its branch"
    );
    let then = app.frag.branch(0).expect("then leg remounted");
    assert_eq!(
        h.text_of(then.hole(0)),
        Some("late"),
        "the remounted then leg re-initializes through the mount marking"
    );
}

/// The M15 per-item event dispatch, end to end: an event handler DIRECTLY on
/// a keyed For body attaches per fresh item (one Attach commit per item, at
/// the event's hole), dispatches index-addressed through the per-item fire
/// fn (the live item's value substitutes into the handler body, and the
/// setter writes cascade through the standard flush), keyed rotations move
/// the SAME handlers with their items (no re-attach), removals dispose the
/// records with the item's slab, and a stale index dispatches as a no-op.
#[test]
fn list_events_dispatch_per_item() {
    let mut h = MockHost::new();
    let mut app = for_event::Picks::build(&mut h, for_event::PicksProps::default());
    // Two items mounted on the initial flush → two Attach commits, one per
    // item, each at that item's own button node (distinct slabs), all with
    // the same handler reaction id and delegation flag.
    let attaches: Vec<_> = h
        .commits
        .iter()
        .filter(|c| matches!(c, mock_host::Commit::Attach { .. }))
        .collect();
    assert_eq!(attaches.len(), 2, "one attach per live item");
    let nodes: Vec<_> = (0..2)
        .map(|i| {
            app.frag
                .list_item(0, i)
                .map(|f| f.hole(0))
                .expect("item fragment")
        })
        .collect();
    assert_ne!(nodes[0], nodes[1], "each item owns its own button node");
    assert_eq!(app.frag.list_len(0), 2, "alpha/beta mounted");

    // Dispatch: fire on the live index substitutes THAT item — item 0 is
    // alpha, item 1 is beta.
    app.fire_f1_d0(&mut h, 0, &EvData::new());
    assert_eq!(app.picked, "alpha", "item 0's handler picked alpha");
    app.fire_f1_d0(&mut h, 1, &EvData::new());
    assert_eq!(app.picked, "beta", "item 1's handler picked beta");

    // A keyed rotate moves the SAME items without rebuilding: no new Attach
    // commits (the handler records move with their fragments), and the live
    // index now addresses the rotated item.
    let before = h
        .commits
        .iter()
        .filter(|c| matches!(c, mock_host::Commit::Attach { .. }))
        .count();
    let alpha_node = nodes[0];
    app.set_rows(vec!["beta".into(), "alpha".into()]);
    lig_runtime::flush(&mut app, &mut h);
    assert_eq!(
        app.frag
            .list_item(0, 1)
            .map(|f| f.hole(0))
            .expect("alpha moved to 1"),
        alpha_node,
        "the LIS-stable item kept its slab (and its handler record)"
    );
    let after = h
        .commits
        .iter()
        .filter(|c| matches!(c, mock_host::Commit::Attach { .. }))
        .count();
    assert_eq!(before, after, "a keyed move re-attaches nothing");
    app.fire_f1_d0(&mut h, 0, &EvData::new());
    assert_eq!(app.picked, "beta", "live index 0 is now beta");

    // A removal disposes the item's slab (its handler record with it); a
    // stale index dispatches as a no-op — the source no longer has the item.
    app.set_rows(vec!["beta".into()]);
    lig_runtime::flush(&mut app, &mut h);
    assert_eq!(app.frag.list_len(0), 1, "alpha removed");
    app.fire_f1_d0(&mut h, 1, &EvData::new());
    assert_eq!(app.picked, "beta", "a stale index is a no-op, not a panic");

    // A fresh key mounts a fresh item and attaches ITS handler exactly once.
    app.set_rows(vec!["beta".into(), "gamma".into()]);
    lig_runtime::flush(&mut app, &mut h);
    let fresh = h
        .commits
        .iter()
        .filter(|c| matches!(c, mock_host::Commit::Attach { .. }))
        .count();
    assert_eq!(fresh, before + 1, "the fresh item attached once");
    app.fire_f1_d0(&mut h, 1, &EvData::new());
    assert_eq!(app.picked, "gamma", "the fresh item's handler dispatches");
}

/// The M17 composed-list event surface, end to end: the For body hangs
/// inside a Show branch, and its DIRECT handler still attaches per fresh
/// item — through the composed reconcile's receiver-parameterized insert
/// arms — and dispatches index-addressed through the same per-item fire fn
/// shape as the direct list (M15). The branch cycle mounts, disposes, and
/// remounts the records with the branch; keyed moves inside the branch carry
/// them; a stale index stays a no-op.
#[test]
fn composed_list_events_dispatch_per_item() {
    let mut h = MockHost::new();
    let mut app =
        for_event_composed::PickPanel::build(&mut h, for_event_composed::PickPanelProps::default());
    // The branch mounts on the initial flush with both items → one Attach
    // per live item, each at that item's own button node inside the branch's
    // list slot.
    let attaches = |h: &MockHost| {
        h.commits
            .iter()
            .filter(|c| matches!(c, mock_host::Commit::Attach { .. }))
            .count()
    };
    assert_eq!(
        attaches(&h),
        2,
        "one attach per live item on the branch mount"
    );
    let branch = app.frag.branch(0).expect("the then leg mounts");
    assert_eq!(
        branch.list_len(0),
        2,
        "alpha/beta mounted inside the branch"
    );
    let alpha_node = branch.list_item(0, 0).map(|f| f.hole(0)).expect("item 0");
    let beta_node = branch.list_item(0, 1).map(|f| f.hole(0)).expect("item 1");
    assert_ne!(alpha_node, beta_node, "each item owns its own button node");

    // Dispatch: index-addressed through the source, so the live item's value
    // substitutes exactly like the direct list (M15) does.
    app.fire_f2_d0(&mut h, 0, &EvData::new());
    assert_eq!(app.picked, "alpha", "item 0's handler picked alpha");
    app.fire_f2_d0(&mut h, 1, &EvData::new());
    assert_eq!(app.picked, "beta", "item 1's handler picked beta");

    // A keyed rotation inside the branch moves the SAME handler records:
    // no re-attach, and the live index addresses the rotated item.
    let before = attaches(&h);
    app.set_rows(vec!["beta".into(), "alpha".into()]);
    lig_runtime::flush(&mut app, &mut h);
    let branch = app
        .frag
        .branch(0)
        .expect("the branch survives the rotation");
    assert_eq!(
        branch.list_item(0, 1).map(|f| f.hole(0)),
        Some(alpha_node),
        "the LIS-stable item kept its slab (and its handler record)"
    );
    assert_eq!(attaches(&h), before, "a keyed move re-attaches nothing");
    app.fire_f2_d0(&mut h, 0, &EvData::new());
    assert_eq!(app.picked, "beta", "live index 0 is now beta");

    // The branch cycle: unmounting disposes the records with the branch,
    // remounting attaches fresh ones — per-mount semantics (M10) composed
    // with per-item dispatch (M15/M17).
    app.set_on(false);
    lig_runtime::flush(&mut app, &mut h);
    assert!(app.frag.branch(0).is_none(), "the branch unmounted");
    app.set_on(true);
    lig_runtime::flush(&mut app, &mut h);
    assert_eq!(
        attaches(&h),
        before + 2,
        "the remount attached one record per fresh item"
    );
    let branch = app.frag.branch(0).expect("the then leg remounted");
    assert_ne!(
        branch.list_item(0, 0).map(|f| f.hole(0)),
        Some(alpha_node),
        "a remounted branch rebuilds fresh slabs"
    );
    app.fire_f2_d0(&mut h, 0, &EvData::new());
    assert_eq!(app.picked, "beta", "the remounted item 0 dispatches");

    // A stale index stays a no-op (source-addressed, never a panic).
    app.set_rows(vec!["beta".into()]);
    lig_runtime::flush(&mut app, &mut h);
    app.fire_f2_d0(&mut h, 1, &EvData::new());
    assert_eq!(app.picked, "beta", "a stale index is a no-op, not a panic");
}

/// The M18 composed-content owner-direct lift, end to end: a fill carried
/// into a branch-mounted child (`Cell`) holds a keyed list whose body
/// carries BOTH the per-item handler and the per-item outlet. The fill
/// mounts into the fresh instance at branch mount (the M10 identity cycle,
/// completed by the M18 mounting-arm assembly), the handler attaches per
/// fresh item through the composed reconcile's insert arms (M17 machinery,
/// the content leg), the outlet's own fallback mounts per item through the
/// shared replay (M14 machinery, through any receiver), keyed moves carry
/// the handler records and the mounted content without rebuilds, removals
/// dispose with the item's slab, and the branch cycle rebuilds everything
/// per mount.
#[test]
fn composed_content_lists_carry_events_and_slots_per_item() {
    let mut h = MockHost::new();
    let mut app = for_composed_content::PickList::build(
        &mut h,
        for_composed_content::PickListProps::default(),
    );
    let attaches = |h: &MockHost| {
        h.commits
            .iter()
            .filter(|c| matches!(c, mock_host::Commit::Attach { .. }))
            .count()
    };
    // The branch mounted at build; the fill mounted INTO the fresh instance.
    let cell = app.cell.as_ref().expect("the branch mounted the instance");
    let fill = cell.frag.slot_fill(0).expect("the fill mounted at mount");
    assert_eq!(
        fill.list_len(0),
        2,
        "alpha/beta mounted in the content list"
    );

    // One attach per live item, and one mounted fallback per item bound to
    // its item by the replay.
    assert_eq!(attaches(&h), 2, "one attach per live item at mount");
    let alpha_fb = fill
        .list_item(0, 0)
        .and_then(|f| f.slot_fallback(1))
        .expect("row 0 fallback mounted");
    let beta_fb = fill
        .list_item(0, 1)
        .and_then(|f| f.slot_fallback(1))
        .expect("row 1 fallback mounted");
    let alpha_node = alpha_fb.hole(0);
    assert_eq!(h.text_of(alpha_node), Some("alpha"), "item bind by replay");
    assert_eq!(h.text_of(beta_fb.hole(0)), Some("beta"));

    // Dispatch: index-addressed through the source, exactly like the direct
    // and branch-owned composed lists (M15/M17).
    app.fire_f4_d0(&mut h, 0, &EvData::new());
    assert_eq!(app.picked, "alpha", "item 0's handler picked alpha");
    app.fire_f4_d0(&mut h, 1, &EvData::new());
    assert_eq!(app.picked, "beta", "item 1's handler picked beta");

    // A keyed rotation moves the SAME slabs (handler records + mounted
    // fallbacks) without re-attaching or rebuilding.
    let before = attaches(&h);
    app.set_rows(vec!["beta".into(), "alpha".into()]);
    lig_runtime::flush(&mut app, &mut h);
    let cell = app.cell.as_ref().expect("the branch survives the rotation");
    let fill = cell.frag.slot_fill(0).expect("the fill survives");
    assert_eq!(
        fill.list_item(0, 1)
            .and_then(|f| f.slot_fallback(1))
            .map(|f| f.hole(0)),
        Some(alpha_node),
        "the LIS-stable item kept its slab and its mounted fallback"
    );
    assert_eq!(attaches(&h), before, "a keyed move re-attaches nothing");
    app.fire_f4_d0(&mut h, 0, &EvData::new());
    assert_eq!(app.picked, "beta", "live index 0 is now beta");

    // The branch cycle: unmounting disposes the instance (the fill content,
    // the items, the records go with it), remounting rebuilds everything —
    // the fill mounts into the FRESH instance, the mount mark re-runs the
    // list's reconcile, and fresh records attach per fresh item.
    app.set_on(false);
    lig_runtime::flush(&mut app, &mut h);
    assert!(app.cell.is_none(), "the unmount disposed the instance");
    app.set_on(true);
    lig_runtime::flush(&mut app, &mut h);
    let cell = app.cell.as_ref().expect("the remount rebuilt the instance");
    let fill = cell.frag.slot_fill(0).expect("the fill remounted");
    assert_eq!(fill.list_len(0), 2, "the remounted list refilled");
    assert_eq!(
        attaches(&h),
        before + 2,
        "the remount attached one record per fresh item"
    );
    assert_ne!(
        fill.list_item(0, 0)
            .and_then(|f| f.slot_fallback(1))
            .map(|f| f.hole(0)),
        Some(alpha_node),
        "a remounted branch rebuilds fresh slabs"
    );
    app.fire_f4_d0(&mut h, 1, &EvData::new());
    assert_eq!(app.picked, "alpha", "the remounted item dispatches");

    // A stale index stays a no-op (source-addressed, never a panic).
    app.set_rows(vec!["alpha".into()]);
    lig_runtime::flush(&mut app, &mut h);
    app.fire_f4_d0(&mut h, 1, &EvData::new());
    assert_eq!(app.picked, "alpha", "a stale index is a no-op, not a panic");
}
