//! Solid TSX frontend: a Rust port of the dom-expressions passes over the
//! oxc AST (§6.1, FR-1).
//!
//! Spike scope (M0, S1): the `counter` component surface — elements, static
//! and dynamic attributes, class, static style objects, delegated events,
//! `{expr}` children, `Show`, `For`, `.map` children, ternaries, destructured
//! props, and `createSignal`/`createMemo`/`createEffect` declarations.
//! Anything outside the spike subset is a fail-closed diagnostic (`LIG-E001`).
//!
//! Pass structure mirrors dom-expressions: `is_dynamic` (in lig-expr) decides
//! static inlining; template extraction builds the `TemplateIr`; anchors are
//! assigned in template order (§6.1 `anchor.rs::assign`).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Shared test/bench fixture sources.
pub mod tests_support;

use lig_core::diagnostic::LigDiagnostic;
use lig_core::id::{ComponentId, ExprId, ReactionId, StrId};
use lig_core::span::Span;
use lig_expr::lower::to_span;
use lig_expr::{Lowering, Scope, ty_of_expr, value_kind_is_str};
use lig_ir::anchor::assign_anchor_paths;
use lig_ir::expr::{Expr, Lit, Ty};
use lig_ir::fragment::{
    ComponentIr, DynamicIr, DynamicKind, EventName, ForIr, FragmentIr, IfIr, PropBinding, PropDecl,
    SlotIr, StateDecl, StateKind,
};
use lig_ir::module::{ReactionDecl, ReactionKind, Tier, ViewModule};
use lig_ir::template::{AttrValue, NodeIr, StaticAttr, TemplateIr};
use lig_ir::{FileId, FragmentId, Frontend, StyleConstId, TemplateId};
use lig_style::camel_to_kebab;
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionBody, Declaration, Expression, Function, JSXAttributeItem, JSXAttributeName,
    JSXAttributeValue, JSXChild, JSXElementName, ModuleDeclaration, Statement, VariableDeclarator,
};
use oxc_parser::Parser;
use oxc_span::GetSpan;
use std::collections::BTreeSet;

/// Lower a Solid TSX source file into a canonical View IR module.
///
/// # Errors
/// All diagnostics (parse + spike admission) are returned together;
/// fail-closed — a module with any error diagnostic produces no output (BR-1).
pub fn lower_module(source: &str, path: &str) -> Result<ViewModule, Vec<LigDiagnostic>> {
    let mut module = ViewModule::new(Frontend::Solid);
    module.source_files.push(lig_ir::SourceFile {
        id: FileId::new(0),
        path: path.to_string(),
        hash: lig_core::hash::fnv1a64(source.as_bytes()),
    });

    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, oxc_span::SourceType::tsx()).parse();
    let diags: Vec<LigDiagnostic> = parsed
        .diagnostics
        .iter()
        .map(|d| {
            let span = d.labels.first().map_or(Span::PLACEHOLDER, |l| {
                Span::new(l.offset(), l.offset() + l.len())
            });
            LigDiagnostic::error("LIG-E001", format!("invalid syntax: {d}"), span)
        })
        .collect();

    // Exported components in declaration order.
    let component_fns: Vec<&Function<'_>> = parsed
        .program
        .body
        .iter()
        .filter_map(|stmt| match stmt {
            _ if let Some(ModuleDeclaration::ExportDeclaration(export)) =
                stmt.as_module_declaration() =>
            {
                match &export.declaration {
                    Declaration::FunctionDeclaration(f) => Some(&**f),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect();

    // Reserve component ids so JSX use-sites resolve before lowering.
    for f in &component_fns {
        let name =
            f.id.as_ref()
                .map_or("Anonymous", |i| i.name.as_str())
                .to_string();
        module.components.push(lig_ir::fragment::ComponentDecl {
            name: module.strings.intern(name),
            root: FragmentId::new(u32::MAX),
            props: vec![],
        });
    }

    let mut diags = diags;
    for (idx, f) in component_fns.iter().enumerate() {
        let root = lower_component(&mut module, f, &mut diags);
        module.components[idx].root = root;
        module.components[idx].props = props_of(&mut module, f);
        own_tree_contract(&module, root, &mut diags);
    }

    if diags
        .iter()
        .any(|d| d.severity == lig_core::diagnostic::Severity::Error)
    {
        return Err(diags);
    }
    unify_prop_types(&mut module);
    module.canonicalize();
    Ok(module)
}

/// The M10/M13 own-tree contract: slot outlets inside FOR bodies of a
/// component's own template deny with one precise reason (per-item slot
/// assembly is still the keyed-component follow-up for outlets). Before M10
/// the outlet lowered silently broken: the item template carried a
/// never-mounted outlet hole (nothing mounts fills/fallbacks per item).
/// Component uses admit since M13 — per-item instance identity: the
/// generated reconcile mirrors the keyed plan onto a `Vec<Option<Child>>`
/// instance store (build+attach on insert, teardown on remove, reposition
/// on move), so each live item owns its instance — DIRECT item-body uses
/// only. Component uses inside list-body BRANCHES stay denied (the sticky
/// `in_for` discipline: an If inside a list item is still per-item, and the
/// branch-context composition is the follow-up). If branches outside lists
/// admit — the per-mount identity story (rebuilt per mount, disposed on
/// unmount).
fn own_tree_contract(module: &ViewModule, root: FragmentId, diags: &mut Vec<LigDiagnostic>) {
    // The walk state: `(fragment, in_for, item_branch)`. `in_for` marks For
    // bodies (the outlet denial is sticky through nested branches);
    // `item_branch` marks branches hanging INSIDE a list body (the M13
    // component boundary — sticky through deeper branches too).
    let mut stack = vec![(root, false, false)];
    while let Some((frag_id, in_for, item_branch)) = stack.pop() {
        let Some(frag) = module.fragments.get(frag_id.get() as usize) else {
            continue;
        };
        for d in &frag.dynamics {
            let why = match (&d.kind, in_for, item_branch) {
                (DynamicKind::Component(_), _, true) => Some(
                    "component uses inside list item branches are not admitted — per-item \
                          instance identity covers the direct item-body use; branch-in-item \
                          composition is the follow-up (hoist the use out of the body)",
                ),
                (DynamicKind::Slot { .. }, true, _) => Some(
                    "slot outlets inside list item bodies are not admitted — per-item slot \
                          assembly is the keyed-component follow-up (hoist the outlet out of \
                          the body)",
                ),
                _ => None,
            };
            if let Some(why) = why {
                let span = d.span;
                diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    why.to_string(),
                    Span::new(span.0, span.1),
                ));
            }
            match &d.kind {
                DynamicKind::If(i) => {
                    stack.push((i.then, in_for, item_branch || in_for));
                    if let Some(el) = i.else_ {
                        stack.push((el, in_for, item_branch || in_for));
                    }
                }
                DynamicKind::For(fo) => stack.push((fo.body, true, false)),
                _ => {}
            }
        }
        for &c in &frag.children {
            stack.push((c, in_for, item_branch));
        }
    }
}

fn props_of(module: &mut ViewModule, f: &Function<'_>) -> Vec<PropDecl> {
    let _ = module;
    let Some(p) = f.params.items.first() else {
        return vec![];
    };
    let oxc_ast::ast::BindingPattern::ObjectPattern(obj) = &p.pattern else {
        return vec![];
    };
    obj.properties
        .iter()
        .filter_map(|prop| match &prop.key {
            oxc_ast::ast::PropertyKey::StaticIdentifier(id) => {
                let name = module.strings.intern(id.name.to_string());
                Some(PropDecl {
                    name,
                    ty: Ty::Str,
                    optional: false,
                })
            }
            _ => None,
        })
        .collect()
}

/// Names visible inside a component body (§2.3 spike substrate).
struct ComponentScope {
    /// Reactive state names.
    state: BTreeSet<String>,
    /// Non-reactive locals.
    local: BTreeSet<String>,
    /// Model fns (setters).
    model_fns: BTreeSet<String>,
    /// Setter → written state field (for `writes` masks).
    setter_targets: Vec<(String, String)>,
    /// Components visible in this module (declaration order — positions are
    /// `ComponentId`s).
    component_names: Vec<String>,
    /// Destructured prop names (for `children` slot detection, §7.4).
    prop_names: BTreeSet<String>,
    /// Event-object parameter inside a handler body (§6.1).
    event_param: Option<String>,
    /// `<For>` index accessor names visible in the current body (§7.5).
    index_accessors: BTreeSet<String>,
}

/// One open fragment during JSX lowering.
struct Frame {
    fragment: FragmentId,
    dynamics: Vec<DynamicIr>,
    anchors: Vec<Vec<u32>>,
    children: Vec<FragmentId>,
}

struct ComponentLowering<'a, 'm> {
    module: &'a mut ViewModule,
    diags: &'a mut Vec<LigDiagnostic>,
    scope: ComponentScope,
    branch_counter: u32,
    frames: Vec<Frame>,
    /// Set while lowering a `.map` body whose root element carries `key={…}`
    /// (§7.5): the root element's `key` attribute is consumed as the list key
    /// and must not become an item-template attribute. Consumed by the first
    /// `jsx_element` call (the body's root).
    skip_root_key: bool,
    _marker: std::marker::PhantomData<&'m ()>,
}

/// The raw `key` attribute detection result on a `.map` body element.
enum MapKeyAttr<'a> {
    /// No `key` attribute — the list stays unkeyed (Index diff).
    None,
    /// A `key` was found but is malformed (diagnostic already recorded).
    Invalid,
    /// `(attribute span, key expression source)`.
    Found(oxc_span::Span, &'a Expression<'a>),
}

fn lower_component(
    module: &mut ViewModule,
    f: &Function<'_>,
    diags: &mut Vec<LigDiagnostic>,
) -> FragmentId {
    let comp_name =
        f.id.as_ref()
            .map_or("Component", |i| i.name.as_str())
            .to_string();
    let comp_name_id = module.strings.intern(comp_name);
    let root = module.push_fragment(FragmentIr {
        name: comp_name_id,
        template: TemplateId::new(0),
        dynamics: vec![],
        children: vec![],
        props: vec![],
        returns: vec![],
    });

    let scope = scan_scope(module, f, root);
    let mut comp = ComponentLowering {
        module,
        diags,
        scope,
        branch_counter: 0,
        skip_root_key: false,
        frames: vec![Frame {
            fragment: root,
            dynamics: vec![],
            anchors: vec![],
            children: vec![],
        }],
        _marker: std::marker::PhantomData,
    };

    let mut jsx_root: Option<&Expression<'_>> = None;
    if let Some(body) = &f.body {
        for stmt in &body.statements {
            match stmt {
                Statement::VariableDeclaration(v) => {
                    for d in &v.declarations {
                        comp.lower_declarator(d);
                    }
                }
                Statement::ExpressionStatement(es) => {
                    // `model(() => { ... })` — the TSX-parseable spelling of
                    // the model!{} escape hatch (§6.5): statements inside the
                    // block get the model treatment (state + reactions).
                    if let Expression::CallExpression(call) = unwrap_parens(&es.expression)
                        && let Expression::Identifier(callee) = &call.callee
                    {
                        match callee.name.as_str() {
                            "model" => {
                                for arg in &call.arguments {
                                    let Some(Expression::ArrowFunctionExpression(arrow)) =
                                        arg.as_expression()
                                    else {
                                        continue;
                                    };
                                    let ArrowFunctionBody::FunctionBody(fb) = &arrow.body else {
                                        continue;
                                    };
                                    for inner in &fb.statements {
                                        if let Statement::VariableDeclaration(v) = inner {
                                            for d in &v.declarations {
                                                comp.lower_declarator(d);
                                            }
                                        }
                                    }
                                }
                            }
                            // A bare `createEffect(() => …)` / `watchEffect(…)`
                            // statement is the idiomatic spelling (§7.1) — it
                            // must not be dropped.
                            "createEffect" | "watchEffect" => {
                                comp.lower_effect_call(call);
                            }
                            _ => {}
                        }
                    }
                }
                Statement::ReturnStatement(r) => {
                    if let Some(arg) = &r.argument {
                        let arg = unwrap_parens(arg);
                        if matches!(arg, Expression::JSXElement(_) | Expression::JSXFragment(_)) {
                            jsx_root = Some(arg);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    match jsx_root {
        Some(jsx) => {
            let mut path = Vec::new();
            let root_node = comp.jsx_to_node(jsx, &mut path);
            let frame = comp.frames.last().expect("root frame");
            let tpl = comp.module.intern_template(TemplateIr::new(root_node));
            let fi = frame.fragment.get() as usize;
            comp.module.fragments[fi].template = tpl;
            comp.module.fragments[fi]
                .dynamics
                .clone_from(&frame.dynamics);
            comp.module.fragments[fi]
                .children
                .clone_from(&frame.children);
            assign_anchor_paths(comp.module, frame.fragment, &frame.anchors);
        }
        None => comp.diags.push(
            LigDiagnostic::error(
                "LIG-E001",
                "component has no JSX return (spike subset)",
                Span::new(f.span.start, f.span.end),
            )
            .with_help(
                "return a JSX element — the spike lowers exactly one view tree per component",
            ),
        ),
    }
    root
}

fn scan_scope(module: &mut ViewModule, f: &Function<'_>, root: FragmentId) -> ComponentScope {
    let mut scope = ComponentScope {
        state: BTreeSet::new(),
        local: BTreeSet::new(),
        model_fns: BTreeSet::new(),
        setter_targets: vec![],
        component_names: module
            .components
            .iter()
            .map(|c| module.strings.get(c.name).to_string())
            .collect::<Vec<_>>(),
        prop_names: BTreeSet::new(),
        event_param: None,
        index_accessors: BTreeSet::new(),
    };
    // Destructured props become Prop state decls (§7.6).
    if let Some(p) = f.params.items.first()
        && let oxc_ast::ast::BindingPattern::ObjectPattern(obj) = &p.pattern
    {
        for prop in &obj.properties {
            if let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &prop.value {
                let name = module.strings.intern(id.name.to_string());
                scope.state.insert(id.name.to_string());
                scope.prop_names.insert(id.name.to_string());
                module.states.push(StateDecl {
                    name,
                    owner: root,
                    kind: StateKind::Prop,
                    ty: Ty::Str,
                    init: None,
                });
            }
        }
    }
    if let Some(body) = &f.body {
        for stmt in &body.statements {
            let Statement::VariableDeclaration(v) = stmt else {
                continue;
            };
            for d in &v.declarations {
                scan_declarator_scope(module, d, &mut scope, root);
            }
        }
    }
    scope
}

fn scan_declarator_scope(
    _module: &mut ViewModule,
    d: &VariableDeclarator<'_>,
    scope: &mut ComponentScope,
    _root: FragmentId,
) {
    let Some(init) = &d.init else {
        if let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &d.id {
            scope.local.insert(id.name.to_string());
        }
        return;
    };
    let Expression::CallExpression(call) = init else {
        if let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &d.id {
            scope.local.insert(id.name.to_string());
        }
        return;
    };
    let Expression::Identifier(callee) = &call.callee else {
        return;
    };
    match callee.name.as_str() {
        "createSignal" => {
            if let oxc_ast::ast::BindingPattern::ArrayPattern(arr) = &d.id
                && let Some(Some(oxc_ast::ast::BindingPattern::BindingIdentifier(name))) =
                    arr.elements.first()
            {
                scope.state.insert(name.name.to_string());
                // The setter half of the destructure is optional (read-only
                // signals are admitted — §7.5 list reads without writes).
                if let Some(Some(oxc_ast::ast::BindingPattern::BindingIdentifier(setter))) =
                    arr.elements.get(1)
                {
                    scope.model_fns.insert(setter.name.to_string());
                    scope
                        .setter_targets
                        .push((setter.name.to_string(), name.name.to_string()));
                }
            }
        }
        "createMemo" | "computed" | "ref" => {
            if let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &d.id {
                scope.state.insert(id.name.to_string());
            }
        }
        _ => {
            if let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &d.id {
                scope.local.insert(id.name.to_string());
            }
        }
    }
}

impl ComponentLowering<'_, '_> {
    fn frame(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("open frame")
    }

    fn intern(&mut self, s: impl Into<String>) -> StrId {
        self.module.strings.intern(s)
    }

    fn lower_expr(&mut self, e: &Expression<'_>) -> Result<ExprId, LigDiagnostic> {
        let scope = Scope {
            state: self.scope.state.clone(),
            local: self.scope.local.clone(),
            model_fns: self.scope.model_fns.clone(),
            setter_targets: self.scope.setter_targets.iter().cloned().collect(),
            event_param: self.scope.event_param.clone(),
            index_accessors: self.scope.index_accessors.clone(),
            unwrapped_refs: BTreeSet::new(),
        };
        Lowering::new(self.module).lower(e, &scope)
    }

    fn push_reaction(&mut self, rx: ReactionDecl) -> ReactionId {
        let id = ReactionId::new(self.module.reactions.len() as u32);
        self.module.reactions.push(rx);
        id
    }

    fn push_dynamic(
        &mut self,
        kind: DynamicKind,
        expr: ExprId,
        reads: Vec<StrId>,
        path: &[u32],
        writes: Vec<StrId>,
        reaction_kind: ReactionKind,
        span: oxc_span::Span,
    ) {
        let dynamic_index = self.frame().dynamics.len() as u32;
        let owner = self.frame().fragment;
        let reaction_id = self.push_reaction(ReactionDecl {
            kind: match reaction_kind {
                ReactionKind::Bind { .. } => ReactionKind::Bind {
                    dynamic: dynamic_index,
                },
                other => other,
            },
            reads,
            writes,
            body: expr,
            owner,
        });
        let frame = self.frame();
        frame.anchors.push(path.to_vec());
        frame.dynamics.push(DynamicIr {
            anchor: lig_ir::Anchor::root(),
            kind,
            expr,
            reaction: Some(reaction_id),
            span: (span.start, span.end),
        });
    }

    fn new_fragment(&mut self, kind_name: &str) -> FragmentId {
        let name = format!("{kind_name}_{}", self.branch_counter);
        self.branch_counter += 1;
        let name_id = self.intern(name);
        self.module.push_fragment(FragmentIr {
            name: name_id,
            template: TemplateId::new(0),
            dynamics: vec![],
            children: vec![],
            props: vec![],
            returns: vec![],
        })
    }

    fn enter_fragment(&mut self, kind_name: &str) {
        let frag = self.new_fragment(kind_name);
        self.frames.push(Frame {
            fragment: frag,
            dynamics: vec![],
            anchors: vec![],
            children: vec![],
        });
    }

    fn exit_fragment(&mut self) -> FragmentId {
        let frame = self.frames.pop().expect("open frame");
        let fi = frame.fragment.get() as usize;
        self.module.fragments[fi]
            .dynamics
            .clone_from(&frame.dynamics);
        self.module.fragments[fi]
            .children
            .clone_from(&frame.children);
        assign_anchor_paths(self.module, frame.fragment, &frame.anchors);
        let frag = frame.fragment;
        self.frame().children.push(frag);
        frag
    }

    /// Build a fresh fragment from one JSX expression (branch/for bodies).
    fn fragment_from_jsx(&mut self, jsx: &Expression<'_>, kind_name: &str) -> FragmentId {
        // Cannot hold `jsx` across `enter_fragment` (module borrow), so copy
        // the span-based dispatch through a builder closure instead.
        let span = jsx.span();
        self.fragment_from(kind_name, span, Self::jsx_to_node_via_copy(jsx))
    }

    fn fragment_from(
        &mut self,
        kind_name: &str,
        _span: oxc_span::Span,
        build: impl FnOnce(&mut Self, &mut Vec<u32>) -> NodeIr,
    ) -> FragmentId {
        self.enter_fragment(kind_name);
        let mut path = Vec::new();
        let root = build(self, &mut path);
        let tpl = self.module.intern_template(TemplateIr::new(root));
        let frag = self.frame().fragment;
        let fi = frag.get() as usize;
        self.module.fragments[fi].template = tpl;
        self.exit_fragment()
    }

    /// Wrap jsx_to_node for deferred invocation.
    fn jsx_to_node_via_copy<'b>(
        jsx: &'b Expression<'b>,
    ) -> impl FnOnce(&mut Self, &mut Vec<u32>) -> NodeIr + 'b {
        move |comp, path| comp.jsx_to_node(jsx, path)
    }

    /// `createSignal(0)` / `createMemo(() => ...)` / `createEffect(() => ...)`
    /// normalize to StateDecls + Reactions (§6.3).
    // One arm per reactive declaration (§6.3 normalization table).
    fn lower_declarator(&mut self, d: &VariableDeclarator<'_>) {
        let Some(init) = &d.init else { return };
        let Expression::CallExpression(call) = init else {
            return;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        match callee.name.as_str() {
            "createSignal" => {
                let oxc_ast::ast::BindingPattern::ArrayPattern(arr) = &d.id else {
                    return;
                };
                let Some(Some(oxc_ast::ast::BindingPattern::BindingIdentifier(name_id))) =
                    arr.elements.first()
                else {
                    return;
                };
                let name = self.intern(name_id.name.as_str());
                let init_expr = call
                    .arguments
                    .first()
                    .and_then(|a| a.as_expression())
                    .map(|e| self.lower_expr(e));
                let ty = init_expr.as_ref().map_or(Ty::Int, |e| match e {
                    Ok(id) => ty_of_expr(self.module, *id),
                    Err(_) => Ty::Int,
                });
                let init_expr = match init_expr {
                    Some(Ok(e)) => Some(e),
                    Some(Err(diag)) => {
                        self.diags.push(diag);
                        None
                    }
                    None => None,
                };
                let root = self.frames[0].fragment;
                self.module.states.push(StateDecl {
                    name,
                    owner: root,
                    kind: StateKind::Signal,
                    ty,
                    init: init_expr,
                });
            }
            "createMemo" | "computed" | "ref" => {
                let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &d.id else {
                    return;
                };
                let name = self.intern(id.name.as_str());
                let kind = if callee.name.as_str() == "ref" {
                    StateKind::Signal
                } else {
                    StateKind::Memo
                };
                let root = self.frames[0].fragment;
                let mut memo_ty = Ty::Int;
                self.module.states.push(StateDecl {
                    name,
                    owner: root,
                    kind,
                    ty: memo_ty,
                    init: None,
                });
                if let Some(arg) = call.arguments.first().and_then(|a| a.as_expression()) {
                    let body = match arg {
                        Expression::ArrowFunctionExpression(arrow) => arrow_body_expr(&arrow.body),
                        other => Some(other),
                    };
                    if let Some(body) = body {
                        match self.lower_expr(body) {
                            Ok(body_id) => {
                                memo_ty = ty_of_expr(self.module, body_id);
                                self.module.states.last_mut().expect("just pushed").ty = memo_ty;
                                let reads = self.collect_reads(body_id);
                                let owner = self.frames[0].fragment;
                                self.push_reaction(ReactionDecl {
                                    kind: ReactionKind::Memo { target: name },
                                    reads: reads.clone(),
                                    writes: vec![name],
                                    body: body_id,
                                    owner,
                                });
                            }
                            Err(diag) => self.diags.push(diag),
                        }
                    }
                }
            }
            "createEffect" | "watchEffect" => self.lower_effect_call(call),
            _ => {}
        }
    }

    /// `createEffect(() => …)` / `watchEffect(…)` → a post-tier reaction
    /// (§7.1) — shared by the declarator and bare-statement spellings.
    fn lower_effect_call(&mut self, call: &oxc_ast::ast::CallExpression<'_>) {
        let Some(arg) = call.arguments.first().and_then(|a| a.as_expression()) else {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                "createEffect requires a callback argument",
                Span::new(call.span.start, call.span.end),
            ));
            return;
        };
        // Statement spellings (§7.1, M6): a block body lowers each expression
        // statement as its own effect reaction — reads/writes stay per
        // statement, the const schedule stays exact. Multi-statement bodies
        // were silently truncated to the first statement before M6.
        let stmts: Vec<&Expression<'_>> = match arg {
            Expression::ArrowFunctionExpression(arrow) => {
                if let Some(e) = arrow.body.as_expression() {
                    vec![e]
                } else {
                    let ArrowFunctionBody::FunctionBody(fb) = &arrow.body else {
                        unreachable!("a non-expression arrow body is a FunctionBody");
                    };
                    for stmt in &fb.statements {
                        if !matches!(stmt, Statement::ExpressionStatement(_)) {
                            self.diags.push(LigDiagnostic::error(
                                "LIG-E001",
                                "effect bodies admit expression statements only (declarations, `if`, and `return` are outside the subset)",
                                Span::new(stmt.span().start, stmt.span().end),
                            ));
                        }
                    }
                    fb.statements
                        .iter()
                        .filter_map(|s| match s {
                            Statement::ExpressionStatement(es) => Some(&es.expression),
                            _ => None,
                        })
                        .collect()
                }
            }
            other => vec![other],
        };
        if stmts.is_empty() {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                "effect bodies admit at least one expression statement",
                Span::new(arg.span().start, arg.span().end),
            ));
            return;
        }
        for body_src in stmts {
            match self.lower_expr(body_src) {
                Ok(body_id) => {
                    let reads = self.collect_reads(body_id);
                    let owner = self.frames[0].fragment;
                    self.push_reaction(ReactionDecl {
                        kind: ReactionKind::Effect { tier: Tier::Post },
                        reads,
                        writes: vec![],
                        body: body_id,
                        owner,
                    });
                }
                Err(diag) => self.diags.push(diag),
            }
        }
    }

    /// All `State` names an expr tree reads (dataflow edge extraction, D-3).
    fn collect_reads(&self, id: ExprId) -> Vec<StrId> {
        lig_ir::expr::collect_reads(self.module, id)
    }

    fn jsx_to_node(&mut self, jsx: &Expression<'_>, path: &mut Vec<u32>) -> NodeIr {
        match unwrap_parens(jsx) {
            Expression::JSXElement(el) => self.jsx_element(el, path),
            Expression::JSXFragment(frag) => {
                let mut children: Vec<NodeIr> = Vec::new();
                let mut inner = Vec::new();
                for child in &frag.children {
                    self.jsx_child(child, &mut inner, &mut children);
                }
                if children.len() == 1 {
                    children.into_iter().next().expect("one child")
                } else {
                    self.diags.push(
                        LigDiagnostic::error(
                            "LIG-E001",
                            "multi-root JSX returns are outside the spike subset",
                            Span::new(frag.span.start, frag.span.end),
                        )
                        .with_help("wrap the roots in a single element"),
                    );
                    NodeIr::Marker
                }
            }
            other => {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "expression in JSX root position is outside the spike subset",
                    Span::new(other.span().start, other.span().end),
                ));
                NodeIr::Marker
            }
        }
    }

    fn jsx_element(&mut self, el: &oxc_ast::ast::JSXElement<'_>, path: &mut Vec<u32>) -> NodeIr {
        let tag = match &el.opening_element.name {
            JSXElementName::Identifier(i) => i.name.to_string(),
            JSXElementName::IdentifierReference(i) => i.name.to_string(),
            _ => {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "namespaced/member JSX tags are outside the spike subset",
                    Span::new(el.opening_element.span.start, el.opening_element.span.end),
                ));
                return NodeIr::Marker;
            }
        };
        let is_component = tag.chars().next().is_some_and(|c| c.is_ascii_uppercase());
        if tag == "Show" && !self.scope.component_names.iter().any(|n| n == "Show") {
            return self.lower_show(el, path);
        }
        if tag == "For" && !self.scope.component_names.iter().any(|n| n == "For") {
            return self.lower_for_component(el, path);
        }
        if tag == "Slot" && !self.scope.component_names.iter().any(|n| n == "Slot") {
            return self.lower_slot_outlet(el, path);
        }
        if is_component {
            return self.lower_component_use(&tag, el, path);
        }

        let mut static_attrs: Vec<StaticAttr> = Vec::new();
        let mut style: Option<StyleConstId> = None;
        // A `.map` body root's `key` attribute is consumed as the list key
        // (§7.5) — it never becomes an item-template attribute.
        let skip_key_here = std::mem::take(&mut self.skip_root_key);

        for item in &el.opening_element.attributes {
            match item {
                JSXAttributeItem::Attribute(attr) => {
                    let JSXAttributeName::Identifier(name_id) = &attr.name else {
                        continue;
                    };
                    let name = name_id.name.to_string();
                    if skip_key_here && name == "key" {
                        continue;
                    }
                    let Some(value) = &attr.value else { continue };
                    match value {
                        JSXAttributeValue::StringLiteral(s) => {
                            let key = self.intern(&name);
                            let v = self.intern(s.value.as_str().unwrap_or(""));
                            static_attrs.push(StaticAttr {
                                key,
                                value: AttrValue::Str(v),
                            });
                        }
                        JSXAttributeValue::ExpressionContainer(c) => {
                            let Some(e) = c.expression.as_expression() else {
                                continue;
                            };
                            if name == "style"
                                && let Expression::ObjectExpression(obj) = e
                            {
                                match self.static_style_object(obj) {
                                    Ok(Some(id)) => {
                                        style = Some(id);
                                        continue;
                                    }
                                    Ok(None) => {
                                        // Dynamic style object: per-property
                                        // StyleProp dynamics (§8.2) + a Lint.
                                        if let Some(folded) =
                                            self.lower_dynamic_style_object(obj, path)
                                        {
                                            style = Some(folded);
                                        }
                                        continue;
                                    }
                                    Err(diag) => {
                                        self.diags.push(diag);
                                        continue;
                                    }
                                }
                            }
                            self.lower_attr_dynamic(&name, e, path);
                        }
                        _ => {
                            self.diags.push(LigDiagnostic::error(
                                "LIG-E001",
                                "attribute form outside the admitted subset",
                                Span::new(attr.span.start, attr.span.end),
                            ));
                        }
                    }
                }
                JSXAttributeItem::SpreadAttribute(sp) => {
                    self.lower_spread_attribute(sp, path, &mut static_attrs);
                }
            }
        }

        let mut children: Vec<NodeIr> = Vec::new();
        for child in &el.children {
            self.jsx_child(child, path, &mut children);
        }

        let tag_id = self.intern(&tag);
        NodeIr::Element {
            tag: tag_id,
            static_attrs,
            style,
            children,
        }
    }

    /// `style={{ display: "flex" }}` with all-literal values → const style.
    /// `Ok(None)` means the object is dynamic (caller lowers a Style dynamic).
    fn static_style_object(
        &mut self,
        obj: &oxc_ast::ast::ObjectExpression<'_>,
    ) -> Result<Option<StyleConstId>, LigDiagnostic> {
        let mut css = String::new();
        for prop in &obj.properties {
            let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) = prop else {
                return Ok(None);
            };
            let key = match &p.key {
                oxc_ast::ast::PropertyKey::StaticIdentifier(id) => id.name.to_string(),
                oxc_ast::ast::PropertyKey::StringLiteral(s) => {
                    s.value.as_str().unwrap_or("").to_string()
                }
                _ => return Ok(None),
            };
            let Expression::StringLiteral(v) = &p.value else {
                return Ok(None);
            };
            css.push_str(&camel_to_kebab(&key));
            css.push(':');
            css.push_str(v.value.as_str().unwrap_or(""));
            css.push(';');
        }
        let ir = lig_style::parse_declarations(&css, Span::PLACEHOLDER)?;
        let mut strings = std::mem::take(&mut self.module.strings);
        let style_const = lig_style::to_style_const(&ir, &mut strings);
        self.module.strings = strings;
        Ok(Some(self.module.intern_style(style_const)))
    }

    /// Dynamic style object: `style={{ color: c, "font-size": "14px" }}` —
    /// static-valued keys fold into a const style (returned); dynamic-valued
    /// keys lower to per-property `StyleProp` dynamics with a LIG-W102 Lint
    /// (§8.2).
    fn lower_dynamic_style_object(
        &mut self,
        obj: &oxc_ast::ast::ObjectExpression<'_>,
        path: &[u32],
    ) -> Option<StyleConstId> {
        let span = obj.span;
        self.module.lints.push(lig_ir::module::LintNote {
            code: "LIG-W102".to_string(),
            message:
                "style object with dynamic values re-created per update — prefer static classes"
                    .to_string(),
            start: span.start,
            end: span.end,
        });
        let mut static_css = String::new();
        for prop in &obj.properties {
            let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) = prop else {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "spread/computed keys in style objects are outside the admitted subset",
                    to_span(prop.span()),
                ));
                continue;
            };
            let key = match &p.key {
                oxc_ast::ast::PropertyKey::StaticIdentifier(id) => id.name.to_string(),
                oxc_ast::ast::PropertyKey::StringLiteral(s) => {
                    s.value.as_str().unwrap_or("").to_string()
                }
                _ => {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        "computed style keys are outside the admitted subset",
                        to_span(p.key.span()),
                    ));
                    continue;
                }
            };
            let kebab = camel_to_kebab(&key);
            let value = &p.value;
            match value {
                Expression::StringLiteral(s) => {
                    static_css.push_str(&kebab);
                    static_css.push(':');
                    static_css.push_str(s.value.as_str().unwrap_or(""));
                    static_css.push(';');
                }
                Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
                    static_css.push_str(&kebab);
                    static_css.push(':');
                    static_css.push_str(t.quasis.first().map_or("", |q| q.value.raw.as_str()));
                    static_css.push(';');
                }
                _ => {
                    let Ok(expr) = self.lower_expr(value) else {
                        continue;
                    };
                    // v1: CSS values are strings (§8.1 longhand values).
                    if value_kind_is_str(self.module, expr) {
                        let prop_id = self.intern(kebab);
                        let reads = self.collect_reads(expr);
                        self.push_dynamic(
                            DynamicKind::StyleProp { prop: prop_id },
                            expr,
                            reads,
                            path,
                            vec![],
                            ReactionKind::Bind { dynamic: 0 },
                            value.span(),
                        );
                    } else {
                        self.diags.push(LigDiagnostic::error(
                            "LIG-E001",
                            "style property values must lower to strings in v1 (format with a template literal)",
                            to_span(value.span()),
                        ));
                    }
                }
            }
        }
        if !static_css.is_empty()
            && let Ok(ir) = lig_style::parse_declarations(&static_css, Span::PLACEHOLDER)
        {
            let mut strings = std::mem::take(&mut self.module.strings);
            let style_const = lig_style::to_style_const(&ir, &mut strings);
            self.module.strings = strings;
            return Some(self.module.intern_style(style_const));
        }
        None
    }

    /// `{...attrs}`: object-literal spreads only (§6.1 spread row). Static
    /// values inline as template attrs; dynamic values lower as per-key
    /// dynamics; a dynamic-valued spread carries a LIG-W101 Lint.
    fn lower_spread_attribute(
        &mut self,
        sp: &oxc_ast::ast::JSXSpreadAttribute<'_>,
        path: &[u32],
        static_attrs: &mut Vec<StaticAttr>,
    ) {
        let Expression::ObjectExpression(obj) = unwrap_parens(&sp.argument) else {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                "spread of a non-literal object is outside the admitted subset (keys must be statically visible)",
                Span::new(sp.span.start, sp.span.end),
            )
            .with_help("write the attributes out, or spread an object literal"));
            return;
        };
        let mut dynamic = false;
        for prop in &obj.properties {
            let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) = prop else {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "spread objects with spread/computed keys are outside the admitted subset",
                    to_span(prop.span()),
                ));
                continue;
            };
            let key = match &p.key {
                oxc_ast::ast::PropertyKey::StaticIdentifier(id) => id.name.to_string(),
                oxc_ast::ast::PropertyKey::StringLiteral(s) => {
                    s.value.as_str().unwrap_or("").to_string()
                }
                _ => {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        "computed spread keys are outside the admitted subset",
                        to_span(p.key.span()),
                    ));
                    continue;
                }
            };
            let value = &p.value;
            match value {
                Expression::StringLiteral(s) => {
                    let k = self.intern(&key);
                    let v = self.intern(s.value.as_str().unwrap_or(""));
                    static_attrs.push(StaticAttr {
                        key: k,
                        value: AttrValue::Str(v),
                    });
                }
                Expression::BooleanLiteral(b) => {
                    let k = self.intern(&key);
                    static_attrs.push(StaticAttr {
                        key: k,
                        value: AttrValue::Bool(b.value),
                    });
                }
                Expression::NumericLiteral(n) => {
                    let k = self.intern(&key);
                    static_attrs.push(StaticAttr {
                        key: k,
                        value: AttrValue::Num(n.value),
                    });
                }
                _ => {
                    dynamic = true;
                    self.lower_attr_dynamic(&key, value, path);
                }
            }
        }
        if dynamic {
            self.module.lints.push(lig_ir::module::LintNote {
                code: "LIG-W101".to_string(),
                message: "dynamic spread on an element lowers to per-key update dynamics — prefer explicit attributes on hot elements".to_string(),
                start: sp.span.start,
                end: sp.span.end,
            });
        }
    }

    fn lower_attr_dynamic(&mut self, name: &str, e: &Expression<'_>, path: &[u32]) {
        if let Some(event) = name.strip_prefix("on").and_then(event_from_jsx_name) {
            self.lower_event(event, e, path);
            return;
        }
        let expr = match self.lower_expr(e) {
            Ok(x) => x,
            Err(d) => {
                self.diags.push(d);
                return;
            }
        };
        let kind = match name {
            "class" | "className" => DynamicKind::Class,
            "ref" => DynamicKind::Ref,
            "style" => DynamicKind::Style,
            "innerHTML" => DynamicKind::Html,
            other => DynamicKind::Attr {
                key: self.intern(other),
            },
        };
        let reads = self.collect_reads(expr);
        self.push_dynamic(
            kind,
            expr,
            reads,
            path,
            vec![],
            ReactionKind::Bind { dynamic: 0 },
            e.span(),
        );
    }

    fn lower_event(&mut self, event: EventName, e: &Expression<'_>, path: &[u32]) {
        let body: Option<&Expression<'_>> = match e {
            Expression::ArrowFunctionExpression(arrow) => match &arrow.body {
                ArrowFunctionBody::FunctionBody(fb) => match fb.statements.first() {
                    Some(Statement::ExpressionStatement(es)) => Some(&es.expression),
                    Some(Statement::ReturnStatement(r)) => r.argument.as_ref().map(unwrap_parens),
                    _ => None,
                },
                other => other.as_expression(),
            },
            other => Some(other),
        };
        let Some(body) = body else {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                "block-bodied event handlers are outside the admitted subset (use a single expression, or a model fn call)",
                Span::new(e.span().start, e.span().end),
            ));
            return;
        };
        // The handler's event parameter makes event-field reads visible
        // (§6.1: handlers compile to model fns with `&mut Ctx, Ev`).
        let mut event_param = None;
        if let Expression::ArrowFunctionExpression(arrow) = e
            && let Some(p) = arrow.params.items.first()
            && let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &p.pattern
        {
            event_param = Some(id.name.to_string());
        }
        self.scope.event_param = event_param;
        let lowered = self.lower_expr(body);
        self.scope.event_param = None;
        let expr = match lowered {
            Ok(x) => x,
            Err(d) => {
                self.diags.push(d);
                return;
            }
        };
        let mut writes = Vec::new();
        let targets = self.scope.setter_targets.clone();
        for (setter, target) in &targets {
            if self.references_setter(expr, setter) {
                writes.push(self.intern(target));
            }
        }
        self.push_dynamic(
            DynamicKind::Event {
                event,
                delegate: true,
            },
            expr,
            vec![], // dispatch-driven: never scheduled by reads (§7.1)
            path,
            writes,
            ReactionKind::Effect { tier: Tier::Post },
            e.span(),
        );
    }

    fn references_setter(&self, expr: ExprId, setter: &str) -> bool {
        let Expr::Call(fid, _) = self.module.exprs.get(expr) else {
            return false;
        };
        let Some(decl) = self.module.pure_fns.get(fid.get() as usize) else {
            return false;
        };
        self.module.strings.get(decl.name) == setter
    }

    fn lower_show(&mut self, el: &oxc_ast::ast::JSXElement<'_>, path: &[u32]) -> NodeIr {
        let cond_src = el.opening_element.attributes.iter().find_map(|item| {
            let JSXAttributeItem::Attribute(attr) = item else {
                return None;
            };
            let JSXAttributeName::Identifier(id) = &attr.name else {
                return None;
            };
            if id.name != "when" {
                return None;
            }
            match &attr.value {
                Some(JSXAttributeValue::ExpressionContainer(c)) => c.expression.as_expression(),
                _ => None,
            }
        });
        let Some(cond_src) = cond_src else {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                "<Show> without `when` is not lowerable",
                Span::new(el.opening_element.span.start, el.opening_element.span.end),
            ));
            return NodeIr::Marker;
        };
        let Ok(cond) = self.lower_expr(cond_src) else {
            return NodeIr::Marker;
        };
        // Branch fragments: a single element child becomes the branch
        // template root; anchors inside it are root-relative (fresh path
        // space). `fallback={<jsx/>}` lowers to the else fragment (C4) —
        // both frontends share the `branch_N` naming scheme.
        let element_child = el.children.iter().find_map(|c| match c {
            JSXChild::Element(e) => Some(&**e),
            _ => None,
        });
        let then_frag = if let Some(e) = element_child {
            let jsx_el = e;
            self.fragment_from("branch", el.span(), move |comp, path| {
                comp.jsx_element(jsx_el, path)
            })
        } else {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                "multi-element branch bodies are outside the spike subset",
                Span::new(el.span().start, el.span().end),
            ));
            self.enter_fragment("branch");
            self.exit_fragment()
        };
        let else_frag = el.opening_element.attributes.iter().find_map(|item| {
            let JSXAttributeItem::Attribute(attr) = item else {
                return None;
            };
            let JSXAttributeName::Identifier(id) = &attr.name else {
                return None;
            };
            if id.name != "fallback" {
                return None;
            }
            match &attr.value {
                Some(JSXAttributeValue::ExpressionContainer(c)) => {
                    match c.expression.as_expression() {
                        Some(Expression::JSXElement(elsb)) => Some(&**elsb),
                        _ => None,
                    }
                }
                _ => None,
            }
        });
        let else_frag = else_frag.map(|elsb| {
            self.fragment_from("branch", el.span(), move |comp, path| {
                comp.jsx_element(elsb, path)
            })
        });
        let reads = self.collect_reads(cond);
        self.push_dynamic(
            DynamicKind::If(IfIr {
                cond,
                then: then_frag,
                else_: else_frag,
            }),
            cond,
            reads,
            path,
            vec![],
            ReactionKind::Bind { dynamic: 0 },
            el.span(),
        );
        NodeIr::Marker
    }

    fn lower_for_component(&mut self, el: &oxc_ast::ast::JSXElement<'_>, path: &[u32]) -> NodeIr {
        let each = el.opening_element.attributes.iter().find_map(|item| {
            let JSXAttributeItem::Attribute(attr) = item else {
                return None;
            };
            let JSXAttributeName::Identifier(id) = &attr.name else {
                return None;
            };
            if id.name != "each" {
                return None;
            }
            match &attr.value {
                Some(JSXAttributeValue::ExpressionContainer(c)) => c.expression.as_expression(),
                _ => None,
            }
        });
        let Some(each_src) = each else {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                "<For> without `each` is not lowerable",
                Span::new(el.opening_element.span.start, el.opening_element.span.end),
            ));
            return NodeIr::Marker;
        };
        let Ok(source) = self.lower_expr(each_src) else {
            return NodeIr::Marker;
        };
        let mut item_name: Option<String> = None;
        let mut index_name: Option<String> = None;
        let mut body_jsx: Option<&Expression<'_>> = None;
        for child in &el.children {
            let JSXChild::ExpressionContainer(c) = child else {
                continue;
            };
            let Some(Expression::ArrowFunctionExpression(arrow)) = c.expression.as_expression()
            else {
                continue;
            };
            if let Some(p) = arrow.params.items.first()
                && let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &p.pattern
            {
                item_name = Some(id.name.to_string());
            }
            if let Some(p) = arrow.params.items.get(1)
                && let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &p.pattern
            {
                index_name = Some(id.name.to_string());
            }
            body_jsx = arrow_body_expr(&arrow.body);
        }
        let (Some(item), Some(body_src)) = (item_name, body_jsx) else {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                "<For> requires a function child `(item) => jsx`",
                Span::new(el.opening_element.span.start, el.opening_element.span.end),
            ));
            return NodeIr::Marker;
        };
        let item_id = self.intern(&item);
        self.scope.local.insert(item.clone());
        if let Some(i) = &index_name {
            // Solid's `<For>` index is an accessor: `i()` inside the body
            // reads the current index (§7.5).
            self.scope.local.insert(i.clone());
            self.scope.index_accessors.insert(i.clone());
        }
        let body_frag = self.fragment_from_jsx(body_src, "items");
        let index_id = index_name.as_ref().map(|i| self.intern(i));
        let key = match self.for_key_expr(el, &item) {
            Ok(k) => k,
            Err(d) => {
                self.diags.push(d);
                return NodeIr::Marker;
            }
        };
        let reads = self.collect_reads(source);
        self.push_dynamic(
            DynamicKind::For(ForIr {
                source,
                item: item_id,
                index: index_id,
                key,
                body: body_frag,
            }),
            source,
            reads,
            path,
            vec![],
            ReactionKind::Bind { dynamic: 0 },
            el.span(),
        );
        NodeIr::Marker
    }

    /// The `<For>` key expression (§7.5): `key={(item) => expr}` — a pure
    /// projection of the item (usually the identity). The arrow takes
    /// exactly the body's item binding (the key and the body describe the
    /// same item), and its body must not read component state: keys are
    /// identities, and a state-dependent key re-identifies items on
    /// unrelated writes. Absent `key` ⇒ index diff (unchanged v1 behavior).
    fn for_key_expr(
        &mut self,
        el: &oxc_ast::ast::JSXElement<'_>,
        item: &str,
    ) -> Result<Option<ExprId>, LigDiagnostic> {
        let el_span = Span::new(el.span().start, el.span().end);
        let mut found: Option<(&oxc_span::Span, &Expression<'_>)> = None;
        for attr_item in &el.opening_element.attributes {
            let JSXAttributeItem::Attribute(attr) = attr_item else {
                continue;
            };
            let JSXAttributeName::Identifier(id) = &attr.name else {
                continue;
            };
            if id.name != "key" {
                continue;
            }
            if found.is_some() {
                return Err(LigDiagnostic::error(
                    "LIG-E001",
                    "<For> takes at most one `key`",
                    el_span,
                ));
            }
            let attr_span = Span::new(attr.span.start, attr.span.end);
            let Some(JSXAttributeValue::ExpressionContainer(c)) = &attr.value else {
                return Err(LigDiagnostic::error(
                    "LIG-E001",
                    "<For> `key` must be an arrow over the item — key={(item) => expr}",
                    attr_span,
                ));
            };
            let Some(Expression::ArrowFunctionExpression(arrow)) = c.expression.as_expression()
            else {
                return Err(LigDiagnostic::error(
                    "LIG-E001",
                    "<For> `key` must be an arrow over the item — key={(item) => expr}",
                    attr_span,
                ));
            };
            let param_is_item = arrow.params.items.len() == 1
                && matches!(
                    arrow.params.items.first().map(|p| &p.pattern),
                    Some(oxc_ast::ast::BindingPattern::BindingIdentifier(b))
                        if b.name.as_str() == item
                );
            if !param_is_item {
                return Err(LigDiagnostic::error(
                    "LIG-E001",
                    format!("<For> `key` arrow must take exactly the item binding `{item}`"),
                    attr_span,
                ));
            }
            let Some(body_expr) = arrow_body_expr(&arrow.body) else {
                return Err(LigDiagnostic::error(
                    "LIG-E001",
                    "<For> `key` arrow body must be a single expression",
                    attr_span,
                ));
            };
            found = Some((&attr.span, body_expr));
        }
        let Some((attr_span, body_expr)) = found else {
            return Ok(None);
        };
        let expr = self.lower_expr(body_expr)?;
        if !self.collect_reads(expr).is_empty() {
            return Err(LigDiagnostic::error(
                "LIG-E001",
                "<For> `key` must be a pure function of the item — state reads \
                 re-identify items on unrelated writes",
                Span::new(attr_span.start, attr_span.end),
            ));
        }
        Ok(Some(expr))
    }

    /// The `<Slot>` outlet (§7.4, M6 — the ligature Solid spelling; real
    /// Solid passes slots as props, this compiler extends the tag set like
    /// `<Show>`/`<For>`): `<Slot name="header">fallback</Slot>` or a bare
    /// `<Slot />`. The children lower to the outlet's fallback fragment
    /// (shown when a use site provides no fill for the name).
    fn lower_slot_outlet(&mut self, el: &oxc_ast::ast::JSXElement<'_>, path: &[u32]) -> NodeIr {
        let name_attr = el.opening_element.attributes.iter().find_map(|item| {
            let JSXAttributeItem::Attribute(attr) = item else {
                return None;
            };
            let JSXAttributeName::Identifier(id) = &attr.name else {
                return None;
            };
            if id.name != "name" {
                return None;
            }
            match &attr.value {
                Some(JSXAttributeValue::StringLiteral(s)) => {
                    Some(s.value.as_str().unwrap_or("").to_string())
                }
                _ => None,
            }
        });
        // A non-static `name` is fail-closed (the outlet name is an identity,
        // not a binding).
        for item in &el.opening_element.attributes {
            let JSXAttributeItem::Attribute(attr) = item else {
                continue;
            };
            let JSXAttributeName::Identifier(id) = &attr.name else {
                continue;
            };
            if id.name == "name" && !matches!(attr.value, Some(JSXAttributeValue::StringLiteral(_)))
            {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "<Slot> `name` must be a static string — slot names are identities",
                    Span::new(el.opening_element.span.start, el.opening_element.span.end),
                ));
                return NodeIr::Marker;
            }
        }
        let slot_name = name_attr.unwrap_or_else(|| "default".to_string());
        let Ok(fallback) = self.slot_fallback_from_children(el) else {
            return NodeIr::Marker;
        };
        let slot_name_id = self.intern(slot_name);
        let null = self.module.insert_expr(Expr::Lit(Lit::Null));
        self.push_dynamic(
            DynamicKind::Slot {
                name: slot_name_id,
                fallback,
            },
            null,
            vec![],
            path,
            vec![],
            ReactionKind::Bind { dynamic: 0 },
            el.span(),
        );
        NodeIr::Marker
    }

    /// One element child of a slot outlet lowers to the fallback fragment;
    /// anything else (multi-element, bare text, dynamic content) is
    /// fail-closed. `None` = no fallback content.
    fn slot_fallback_from_children(
        &mut self,
        el: &oxc_ast::ast::JSXElement<'_>,
    ) -> Result<Option<FragmentId>, ()> {
        let element_children: Vec<&oxc_ast::ast::JSXElement<'_>> = el
            .children
            .iter()
            .filter_map(|c| match c {
                JSXChild::Element(e) => Some(&**e),
                _ => None,
            })
            .collect();
        let has_visible_text = el.children.iter().any(|c| match c {
            JSXChild::Text(t) => !t.value.as_str().trim().is_empty(),
            JSXChild::ExpressionContainer(c) => c.expression.as_expression().is_some(),
            _ => false,
        });
        if element_children.is_empty() {
            if has_visible_text {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "slot fallback content must be a single element (wrap the content in one element)",
                    Span::new(el.span().start, el.span().end),
                ));
                return Err(());
            }
            return Ok(None);
        }
        if element_children.len() > 1 {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                "multi-element slot fallbacks are outside the admitted subset (wrap them in one element)",
                Span::new(el.span().start, el.span().end),
            ));
            return Err(());
        }
        let child_el = element_children[0];
        let fill = self.fragment_from("slotfallback", el.span(), move |comp, path| {
            comp.jsx_element(child_el, path)
        });
        self.ensure_static_slot_content(fill, el.span());
        Ok(Some(fill))
    }

    /// The M10/M13 slot-content contract: fills and fallbacks mount as live
    /// fragments — scalar binds (text/class/attr/style-prop/html), events,
    /// refs, If/For dynamics (M8), component uses and nested outlets (M9),
    /// AND (M10) component uses and nested outlets inside BRANCH bodies of
    /// content: a remounted branch rebuilds its per-mount component instance
    /// (Option field, dispose on unmount), and the branch mounts its
    /// outlets' fills/fallbacks with it (branch-time slot assembly). Since
    /// M13, component uses admit inside LIST bodies of content too — per-item
    /// instance identity (the generated reconcile mirrors the keyed plan onto
    /// a `Vec<Option<Child>>` store). OUTLETS inside list bodies stay
    /// fail-closed: per-item slot assembly is still the keyed-component
    /// follow-up. Children/prop-pass/whole-style dynamics stay denied (M2),
    /// and event handlers inside list item bodies stay denied (the LIG-E118
    /// discipline — per-item dispatch needs keyed components).
    fn ensure_static_slot_content(&mut self, frag: FragmentId, span: oxc_span::Span) {
        self.content_contract(frag, false, span);
    }

    /// The recursive content walk: `in_list_item` marks fragments mounted as
    /// For bodies (their binds are the per-item replay's; events there are
    /// the LIG-E118 denial; components and outlets there are the per-item
    /// identity denial).
    fn content_contract(&mut self, frag: FragmentId, in_list_item: bool, span: oxc_span::Span) {
        let (dynamics, children) = {
            let f = &self.module.fragments[frag.get() as usize];
            (f.dynamics.clone(), f.children.clone())
        };
        for d in &dynamics {
            let denied: Option<String> = match &d.kind {
                // Scalar binds, refs, the structural constructs (M8),
                // component uses + nested outlets on content (M9), inside
                // content BRANCHES (M10 — per-mount identity), and inside
                // content LIST bodies (M13 — per-item instance identity) are
                // admitted.
                DynamicKind::Text
                | DynamicKind::Html
                | DynamicKind::Attr { .. }
                | DynamicKind::Class
                | DynamicKind::StyleProp { .. }
                | DynamicKind::Ref
                | DynamicKind::If(_)
                | DynamicKind::For(_)
                | DynamicKind::Component(_) => None,
                DynamicKind::Event { .. } => in_list_item.then(|| {
                    "event handlers inside list item bodies are not admitted in v1 — hoist the \
                     handler to the list level (the LIG-E118 discipline)"
                        .to_string()
                }),
                DynamicKind::Slot { .. } => in_list_item.then(|| {
                    "slot outlets inside list item bodies of slot content are not admitted — \
                     per-item slot assembly is the keyed-component follow-up (hoist the outlet out \
                     of the body)"
                        .to_string()
                }),
                DynamicKind::Children | DynamicKind::PropPass { .. } | DynamicKind::Style => Some(
                    "children/prop-pass/whole-style dynamics inside slot content are outside the \
                     admitted subset (M2)"
                        .to_string(),
                ),
            };
            if let Some(why) = denied {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    format!("slot fill/fallback content: {why}"),
                    Span::new(span.start, span.end),
                ));
            }
        }
        // Structural bodies recurse: If branches and For bodies keep the
        // scalar content contract (binds, events, refs compose); For bodies
        // additionally keep the per-item instance denials.
        let mut structural: Vec<(FragmentId, bool)> = Vec::new();
        {
            let f = &self.module.fragments[frag.get() as usize];
            for d in &f.dynamics {
                match &d.kind {
                    DynamicKind::If(i) => {
                        structural.push((i.then, false));
                        if let Some(el) = i.else_ {
                            structural.push((el, false));
                        }
                    }
                    DynamicKind::For(fo) => structural.push((fo.body, true)),
                    _ => {}
                }
            }
        }
        for child in children {
            if let Some((_, child_in_list)) = structural.iter().find(|(id, _)| *id == child) {
                self.content_contract(child, *child_in_list, span);
            } else {
                self.content_contract(child, in_list_item, span);
            }
        }
    }

    fn lower_component_use(
        &mut self,
        tag: &str,
        el: &oxc_ast::ast::JSXElement<'_>,
        path: &[u32],
    ) -> NodeIr {
        let Some(component_idx) = self.scope.component_names.iter().position(|n| n == tag) else {
            self.diags.push(
                LigDiagnostic::error(
                    "LIG-E100",
                    format!("unknown component `{tag}`"),
                    Span::new(el.span().start, el.span().end),
                )
                .with_help("export the component from this module"),
            );
            return NodeIr::Marker;
        };
        let mut props: Vec<PropBinding> = Vec::new();
        let mut attr_fills: Vec<SlotIr> = Vec::new();
        for item in &el.opening_element.attributes {
            let JSXAttributeItem::Attribute(attr) = item else {
                continue;
            };
            let JSXAttributeName::Identifier(name) = &attr.name else {
                continue;
            };
            // JSX-element-valued attributes fill the named slot of the same
            // name (§7.4, M6): `<Panel header={<h1>…</h1>}>` (braces — the
            // idiomatic JSX spelling) or `header=<h1>…</h1>` (bare). They are
            // slot content, not prop bindings.
            let element_fill = match &attr.value {
                Some(JSXAttributeValue::Element(el)) => Some((el, attr.span)),
                Some(JSXAttributeValue::ExpressionContainer(c)) => {
                    match c.expression.as_expression() {
                        Some(Expression::JSXElement(el)) => Some((el, attr.span)),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some((fill_el, fill_span)) = element_fill {
                if name.name == "children" {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        "`children={<…/>}` is not a slot fill — element children are the default slot",
                        Span::new(fill_span.start, fill_span.end),
                    ));
                    continue;
                }
                let fill_el: &oxc_ast::ast::JSXElement<'_> = fill_el;
                let fill = self.fragment_from("slotfill", el.span(), move |comp, path| {
                    comp.jsx_element(fill_el, path)
                });
                self.ensure_static_slot_content(fill, el.span());
                attr_fills.push(SlotIr {
                    name: self.intern(name.name.to_string()),
                    fallback: Some(fill),
                });
                continue;
            }
            let value = match &attr.value {
                Some(JSXAttributeValue::ExpressionContainer(c)) => {
                    let Some(e) = c.expression.as_expression() else {
                        continue;
                    };
                    match self.lower_expr(e) {
                        Ok(v) => v,
                        Err(d) => {
                            self.diags.push(d);
                            continue;
                        }
                    }
                }
                // Static string props: `title="Status"` (§7.6 — plain fields).
                Some(JSXAttributeValue::StringLiteral(s)) => {
                    let v = self.intern(s.value.as_str().unwrap_or(""));
                    self.module.insert_expr(Expr::Lit(Lit::Str(v)))
                }
                _ => continue,
            };
            props.push(PropBinding {
                prop: self.intern(name.name.to_string()),
                value,
            });
        }
        let mut reads: Vec<StrId> = props
            .iter()
            .flat_map(|p| self.collect_reads(p.value))
            .collect();
        reads.sort();
        reads.dedup();
        let prop_expr = props.first().map_or_else(
            || self.module.insert_expr(Expr::Lit(Lit::Null)),
            |p| p.value,
        );
        let mut slots: Vec<SlotIr> = attr_fills;
        let element_children: Vec<&oxc_ast::ast::JSXElement<'_>> = el
            .children
            .iter()
            .filter_map(|c| match c {
                JSXChild::Element(e) => Some(&**e),
                _ => None,
            })
            .collect();
        if !element_children.is_empty() {
            if element_children.len() > 1 {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "multi-element slot fills are outside the admitted subset (wrap them in one element)",
                    Span::new(el.span().start, el.span().end),
                ));
            } else if let Some(child_el) = element_children.first() {
                let jsx_el = *child_el;
                let fill = self.fragment_from("slotfill", el.span(), move |comp, path| {
                    comp.jsx_element(jsx_el, path)
                });
                self.ensure_static_slot_content(fill, el.span());
                slots.push(SlotIr {
                    name: self.intern("default"),
                    fallback: Some(fill),
                });
            }
        }
        let component = ComponentIr {
            component: ComponentId::new(component_idx as u32),
            props,
            slots,
        };
        self.push_dynamic(
            DynamicKind::Component(component),
            prop_expr,
            reads,
            path,
            vec![],
            ReactionKind::Bind { dynamic: 0 },
            el.span(),
        );
        NodeIr::Marker
    }

    fn jsx_child(&mut self, child: &JSXChild<'_>, path: &mut Vec<u32>, out: &mut Vec<NodeIr>) {
        match child {
            JSXChild::Element(el) => {
                path.push(out.len() as u32);
                let node = self.jsx_element(el, path);
                path.pop();
                out.push(node);
            }
            JSXChild::ExpressionContainer(c) => {
                let Some(e) = c.expression.as_expression() else {
                    return;
                };
                path.push(out.len() as u32);
                let node = self.jsx_child_expr(e, path);
                path.pop();
                out.push(node);
            }
            JSXChild::Text(t) => {
                let raw = t.value.as_str();
                if raw.trim().is_empty() {
                    return; // Solid drops whitespace-only text with a newline
                }
                let id = self.intern(raw.trim());
                out.push(NodeIr::Text { value: id });
            }
            JSXChild::Spread(_) => {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "JSX spread children are outside the spike subset",
                    Span::new(child.span().start, child.span().end),
                ));
            }
            JSXChild::Fragment(_) => {}
        }
    }

    fn jsx_child_expr(&mut self, e: &Expression<'_>, path: &[u32]) -> NodeIr {
        if let Expression::CallExpression(call) = e
            && let Expression::StaticMemberExpression(m) = &call.callee
            && m.property.name == "map"
        {
            return self.lower_map_call(&m.object, call, path);
        }
        if let Expression::ConditionalExpression(c) = e {
            // jsx_child_expr receives a shared path; ternary lowering does not
            // mutate it (branch fragments use fresh path spaces).
            let mut path_copy = path.to_vec();
            return self.lower_ternary(c, &mut path_copy);
        }
        // `children` (a destructured prop read) in child position is the
        // slot outlet (§7.4): the parent fills it at build time.
        if let Expression::Identifier(id) = unwrap_parens(e)
            && id.name == "children"
            && self.scope.prop_names.contains("children")
        {
            let null = self.module.insert_expr(Expr::Lit(Lit::Null));
            let slot_name = self.intern("default");
            self.push_dynamic(
                DynamicKind::Slot {
                    name: slot_name,
                    fallback: None,
                },
                null,
                vec![],
                path,
                vec![],
                ReactionKind::Bind { dynamic: 0 },
                e.span(),
            );
            return NodeIr::Marker;
        }
        let expr = match self.lower_expr(e) {
            Ok(x) => x,
            Err(d) => {
                self.diags.push(d);
                return NodeIr::Marker;
            }
        };
        let reads = self.collect_reads(expr);
        self.push_dynamic(
            DynamicKind::Text,
            expr,
            reads,
            path,
            vec![],
            ReactionKind::Bind { dynamic: 0 },
            e.span(),
        );
        NodeIr::Marker
    }

    fn lower_map_call(
        &mut self,
        source_expr: &Expression<'_>,
        call: &oxc_ast::ast::CallExpression<'_>,
        path: &[u32],
    ) -> NodeIr {
        let Ok(source) = self.lower_expr(source_expr) else {
            return NodeIr::Marker;
        };
        let Some(arg) = call.arguments.first().and_then(|a| a.as_expression()) else {
            return NodeIr::Marker;
        };
        let Expression::ArrowFunctionExpression(arrow) = arg else {
            self.diags.push(LigDiagnostic::error(
                "LIG-E114",
                "calls with callbacks are admitted only as `.map` in child position",
                Span::new(call.span.start, call.span.end),
            ));
            return NodeIr::Marker;
        };
        let mut item_name: Option<String> = None;
        if let Some(p) = arrow.params.items.first()
            && let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &p.pattern
        {
            item_name = Some(id.name.to_string());
        }
        let body_src = arrow_body_expr(&arrow.body);
        let (Some(item), Some(body_src)) = (item_name, body_src) else {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                "`.map` callbacks must return JSX directly (spike subset)",
                Span::new(call.span.start, call.span.end),
            ));
            return NodeIr::Marker;
        };
        let item_id = self.intern(&item);
        self.scope.local.insert(item.clone());
        // A `key` attribute on the mapped element (§7.5, M6) turns the list
        // keyed — the React-style spelling of `<For key>` / `:key`. It is
        // stripped from the item fragment: the key drives diffing, it is not
        // an attribute of the item template.
        let key_attr = self.map_key_attr(body_src, &item);
        if matches!(key_attr, MapKeyAttr::Invalid) {
            return NodeIr::Marker;
        }
        self.skip_root_key = matches!(key_attr, MapKeyAttr::Found(..));
        let body_frag = self.fragment_from_jsx(body_src, "items");
        self.skip_root_key = false;
        let key = match key_attr {
            MapKeyAttr::None => None,
            MapKeyAttr::Invalid => unreachable!("checked above"),
            MapKeyAttr::Found(attr_span, key_expr_src) => {
                match self.finish_map_key(attr_span, key_expr_src) {
                    Ok(k) => k,
                    Err(d) => {
                        self.diags.push(d);
                        return NodeIr::Marker;
                    }
                }
            }
        };
        let reads = self.collect_reads(source);
        self.push_dynamic(
            DynamicKind::For(ForIr {
                source,
                item: item_id,
                index: None,
                key,
                body: body_frag,
            }),
            source,
            reads,
            path,
            vec![],
            ReactionKind::Bind { dynamic: 0 },
            call.span,
        );
        NodeIr::Marker
    }

    /// Detect + validate the `key` attribute on a `.map` body element (raw
    /// AST walk — nothing is lowered yet): at most one, an expression (a
    /// static `key` gives every item the same identity), and an arrow form
    /// taking exactly the item binding.
    fn map_key_attr<'a>(&mut self, body_src: &'a Expression<'_>, item: &str) -> MapKeyAttr<'a> {
        let Expression::JSXElement(el) = unwrap_parens(body_src) else {
            return MapKeyAttr::None;
        };
        let mut found: Option<(oxc_span::Span, &'a Expression<'a>)> = None;
        for attr_item in &el.opening_element.attributes {
            let JSXAttributeItem::Attribute(attr) = attr_item else {
                continue;
            };
            let JSXAttributeName::Identifier(id) = &attr.name else {
                continue;
            };
            if id.name != "key" {
                continue;
            }
            if found.is_some() {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "`.map` bodies take at most one `key`",
                    Span::new(el.opening_element.span.start, el.opening_element.span.end),
                ));
                return MapKeyAttr::Invalid;
            }
            let Some(JSXAttributeValue::ExpressionContainer(c)) = &attr.value else {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "a static `key` gives every item the same identity — key={expr} over the item",
                    Span::new(attr.span.start, attr.span.end),
                ));
                return MapKeyAttr::Invalid;
            };
            let Some(e) = c.expression.as_expression() else {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "`key` must be an expression over the item",
                    Span::new(attr.span.start, attr.span.end),
                ));
                return MapKeyAttr::Invalid;
            };
            // The arrow form takes exactly the item binding — the key and the
            // body describe the same item (§7.5).
            if let Expression::ArrowFunctionExpression(arrow) = unwrap_parens(e) {
                let param_is_item = arrow.params.items.len() == 1
                    && matches!(
                        arrow.params.items.first().map(|p| &p.pattern),
                        Some(oxc_ast::ast::BindingPattern::BindingIdentifier(b))
                            if b.name.as_str() == item
                    );
                if !param_is_item {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        format!("`.map` `key` arrow must take exactly the item binding `{item}`"),
                        Span::new(attr.span.start, attr.span.end),
                    ));
                    return MapKeyAttr::Invalid;
                }
                if arrow_body_expr(&arrow.body).is_none() {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        "`.map` `key` arrow body must be a single expression",
                        Span::new(attr.span.start, attr.span.end),
                    ));
                    return MapKeyAttr::Invalid;
                }
            }
            found = Some((attr.span, e));
        }
        found.map_or(MapKeyAttr::None, |(span, e)| MapKeyAttr::Found(span, e))
    }

    /// Lower the `.map` key expression (already shape-validated in
    /// [`Self::map_key_attr`]). Keys are identities — state reads re-identify
    /// items on unrelated writes and fail closed (§7.5, ADR-018 §2).
    fn finish_map_key(
        &mut self,
        attr_span: oxc_span::Span,
        key_expr_src: &Expression<'_>,
    ) -> Result<Option<ExprId>, LigDiagnostic> {
        let body_expr = match unwrap_parens(key_expr_src) {
            Expression::ArrowFunctionExpression(arrow) => {
                arrow_body_expr(&arrow.body).expect("validated in map_key_attr")
            }
            other => other,
        };
        let expr = self.lower_expr(body_expr)?;
        if !self.collect_reads(expr).is_empty() {
            return Err(LigDiagnostic::error(
                "LIG-E001",
                "`key` must be a pure function of the item — state reads re-identify items on unrelated writes",
                Span::new(attr_span.start, attr_span.end),
            ));
        }
        Ok(Some(expr))
    }

    fn lower_ternary(
        &mut self,
        c: &oxc_ast::ast::ConditionalExpression<'_>,
        path: &mut Vec<u32>,
    ) -> NodeIr {
        let path = &*path;
        // Conditional fragments share the `branch_N` naming scheme across
        // both frontends (the canonical IR must not encode which surface
        // syntax produced a branch — ternary vs `Show` vs `v-if`).
        let Ok(cond) = self.lower_expr(&c.test) else {
            return NodeIr::Marker;
        };
        let then_frag = self.fragment_from_jsx(&c.consequent, "branch");
        let else_frag = self.fragment_from_jsx(&c.alternate, "branch");
        let reads = self.collect_reads(cond);
        self.push_dynamic(
            DynamicKind::If(IfIr {
                cond,
                then: then_frag,
                else_: Some(else_frag),
            }),
            cond,
            reads,
            path,
            vec![],
            ReactionKind::Bind { dynamic: 0 },
            c.span(),
        );
        NodeIr::Marker
    }
}

fn arrow_body_expr<'a, 'b>(body: &'a ArrowFunctionBody<'b>) -> Option<&'a Expression<'b>> {
    match body {
        ArrowFunctionBody::FunctionBody(fb) => match fb.statements.first() {
            Some(Statement::ReturnStatement(r)) => r.argument.as_ref().map(unwrap_parens),
            Some(Statement::ExpressionStatement(es)) => Some(&es.expression),
            _ => None,
        },
        other => other.as_expression(),
    }
}

fn unwrap_parens<'a, 'b>(e: &'a Expression<'b>) -> &'a Expression<'b> {
    match e {
        Expression::ParenthesizedExpression(p) => &p.expression,
        other => other,
    }
}

/// Post-pass: unify each component's prop types with the types its callers
/// bind (§7.6 — props are plain struct fields, typed once). String wins over
/// numeric (rendering is the dominant use; arithmetic props keep `Int` only
/// when every binder is numeric).
fn unify_prop_types(module: &mut ViewModule) {
    let uses: Vec<(usize, StrId, ExprId)> = module
        .fragments
        .iter()
        .flat_map(|f| f.dynamics.iter())
        .filter_map(|d| match &d.kind {
            DynamicKind::Component(c) => Some(c.clone()),
            _ => None,
        })
        .flat_map(|c| {
            c.props
                .into_iter()
                .map(move |p| (c.component.get() as usize, p.prop, p.value))
        })
        .collect();
    for (comp_idx, prop_name, value) in uses {
        let ty = ty_of_expr(module, value);
        let Some(decl) = module.components.get_mut(comp_idx) else {
            continue;
        };
        if let Some(p) = decl.props.iter_mut().find(|p| p.name == prop_name) {
            p.ty = ty;
        }
    }
}

fn event_from_jsx_name(rest: &str) -> Option<EventName> {
    match rest.to_ascii_lowercase().as_str() {
        "click" => Some(EventName::Click),
        "input" => Some(EventName::Input),
        "keydown" => Some(EventName::Keydown),
        "keyup" => Some(EventName::Keyup),
        "dblclick" => Some(EventName::Dblclick),
        "contextmenu" => Some(EventName::Contextmenu),
        "focusin" => Some(EventName::Focusin),
        "focusout" => Some(EventName::Focusout),
        "beforeinput" => Some(EventName::Beforeinput),
        "mousedown" => Some(EventName::Mousedown),
        "mouseenter" => Some(EventName::Mouseenter),
        "mouseleave" => Some(EventName::Mouseleave),
        "mousemove" => Some(EventName::Mousemove),
        "mouseout" => Some(EventName::Mouseout),
        "mouseover" => Some(EventName::Mouseover),
        "mouseup" => Some(EventName::Mouseup),
        "pointerdown" => Some(EventName::Pointerdown),
        "pointermove" => Some(EventName::Pointermove),
        "pointerout" => Some(EventName::Pointerout),
        "pointerover" => Some(EventName::Pointerover),
        "pointerup" => Some(EventName::Pointerup),
        "touchcancel" => Some(EventName::Touchcancel),
        "touchend" => Some(EventName::Touchend),
        "touchmove" => Some(EventName::Touchmove),
        "touchstart" => Some(EventName::Touchstart),
        _ => None,
    }
}
