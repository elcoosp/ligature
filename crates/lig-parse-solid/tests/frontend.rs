//! Frontend contract tests: the `counter` fixture (S1) is the golden path;
//! admission-spike diagnostics are asserted with spans and codes.

use lig_ir::{Frontend, ViewModule};

use lig_parse_solid::tests_support::{COUNTER_TSX, ITEMS_TSX};

/// Lower helper that asserts success.
pub fn expect_ok(source: &str, path: &str) -> ViewModule {
    match lig_parse_solid::lower_module(source, path) {
        Ok(m) => m,
        Err(diags) => {
            let rendered: Vec<String> = diags
                .iter()
                .map(|d| format!("{}: {}", d.code, d.message))
                .collect();
            panic!("lowering failed: {rendered:?}");
        }
    }
}

#[test]
fn counter_lowers_with_no_diagnostics() {
    let m = expect_ok(COUNTER_TSX, "counter.tsx");
    assert_eq!(m.frontend, Frontend::Solid);
    assert_eq!(m.components.len(), 1);
    let v = lig_ir::validate::validate(&m);
    assert!(v.is_empty(), "invariants must hold: {v:?}");
}

#[test]
fn counter_states_and_reactions_match_the_spec_shape() {
    let m = expect_ok(COUNTER_TSX, "counter.tsx");
    let names: Vec<&str> = m.states.iter().map(|s| m.strings.get(s.name)).collect();
    assert!(names.contains(&"count"), "signal: {names:?}");
    assert!(names.contains(&"doubled"), "memo: {names:?}");
    let memos = m
        .reactions
        .iter()
        .filter(|r| matches!(r.kind, lig_ir::ReactionKind::Memo { .. }))
        .count();
    assert_eq!(memos, 1);
    let effects = m
        .reactions
        .iter()
        .filter(|r| matches!(r.kind, lig_ir::ReactionKind::Effect { .. }))
        .count();
    assert_eq!(effects, 1, "the onClick handler is the only effect");
    let writes_effect = m
        .reactions
        .iter()
        .find(|r| matches!(r.kind, lig_ir::ReactionKind::Effect { .. }))
        .expect("effect");
    assert_eq!(writes_effect.writes.len(), 1);
    assert_eq!(m.strings.get(writes_effect.writes[0]), "count");
}

#[test]
fn counter_canonical_json_is_stable() {
    let m = expect_ok(COUNTER_TSX, "counter.tsx");
    insta::assert_snapshot!("counter_canonical_ir", m.canonical_json_pretty());
}

#[test]
fn lower_twice_is_byte_identical() {
    let a = expect_ok(COUNTER_TSX, "counter.tsx").canonical_json();
    let b = expect_ok(COUNTER_TSX, "counter.tsx").canonical_json();
    assert_eq!(a, b, "byte-reproducibility (D-8) starts at the frontend");
}

#[test]
fn static_style_becomes_a_template_const() {
    let m = expect_ok(COUNTER_TSX, "counter.tsx");
    assert_eq!(m.styles.len(), 1, "display:flex;padding:16px interns once");
    assert!(m.styles[0].css.contains("display:flex"));
    assert!(m.styles[0].css.contains("padding:16px"));
}

#[test]
fn anchors_resolve_inside_templates() {
    let m = expect_ok(COUNTER_TSX, "counter.tsx");
    for (fi, f) in m.fragments.iter().enumerate() {
        for (di, d) in f.dynamics.iter().enumerate() {
            assert!(
                lig_ir::validate::resolve_anchor(&m, f.template.get() as usize, d.anchor.steps())
                    .is_some(),
                "fragments[{fi}].dynamics[{di}] anchor must resolve"
            );
        }
    }
}

#[test]
fn map_children_lower_to_unkeyed_for() {
    let m = expect_ok(ITEMS_TSX, "items.tsx");
    let fors: Vec<&str> = m
        .fragments
        .iter()
        .flat_map(|f| f.dynamics.iter())
        .filter_map(|d| match &d.kind {
            lig_ir::DynamicKind::For(_) => Some("for"),
            _ => None,
        })
        .collect();
    assert_eq!(fors.len(), 1, "one For dynamic: {fors:?}");
    assert_eq!(
        lig_ir::validate::validate(&m),
        [] as [lig_ir::validate::Violation; 0]
    );
}

const KEYED_TSX: &str = r#"export function Tags() {
  const [tags, setTags] = createSignal(["alpha", "beta", "gamma"]);
  return (
    <ul>
      <For each={tags()} key={(t) => t}>
        {(t) => <li>{t}</li>}
      </For>
    </ul>
  );
}
"#;

/// The For's key expression: `key={(t) => t}` lowers to a `Local` read of
/// the item binding (the same shape the Vue `:key` leg produces — §6.4
/// parity), and the module still validates.
#[test]
fn for_key_lowers_to_the_item_read() {
    let m = expect_ok(KEYED_TSX, "tags.tsx");
    let key = m
        .fragments
        .iter()
        .flat_map(|f| f.dynamics.iter())
        .find_map(|d| match &d.kind {
            lig_ir::DynamicKind::For(f) => f.key,
            _ => None,
        })
        .expect("keyed For carries a key expression");
    let item = m
        .fragments
        .iter()
        .flat_map(|f| f.dynamics.iter())
        .find_map(|d| match &d.kind {
            lig_ir::DynamicKind::For(f) => Some(f.item),
            _ => None,
        })
        .expect("For item binding");
    assert_eq!(
        m.exprs.get(key),
        &lig_ir::expr::Expr::Local(item),
        "identity key reads the item binding"
    );
    assert_eq!(
        lig_ir::validate::validate(&m),
        [] as [lig_ir::validate::Violation; 0]
    );
}

/// A key arrow over a name that is not the body's item binding is
/// fail-closed: the key and the body must describe the same item.
#[test]
fn for_key_with_a_foreign_param_is_e001() {
    let src = r#"export function Tags() {
  const [tags, setTags] = createSignal(["alpha"]);
  return (
    <ul>
      <For each={tags()} key={(x) => x}>
        {(t) => <li>{t}</li>}
      </For>
    </ul>
  );
}
"#;
    let err = lig_parse_solid::lower_module(src, "tags.tsx").expect_err("must fail");
    assert!(err.iter().any(|d| d.code == "LIG-E001"), "{err:?}");
    assert!(err.iter().any(|d| d.message.contains("item binding")));
}

/// A key reading component state re-identifies items on unrelated writes —
/// fail-closed at admission.
#[test]
fn for_key_reading_state_is_e001() {
    let src = r#"export function Tags() {
  const [tags, setTags] = createSignal(["alpha"]);
  const [salt, setSalt] = createSignal(1);
  return (
    <ul>
      <For each={tags()} key={(t) => salt()}>
        {(t) => <li>{t}</li>}
      </For>
    </ul>
  );
}
"#;
    let err = lig_parse_solid::lower_module(src, "tags.tsx").expect_err("must fail");
    assert!(err.iter().any(|d| d.code == "LIG-E001"), "{err:?}");
    assert!(err.iter().any(|d| d.message.contains("pure function")));
}

/// A non-arrow `key` value is fail-closed with the admitted shape in the
/// message.
#[test]
fn for_key_non_arrow_is_e001() {
    let src = r#"export function Tags() {
  const [tags, setTags] = createSignal(["alpha"]);
  return (
    <ul>
      <For each={tags()} key={tags()}>
        {(t) => <li>{t}</li>}
      </For>
    </ul>
  );
}
"#;
    let err = lig_parse_solid::lower_module(src, "tags.tsx").expect_err("must fail");
    assert!(err.iter().any(|d| d.code == "LIG-E001"), "{err:?}");
    assert!(err.iter().any(|d| d.message.contains("arrow")));
}

#[test]
fn unknown_identifier_is_e100() {
    let src = r"export function Bad() {
  return <p>{nope}</p>;
}
";
    let err = lig_parse_solid::lower_module(src, "bad.tsx").expect_err("must fail");
    assert!(err.iter().any(|d| d.code == "LIG-E100"));
    assert!(err.iter().any(|d| d.message.contains("`nope`")));
}

#[test]
fn callback_calls_are_denied_with_e114() {
    let src = r"export function Bad() {
  const [items, setItems] = createSignal(0);
  return <p>{items.filter((i) => i)}</p>;
}
";
    let err = lig_parse_solid::lower_module(src, "bad.tsx").expect_err("must fail");
    assert!(err.iter().any(|d| d.code == "LIG-E114"), "{err:?}");
}

#[test]
fn parse_errors_are_e001_with_spans() {
    let err = lig_parse_solid::lower_module("export function (", "bad.tsx").expect_err("must fail");
    assert!(err.iter().any(|d| d.code == "LIG-E001"));
}

#[test]
fn props_from_destructured_params() {
    let src = r"export function Greet({ name }: { name: string }) {
  return <p>{name}</p>;
}
";
    let m = expect_ok(src, "greet.tsx");
    let props: Vec<&str> = m
        .states
        .iter()
        .filter(|s| s.kind == lig_ir::StateKind::Prop)
        .map(|s| m.strings.get(s.name))
        .collect();
    assert_eq!(props, vec!["name"]);
}

/// The M7 slot-content contract (Solid leg): scalar binds, events, and refs
/// directly on fill/fallback fragments are admitted — the runtime addresses
/// them through the mounted content.
#[test]
fn slot_content_admits_scalar_binds_events_and_refs() {
    let src = r#"export function Panel({ title }) {
  let foot;
  return (
    <section>
      <Slot name="header">
        <em class={title}>{title}</em>
      </Slot>
      <Slot name="foot">
        <i ref={foot}>{title}</i>
      </Slot>
    </section>
  );
}

export function App() {
  const [label, setLabel] = createSignal("live");
  return (
    <Panel
      title="Status"
      header={<button onClick={() => setLabel("next")}>{label}</button>}
    />
  );
}
"#;
    let m = expect_ok(src, "slots_dynamic.tsx");
    let v = lig_ir::validate::validate(&m);
    assert!(v.is_empty(), "invariants must hold: {v:?}");
    // The fill (button) carries the event + text; the fallbacks carry their
    // own binds; the foot fallback carries the ref.
    let dynamic_frags: Vec<String> = m
        .fragments
        .iter()
        .filter(|f| !f.dynamics.is_empty())
        .map(|f| m.strings.get(f.name).to_string())
        .collect();
    assert!(
        dynamic_frags.iter().any(|n| n.starts_with("slotfill")),
        "fill fragment with dynamics: {dynamic_frags:?}"
    );
    assert!(
        dynamic_frags.iter().any(|n| n.starts_with("slotfallback")),
        "fallback fragment with dynamics: {dynamic_frags:?}"
    );
}

/// The M8 slot-content contract (Solid leg): the content may be STRUCTURAL —
/// If/For dynamics on the content root lower onto the content fragment (the
/// branch/list tables are the content's, addressed through composed `Ctx`
/// walks), while component uses and nested outlets stay fail-closed.
#[test]
fn slot_content_admits_structural_constructs() {
    // If inside a fill (the Show root lowers onto the fill fragment)
    let src = r#"export function App() {
  const [on, setOn] = createSignal(true);
  return (
    <Panel title="x" header={<Show when={on()}><b>hi</b></Show>} />
  );
}

export function Panel({ title }) {
  return <section><Slot name="header" /><Slot name="foot" /></section>;
}
"#;
    let m = expect_ok(src, "structural_fill.tsx");
    let fill = m
        .fragments
        .iter()
        .find(|f| m.strings.get(f.name).starts_with("slotfill"))
        .expect("fill fragment");
    assert!(
        fill.dynamics
            .iter()
            .any(|d| matches!(d.kind, lig_ir::DynamicKind::If(_))),
        "the If lowers onto the fill fragment: {fill:?}"
    );
    // For inside a fallback
    let src = r#"export function Panel({ title }) {
  return (
    <section>
      <Slot name="foot">
        <For each={["a"]}>{(item) => <i>{item}</i>}</For>
      </Slot>
    </section>
  );
}
"#;
    let m = expect_ok(src, "structural_fallback.tsx");
    let fb = m
        .fragments
        .iter()
        .find(|f| m.strings.get(f.name).starts_with("slotfallback"))
        .expect("fallback fragment");
    assert!(
        fb.dynamics
            .iter()
            .any(|d| matches!(d.kind, lig_ir::DynamicKind::For(_))),
        "the For lowers onto the fallback fragment: {fb:?}"
    );
}

/// The M9 slot-content contract (Solid leg): component uses and nested slot
/// outlets admit DIRECTLY on fill/fallback content — a grandchild component
/// use lowers onto the content fragment with its prop bindings (the
/// prop-pass is the content author's reaction), and a nested outlet lowers
/// with its own fallback (whose fills match the same use site).
#[test]
fn slot_content_admits_component_uses_and_nested_outlets() {
    // A grandchild component use inside a fill, with a reactive prop.
    let src = r#"export function App() {
  const [n, setN] = createSignal(1);
  return (
    <Panel title="x" header={<Badge count={n()} />} />
  );
}

export function Badge({ count }) {
  return <b class="badge">{count}</b>;
}

export function Panel({ title }) {
  return <section><Slot name="header" /></section>;
}
"#;
    let m = expect_ok(src, "component_in_fill.tsx");
    let v = lig_ir::validate::validate(&m);
    assert!(v.is_empty(), "invariants must hold: {v:?}");
    let comp = m
        .fragments
        .iter()
        .filter(|f| m.strings.get(f.name).starts_with("slotfill"))
        .flat_map(|f| f.dynamics.iter())
        .find_map(|d| match &d.kind {
            lig_ir::DynamicKind::Component(c) => Some(c.clone()),
            _ => None,
        });
    assert!(
        comp.is_some(),
        "the component use lowers onto a fill fragment: {:?}",
        m.fragments
            .iter()
            .map(|f| (m.strings.get(f.name).to_string(), f.dynamics.len()))
            .collect::<Vec<_>>()
    );
    let comp = comp.expect("component ir");
    assert_eq!(comp.props.len(), 1, "the reactive prop binds");
    // A nested outlet inside a fill, with the fill for it on the same use site.
    let src = r#"export function App() {
  return (
    <Panel
      title="x"
      header={<div><Slot name="inner"><em>none</em></Slot></div>}
      inner={<b>inner filled</b>}
    />
  );
}

export function Panel({ title }) {
  return <section><Slot name="header" /></section>;
}
"#;
    let m = expect_ok(src, "nested_outlet.tsx");
    let v = lig_ir::validate::validate(&m);
    assert!(v.is_empty(), "invariants must hold: {v:?}");
    let nested_outlet = m.fragments.iter().any(|f| {
        m.strings.get(f.name).starts_with("slotfill")
            && f.dynamics.iter().any(|d| {
                matches!(&d.kind, lig_ir::DynamicKind::Slot { name, .. }
                    if m.strings.get(*name) == "inner")
            })
    });
    assert!(
        nested_outlet,
        "the nested `inner` outlet lowers onto a fill fragment: {:?}",
        m.fragments
            .iter()
            .map(|f| m.strings.get(f.name).to_string())
            .collect::<Vec<_>>()
    );
}

/// The M10 fail-closed boundary (Solid leg): component uses and nested
/// outlets inside LIST bodies of content stay denied with LIG-E001 —
/// per-item instance identity remains the keyed-component story. The
/// BRANCH cases M9 denied admit since M10 (per-mount identity + the
/// branch-time slot assembly).
#[test]
fn slot_content_stays_fail_closed_on_structural_instances() {
    let cases: [(&str, &str); 2] = [
        // Component use inside a For body on content
        (
            r#"import { For } from "solid-js";
export function App() {
  return (
    <Panel
      title="x"
      header={<For each={["a"]}>{(t) => <Badge count={1} />}</For>}
    />
  );
}

export function Badge({ count }) {
  return <b>{count}</b>;
}
"#,
            "Component in content list",
        ),
        // Nested outlet inside a For body on content
        (
            r#"import { For } from "solid-js";
export function App() {
  return (
    <Panel
      title="x"
      header={<For each={["a"]}>{(t) => <Slot name="inner" />}</For>}
    />
  );
}
"#,
            "Outlet in content list",
        ),
    ];
    for (body, what) in cases {
        let src = format!(
            "{body}\nexport function Panel({{ title }}) {{\n  return <section><Slot name=\"foot\" /></section>;\n}}\n"
        );
        let src = src.replace("\\n", "\n");
        match lig_parse_solid::lower_module(&src, "bad.tsx") {
            Ok(_) => panic!("{what}: expected LIG-E001, got success"),
            Err(diags) => assert!(
                diags.iter().any(|d| d.code == "LIG-E001"),
                "{what}: expected LIG-E001, got {diags:?}"
            ),
        }
    }
}

/// The M10 lift (Solid leg): component uses and nested outlets admit
/// DIRECTLY inside BRANCH bodies of slot content — a remounted branch
/// rebuilds its per-mount component instance, and the outlet mounts its
/// fill/fallback with the branch (branch-time slot assembly).
#[test]
fn content_branches_admit_components_and_outlets() {
    let cases: [(&str, &str); 2] = [
        // Component use inside a branch hanging on content
        (
            r#"export function App() {
  const [on, setOn] = createSignal(true);
  return (
    <Panel title="x" header={<Show when={on()}><Badge count={1} /></Show>} />
  );
}

export function Badge({ count }) {
  return <b>{count}</b>;
}
"#,
            "Component in content branch",
        ),
        // Nested outlet inside a branch hanging on content
        (
            r#"export function App() {
  const [on, setOn] = createSignal(true);
  return (
    <Panel
      title="x"
      header={<Show when={on()}><Slot name="inner" /></Show>}
    />
  );
}
"#,
            "Outlet in content branch",
        ),
    ];
    for (body, what) in cases {
        let src = format!(
            "{body}\nexport function Panel({{ title }}) {{\n  return <section><Slot name=\"foot\" /></section>;\n}}\n"
        );
        let src = src.replace("\\n", "\n");
        lig_parse_solid::lower_module(&src, "branch.tsx")
            .unwrap_or_else(|diags| panic!("{what}: expected admission, got {diags:?}"));
    }
}

/// The M10 own-tree contract (Solid leg): component uses and outlets inside
/// FOR bodies of the component's OWN template deny with one precise reason
/// (per-item identity is the keyed-component story) — closing two silent
/// fail-opens where the item template lowered with a never-mounted outlet
/// hole and a single detached component instance. If branches admit (the
/// per-mount identity story).
#[test]
fn own_tree_for_bodies_deny_components_and_outlets() {
    let cases: [(&str, &str); 2] = [
        // Outlet inside a For body of the component's own template
        (
            r#"import { For } from "solid-js";
export function Inner({ items }) {
  return (
    <ul>
      <For each={items()}>{(item) => <Slot name="a"><em>{item}</em></Slot>}</For>
    </ul>
  );
}
"#,
            "Outlet in own-tree list",
        ),
        // Component use inside a For body of the component's own template
        (
            r#"import { For } from "solid-js";
export function App({ items }) {
  return (
    <main>
      <For each={items()}>{(item) => <Badge count={item} />}</For>
    </main>
  );
}

export function Badge({ count }) {
  return <b>{count}</b>;
}
"#,
            "Component in own-tree list",
        ),
    ];
    for (src, what) in cases {
        match lig_parse_solid::lower_module(src, "own_list.tsx") {
            Ok(_) => panic!("{what}: expected LIG-E001, got success"),
            Err(diags) => {
                assert!(
                    diags.iter().any(|d| d.code == "LIG-E001"),
                    "{what}: expected LIG-E001, got {diags:?}"
                );
                assert!(
                    diags.iter().any(|d| d.message.contains("per-item")),
                    "{what}: expected the per-item reason, got {diags:?}"
                );
            }
        }
    }
}
