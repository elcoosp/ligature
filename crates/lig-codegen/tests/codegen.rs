//! Codegen contract tests: byte-reproducibility (D-8), shape checks against
//! §9.2, and the fail-closed subset boundaries.

use lig_codegen::generate_component;
use lig_parse_solid::tests_support::COUNTER_TSX;

fn counter_code() -> String {
    let module = lig_parse_solid::lower_module(COUNTER_TSX, "counter.tsx").expect("lower");
    let unit = generate_component(&module, lig_core::id::ComponentId::new(0)).expect("codegen");
    unit.text
}

#[test]
fn counter_generates_with_expected_shape() {
    let text = counter_code();
    assert!(text.contains("pub struct Counter {"), "{text}");
    assert!(text.contains("pub count: i64"));
    assert!(text.contains("pub doubled: i64"));
    assert!(text.contains("pub fn build(h: &mut dyn Host, props: CounterProps) -> Self"));
    assert!(text.contains("impl ::lig_runtime::Reactions for Counter"));
    assert!(text.contains("pub fn set_count"));
    assert!(text.contains("TPL_T"));
    assert!(text.contains("AnchorStep::Down"));
    // the memo gate (§7.2 equality-gated memos)
    assert!(text.contains("if next != self.doubled"));
}

#[test]
fn counter_codegen_is_byte_reproducible() {
    let a = counter_code();
    let b = counter_code();
    assert_eq!(a, b, "two generations must be byte-identical (D-8)");
}

#[test]
fn event_glue_dispatches_through_the_flush_loop() {
    let text = counter_code();
    assert!(text.contains("pub fn fire_"), "{text}");
    assert!(text.contains("ev: &::lig_runtime::EvData"), "{text}");
    assert!(text.contains("::lig_runtime::flush(self, h)"));
    assert!(text.contains("EventKind::Click"), "{text}");
}

#[test]
fn schedule_is_a_program_constant() {
    let text = counter_code();
    assert!(
        text.contains("static Counter_SCHEDULE: ::lig_runtime::Schedule"),
        "{text}"
    );
    assert!(text.contains("order: &["));
    assert!(text.contains("writes: &["));
}

#[test]
fn props_generate_a_props_struct_and_setter() {
    let src = r"export function Greet({ name }: { name: string }) {
  return <p>{name}</p>;
}
";
    let module = lig_parse_solid::lower_module(src, "greet.tsx").expect("lower");
    let unit = generate_component(&module, lig_core::id::ComponentId::new(0)).expect("props (M1)");
    // §7.6: props are plain struct fields; the parent passes a <Name>Props.
    assert!(unit.text.contains("pub struct GreetProps"), "{}", unit.text);
    assert!(unit.text.contains("pub name: ::alloc::string::String"));
    assert!(unit.text.contains("props.name"));
    // §7.6: setters are the only write path.
    assert!(unit.text.contains("pub fn set_name"));
}

#[test]
fn snapshot_counter_generated_code() {
    let text = counter_code();
    insta::assert_snapshot!("counter_generated", text);
}
#[test]
fn spill_path_emits_exact_per_bit_masks_and_writes_hi() {
    // A synthetic 70-reaction component: one signal read by 70 effects. The
    // init masks must be exact per bit (low word full, spill word 0b111111),
    // and the schedule carries a (zeroed) writes_hi row — the M3 dispatch
    // refinement over the former mark-everything spill path.
    use lig_ir::expr::{Expr, Lit};
    use lig_ir::fragment::{ComponentDecl, FragmentIr, StateDecl, StateKind};
    use lig_ir::module::{ReactionDecl, ReactionKind, Tier};

    let mut m = lig_ir::ViewModule::new(lig_ir::Frontend::Solid);
    let tpl = m.intern_template(lig_ir::template::TemplateIr::new(
        lig_ir::template::NodeIr::Marker,
    ));
    let name = m.strings.intern("Big");
    let root = m.push_fragment(FragmentIr {
        name,
        template: tpl,
        dynamics: vec![],
        children: vec![],
        props: vec![],
        returns: vec![],
    });
    m.components.push(ComponentDecl {
        name,
        root,
        props: vec![],
    });
    let count = m.strings.intern("count");
    let init = m.insert_expr(Expr::Lit(Lit::Int(0)));
    m.states.push(StateDecl {
        name: count,
        owner: root,
        kind: StateKind::Signal,
        ty: lig_ir::Ty::Int,
        init: Some(init),
    });
    for i in 0..70 {
        let body = m.insert_expr(Expr::Lit(Lit::Int(i)));
        m.reactions.push(ReactionDecl {
            kind: ReactionKind::Effect { tier: Tier::Post },
            reads: vec![count],
            writes: vec![],
            body,
            owner: root,
        });
    }
    let unit = generate_component(&m, lig_core::id::ComponentId::new(0)).expect("codegen");
    let text = unit.text;
    assert!(
        text.contains("const Big_N_REACTIONS: usize = 70usize;"),
        "{text}"
    );
    assert!(
        text.contains("writes_hi: &["),
        "schedule carries the spill row: {text}"
    );
    assert!(
        text.contains("mark_mask(0xffffffffffffffff)"),
        "low init word is exact: {text}"
    );
    assert!(
        text.contains("mark_spill_word(0, 0x3f)"),
        "spill init word is exact (70 − 64 bits): {text}"
    );
    assert!(text.contains("const Big_R69_FX: u8 = 69u8;"), "{text}");
}
