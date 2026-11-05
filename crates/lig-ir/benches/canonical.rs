//! Benchmarks for canonicalization and fingerprinting (§14.2: `view_ir`).

use criterion::{Criterion, criterion_group, criterion_main};
use lig_ir::expr::{BinOp, Expr, Lit};
use lig_ir::fragment::{
    ComponentDecl, DynamicIr, DynamicKind, FragmentIr, IfIr, StateDecl, StateKind,
};
use lig_ir::module::{ReactionDecl, ReactionKind, Tier, ViewModule};
use lig_ir::template::{NodeIr, TemplateIr};
use lig_ir::{ExprId, FragmentId, Frontend, Ty};

/// A counter-sized module: 1 component, 3 templates, 2 dynamics, 4 states,
/// 4 reactions — the M0 reference shape (§9.2).
// Fixture-sized module builder; length is fixture description.
#[allow(clippy::too_many_lines)]
fn counter_module() -> ViewModule {
    let mut m = ViewModule::new(Frontend::Solid);
    let name = m.strings.intern("Counter");
    let count = m.strings.intern("count");
    let doubled = m.strings.intern("doubled");
    let label = m.strings.intern("Add");

    let p_tag = m.strings.intern("p");
    let branch_tpl = m.intern_template(TemplateIr::new(NodeIr::Element {
        tag: p_tag,
        static_attrs: vec![],
        style: None,
        children: vec![NodeIr::Text { value: label }, NodeIr::Marker],
    }));
    let div_tag = m.strings.intern("div");
    let root_tpl = m.intern_template(TemplateIr::new(NodeIr::Element {
        tag: div_tag,
        static_attrs: vec![],
        style: None,
        children: vec![NodeIr::Marker, NodeIr::Marker],
    }));

    let branch_name = m.strings.intern("Counter_branch_0");
    let branch = m.push_fragment(FragmentIr {
        name: branch_name,
        template: branch_tpl,
        dynamics: vec![DynamicIr {
            anchor: lig_ir::anchor::Anchor(vec![lig_ir::anchor::AnchorStep::MarkerDown(1)]),
            kind: DynamicKind::Text,
            expr: ExprId::new(0),
            reaction: None,
            span: (0, 0),
        }],
        children: vec![],
        props: vec![],
        returns: vec![],
    });
    let cond_e = m.insert_expr(Expr::Binary(
        BinOp::Gt,
        Box::new(Expr::State(doubled)),
        Box::new(Expr::Lit(Lit::Int(10))),
    ));
    let count_e = m.insert_expr(Expr::State(count));
    let dynamics = vec![
        DynamicIr {
            anchor: lig_ir::Anchor(vec![
                lig_ir::AnchorStep::Down(0),
                lig_ir::AnchorStep::MarkerDown(0),
            ]),
            kind: DynamicKind::Text,
            expr: count_e,
            reaction: None,
            span: (0, 0),
        },
        DynamicIr {
            anchor: lig_ir::Anchor::root(),
            kind: DynamicKind::If(IfIr {
                cond: cond_e,
                then: branch,
                else_: None,
            }),
            expr: cond_e,
            reaction: None,
            span: (0, 0),
        },
    ];
    let frag = m.push_fragment(FragmentIr {
        name,
        template: root_tpl,
        dynamics,
        children: vec![branch],
        props: vec![],
        returns: vec![],
    });
    m.components.push(ComponentDecl {
        name,
        root: frag,
        props: vec![],
    });

    let init = m.insert_expr(Expr::Lit(Lit::Int(0)));
    let memo_body = m.insert_expr(Expr::Binary(
        BinOp::Mul,
        Box::new(Expr::State(count)),
        Box::new(Expr::Lit(Lit::Int(2))),
    ));
    m.states.push(StateDecl {
        name: count,
        owner: frag,
        kind: StateKind::Signal,
        ty: Ty::Int,
        init: Some(init),
    });
    m.states.push(StateDecl {
        name: doubled,
        owner: frag,
        kind: StateKind::Memo,
        ty: Ty::Int,
        init: None,
    });
    m.reactions.push(ReactionDecl {
        kind: ReactionKind::Memo { target: doubled },
        reads: vec![count],
        writes: vec![doubled],
        body: memo_body,
        owner: frag,
    });
    let _ = Tier::Pre;
    m
}

fn bench_canonical(c: &mut Criterion) {
    let mut m = counter_module();
    m.canonicalize();
    c.bench_function("view_ir/canonical_json", |b| b.iter(|| m.canonical_json()));
    c.bench_function("view_ir/canonicalize_idempotent", |b| {
        b.iter(|| {
            let mut copy = m.clone();
            copy.canonicalize();
            copy
        });
    });
    c.bench_function("view_ir/fingerprint_fragment", |b| {
        b.iter(|| m.fingerprint_fragment(FragmentId::new(0)));
    });
}

criterion_group!(benches, bench_canonical);
criterion_main!(benches);
