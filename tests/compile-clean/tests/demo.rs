//! The KR-1 demo (§17.1): Solid TSX → generated Rust renders on the mock
//! host and updates through the const schedule. Every assertion here is a
//! compile-clean fixture run end-to-end.

use compile_clean::{
    component_use, counter, dynamic_style, effect_multi, event_object, for_keyed, for_list,
    slots_branch_component, slots_branch_outlet, slots_component, slots_dynamic, slots_named,
    slots_nested, slots_structural, template_ref, v_model,
};
use lig_runtime::{EvData, SlotOrigin, key_hash};
use mock_host::MockHost;

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
