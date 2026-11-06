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
