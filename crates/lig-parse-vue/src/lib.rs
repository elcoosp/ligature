//! Vue SFC frontend: `<script setup lang="ts">` + `<template>` lowering to
//! the canonical View IR (§6.2, FR-1's second leg).
//!
//! The mapping is structural: `ref`/`computed`/`watchEffect`/`defineProps`
//! normalize to the same `StateDecl`s/`ReactionDecl`s as Solid's
//! `createSignal`/`createMemo`/`createEffect`/destructured props; templates
//! walk like the dom-expressions passes (static shape extraction, marker
//! holes, precomputed anchors). Anything outside the admitted subset
//! (ADR-014) is a fail-closed diagnostic (`LIG-E001`).
//!
//! Expressions are lowered at absolute byte offsets: every script and
//! template expression region is re-parsed out of a blank-padded copy of the
//! `.vue` source, so spans (and diagnostics) land in true source positions.
//!
//! Parity contract (§6.4): for equivalent UIs the two frontends produce
//! byte-identical canonical JSON modulo the parity-ignored fields — the
//! `frontend` header, source-file identity, and spans. Both legs of every
//! conformance fixture are pinned by `lig fixtures verify`.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod sfc;
pub mod template;

use lig_core::diagnostic::LigDiagnostic;
use lig_core::id::{ExprId, StrId};
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
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionBody, AssignmentOperator, BindingPattern, Expression, Statement,
    VariableDeclarator,
};
use oxc_parser::{Parser, ParserReturn};
use oxc_span::GetSpan;
use sfc::Section;
use std::collections::{BTreeMap, BTreeSet};
use template::{TplAttr, TplElement, TplNode, TplTextPart};

/// Lower a Vue SFC source file into a canonical View IR module.
///
/// # Errors
/// All diagnostics (SFC scan + script/template lowering) are returned
/// together; fail-closed — a module with any error diagnostic produces no
/// output (BR-1).
pub fn lower_module(source: &str, path: &str) -> Result<ViewModule, Vec<LigDiagnostic>> {
    let mut module = ViewModule::new(Frontend::Vue);
    module.source_files.push(lig_ir::SourceFile {
        id: FileId::new(0),
        path: path.to_string(),
        hash: lig_core::hash::fnv1a64(source.as_bytes()),
    });

    let sections = sfc::scan(source)?;
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .strip_suffix(".vue")
        .unwrap_or(path)
        .to_string();

    // Reserve component ids so template use-sites resolve before lowering
    // (the Solid frontend does the same over exported functions).
    let mut names: Vec<String> = Vec::with_capacity(sections.len());
    for (idx, section) in sections.iter().enumerate() {
        names.push(section_name(
            source,
            section,
            &stem,
            sections.len() > 1,
            idx,
        )?);
    }
    for name in &names {
        module.components.push(lig_ir::fragment::ComponentDecl {
            name: module.strings.intern(name.clone()),
            root: FragmentId::new(u32::MAX),
            props: vec![],
        });
    }

    let mut diags: Vec<LigDiagnostic> = Vec::new();
    for (idx, section) in sections.iter().enumerate() {
        let (root, props) = lower_component(&mut module, source, section, &names[idx], &mut diags);
        module.components[idx].root = root;
        module.components[idx].props = props;
        own_tree_contract(&module, root, &mut diags);
    }

    if diags
        .iter()
        .any(|d| d.severity == lig_core::diagnostic::Severity::Error)
    {
        return Err(diags);
    }
    module.canonicalize();
    Ok(module)
}

/// The M10 own-tree contract (mirrors the Solid leg): component uses and
/// slot outlets inside v-for bodies of a component's own template deny with
/// one precise reason. Before M10 both lowered silently broken: the item
/// template carried a never-mounted outlet hole, and a v-for body carrying
/// a component use built ONE eager instance field shared by every item, its
/// root never attached. v-if branches admit (per-mount identity).
fn own_tree_contract(module: &ViewModule, root: FragmentId, diags: &mut Vec<LigDiagnostic>) {
    let mut stack = vec![(root, false)];
    while let Some((frag_id, in_for)) = stack.pop() {
        let Some(frag) = module.fragments.get(frag_id.get() as usize) else {
            continue;
        };
        for d in &frag.dynamics {
            let why = match (&d.kind, in_for) {
                (DynamicKind::Component(_), true) => Some(
                    "component uses inside list item bodies are not admitted — per-item \
                          instance identity is the keyed-component story (hoist the use out of \
                          the body)",
                ),
                (DynamicKind::Slot { .. }, true) => Some(
                    "slot outlets inside list item bodies are not admitted — per-item slot \
                          assembly is the keyed-component story (hoist the outlet out of the \
                          body)",
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
                    stack.push((i.then, in_for));
                    if let Some(el) = i.else_ {
                        stack.push((el, in_for));
                    }
                }
                DynamicKind::For(fo) => stack.push((fo.body, true)),
                _ => {}
            }
        }
        for &c in &frag.children {
            stack.push((c, in_for));
        }
    }
}

/// The component name of one section: `defineOptions({ name })` or the
/// file stem's PascalCase (single-section SFCs only).
fn section_name(
    source: &str,
    section: &Section,
    stem: &str,
    multi: bool,
    idx: usize,
) -> Result<String, Vec<LigDiagnostic>> {
    let fallback = pascal_case(stem);
    let Some((s, e)) = section.script else {
        if multi {
            return Err(vec![
                LigDiagnostic::error(
                    "LIG-E001",
                    "multi-section modules must name every component with defineOptions",
                    Span::new(0, 1),
                )
                .with_help("add defineOptions({ name: \"…\" }) to each <script setup>"),
            ]);
        }
        return Ok(fallback);
    };
    let allocator = Allocator::default();
    let padded = blank_except(source, s, e);
    let parsed = Parser::new(&allocator, &padded, oxc_span::SourceType::ts()).parse();
    let mut diags: Vec<LigDiagnostic> = parse_diags(&parsed);
    let mut name = fallback;
    let mut found = false;
    for stmt in &parsed.program.body {
        let Statement::ExpressionStatement(es) = stmt else {
            continue;
        };
        let Expression::CallExpression(call) = &es.expression else {
            continue;
        };
        let Expression::Identifier(id) = &call.callee else {
            continue;
        };
        if id.name != "defineOptions" {
            continue;
        }
        match define_options_name(call) {
            Some(n) if !n.is_empty() => {
                name = n;
                found = true;
            }
            _ => diags.push(LigDiagnostic::error(
                "LIG-E001",
                "defineOptions admits exactly { name: \"…\" }",
                to_span(es.expression.span()),
            )),
        }
    }
    if multi && !found {
        diags.push(
            LigDiagnostic::error(
                "LIG-E001",
                format!(
                    "section {idx} needs defineOptions({{ name: \"…\" }}) in a multi-section module"
                ),
                Span::new(s as u32, e as u32),
            )
            .with_help("single-component SFCs take their name from the file stem"),
        );
    }
    if diags.is_empty() {
        Ok(name)
    } else {
        Err(diags)
    }
}

/// oxc parse errors → fail-closed `LIG-E001`s (same shape as the Solid
/// frontend's handling).
fn parse_diags(parsed: &ParserReturn<'_>) -> Vec<LigDiagnostic> {
    parsed
        .diagnostics
        .iter()
        .map(|d| {
            let span = d.labels.first().map_or(Span::PLACEHOLDER, |l| {
                Span::new(l.offset(), l.offset() + l.len())
            });
            LigDiagnostic::error("LIG-E001", format!("invalid syntax: {d}"), span)
        })
        .collect()
}

/// A copy of `source` with every byte outside `[keep]` blanked to spaces
/// (newlines preserved) — oxc parses at true absolute offsets, so spans and
/// diagnostics land in the original `.vue` coordinates.
fn blank_except(source: &str, keep_start: usize, keep_end: usize) -> String {
    let mut out: Vec<u8> = source
        .bytes()
        .map(|b| if b == b'\n' { b'\n' } else { b' ' })
        .collect();
    let end = keep_end.min(out.len());
    if keep_start < end {
        out[keep_start..end].copy_from_slice(&source.as_bytes()[keep_start..end]);
    }
    String::from_utf8(out).expect("byte-for-byte copy")
}

/// Parse one expression region and hand it to `f` (the arena lives only
/// inside this call — `f` must not return anything borrowing the arena).
fn with_parsed_expr<R>(
    source: &str,
    span: (u32, u32),
    f: impl FnOnce(&Expression<'_>) -> Result<R, LigDiagnostic>,
) -> Result<R, LigDiagnostic> {
    let padded = blank_except(source, span.0 as usize, span.1 as usize);
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &padded, oxc_span::SourceType::mjs()).parse();
    if !parsed.diagnostics.is_empty() {
        return Err(parse_diags(&parsed).remove(0));
    }
    match parsed.program.body.first() {
        Some(Statement::ExpressionStatement(es)) => f(unwrap_parens(&es.expression)),
        // A padded program that is only a string literal parses as a
        // directive prologue (e.g. `ref("a.png")` inits) — unwrap it.
        None if parsed.program.directives.len() == 1 => {
            // The directive's expression IS the string literal; re-box it
            // into the arena as a plain expression.
            let d = &parsed.program.directives[0];
            let alloc_ref: &Allocator = &allocator;
            let expr = oxc_ast::ast::Expression::StringLiteral(oxc_allocator::Box::new_in(
                d.expression.clone(),
                &alloc_ref,
            ));
            f(&expr)
        }
        _ => Err(LigDiagnostic::error(
            "LIG-E001",
            "template expressions must be single expressions",
            Span::new(span.0, span.1),
        )),
    }
}

/// Parens are transparent (the Solid frontend's `unwrap_parens` discipline).
fn unwrap_parens<'a, 'b>(e: &'a Expression<'b>) -> &'a Expression<'b> {
    match e {
        Expression::ParenthesizedExpression(p) => &p.expression,
        other => other,
    }
}

/// `defineOptions({ name: "X" })` → the name.
fn define_options_name(call: &oxc_ast::ast::CallExpression<'_>) -> Option<String> {
    use oxc_ast::ast::{ObjectPropertyKind, PropertyKey};
    let arg = call.arguments.first()?.as_expression()?;
    let Expression::ObjectExpression(obj) = arg else {
        return None;
    };
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        let key = match &p.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str().to_string(),
            PropertyKey::StringLiteral(s) => s.value.as_str().unwrap_or("").to_string(),
            _ => continue,
        };
        if key == "name"
            && let Expression::StringLiteral(v) = &p.value
        {
            return Some(v.value.as_str().unwrap_or("").to_string());
        }
    }
    None
}

/// `defineProps<{ a: string; b?: number }>()` → name/type table. Unknown
/// type annotations fail closed.
fn define_props_types(
    call: &oxc_ast::ast::CallExpression<'_>,
    span: Span,
) -> Result<std::collections::BTreeMap<String, Ty>, LigDiagnostic> {
    use oxc_ast::ast::{PropertyKey, TSSignature, TSType};
    let bad = || {
        LigDiagnostic::error(
            "LIG-E001",
            "defineProps admits exactly `defineProps<{ name: string | number | boolean }>()`",
            span,
        )
    };
    let generics = call.type_arguments.as_ref().ok_or_else(bad)?;
    let Some(TSType::TSTypeLiteral(lit)) = generics.params.first() else {
        return Err(bad());
    };
    let mut out = std::collections::BTreeMap::new();
    for member in &lit.members {
        let TSSignature::TSPropertySignature(sig) = member else {
            return Err(bad());
        };
        let name = match &sig.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str().to_string(),
            _ => return Err(bad()),
        };
        let ty = match sig.type_annotation.as_ref().map(|t| &t.type_annotation) {
            Some(TSType::TSStringKeyword(_)) => Ty::Str,
            Some(TSType::TSNumberKeyword(_)) => Ty::Int,
            Some(TSType::TSBooleanKeyword(_)) => Ty::Bool,
            _ => return Err(bad()),
        };
        out.insert(name, ty);
    }
    Ok(out)
}

/// `foo-bar` → `FooBar` (Vue's file-stem component naming).
fn pascal_case(s: &str) -> String {
    s.split(['-', '_'])
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut cs = p.chars();
            match cs.next() {
                Some(c) => c.to_ascii_uppercase().to_string() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// `set` + PascalCase — the synthesized setter name for `state = expr`
/// handler assignments (§6.3; matches the Solid `setX` convention).
fn setter_name(state: &str) -> String {
    format!("set{}", pascal_case(state))
}

/// Single-expression body of an arrow function.
fn arrow_body_expr<'a>(body: &'a ArrowFunctionBody<'a>) -> Option<&'a Expression<'a>> {
    match body {
        ArrowFunctionBody::FunctionBody(fb) => match fb.statements.first() {
            Some(Statement::ReturnStatement(r)) => r.argument.as_ref(),
            Some(Statement::ExpressionStatement(es)) => Some(&es.expression),
            _ => None,
        },
        other => other.as_expression(),
    }
}

// ---------------------------------------------------------------------------
// Component lowering
// ---------------------------------------------------------------------------

/// One open fragment during template lowering.
struct Frame {
    fragment: FragmentId,
    dynamics: Vec<DynamicIr>,
    anchors: Vec<Vec<u32>>,
    children: Vec<FragmentId>,
}

/// The `ref` attribute's target spelling (M12): the static form names the
/// placeholder local, the bound form lowers an expression, the bare form
/// has nothing to bind.
enum RefTarget {
    /// `ref="x"` — the target names the placeholder local directly.
    Name(String),
    /// `:ref="expr"` / `v-bind:ref="expr"` — the target is a bound
    /// expression (a plain identifier only).
    Expr((usize, usize)),
    /// Bare `ref` — no target; fail-closed.
    Bare,
}

/// A collected script declaration (source order, span-based so the script
/// parse can drop before materialization).
enum ScriptDecl {
    /// `const { … } = defineProps<{ … }>()` — name/type pairs.
    Props(Vec<(String, Ty)>),
    /// `const x = ref(init)` — the init expression's span.
    Ref(String, (u32, u32)),
    /// `const x = computed(() => expr)` — the getter body's span.
    Computed(String, (u32, u32)),
    /// `watchEffect(…)` — the body expression's span.
    Effect((u32, u32)),
    /// A named single-expression handler.
    Handler(String, Option<String>, (u32, u32)),
    /// `let x;` (M12) — a template-ref placeholder local. No state: the
    /// generated unit captures the element's `NodeId` into a dedicated
    /// `ref_x` field, exactly what the Solid leg's bare `let x;` skips to.
    Placeholder(String),
}

/// The per-component lowering state (mirrors the Solid frontend's frame
/// machinery: same fragment/anchor discipline, different surface syntax).
struct ComponentLowering<'d> {
    module: &'d mut ViewModule,
    diags: &'d mut Vec<LigDiagnostic>,
    /// The full `.vue` source (absolute offsets).
    source: &'d str,
    /// Component names visible in this module.
    component_names: Vec<String>,
    /// Reactive state names (refs, computeds, props).
    state: BTreeSet<String>,
    /// Vue refs/computeds read through `.value` in script contexts.
    unwrapped_refs: BTreeSet<String>,
    /// Non-reactive locals (For item/index bindings).
    local: BTreeSet<String>,
    /// `<For>` index accessors (`i()` reads).
    index_accessors: BTreeSet<String>,
    /// Named script handlers: name → (event param, body span).
    handlers: BTreeMap<String, (Option<String>, (u32, u32))>,
    /// Branch/for/slot fragment counter (per component, like Solid).
    branch_counter: u32,
    frames: Vec<Frame>,
    /// Whether the template carries a `<slot/>` outlet (§7.4).
    slot_seen: bool,
    /// The component's prop declarations, in materialization order.
    props_out: Vec<PropDecl>,
}

fn lower_component(
    module: &mut ViewModule,
    source: &str,
    section: &Section,
    comp_name: &str,
    diags: &mut Vec<LigDiagnostic>,
) -> (FragmentId, Vec<PropDecl>) {
    let comp_name_id = module.strings.intern(comp_name);
    let root = module.push_fragment(FragmentIr {
        name: comp_name_id,
        template: TemplateId::new(0),
        dynamics: vec![],
        children: vec![],
        props: vec![],
        returns: vec![],
    });

    // Collect the declarations from the script (source order), spans only.
    let decls: Vec<ScriptDecl> = section
        .script
        .map(|(s, e)| collect_decls(source, s, e, diags))
        .unwrap_or_default();

    // Materialize states props-first (the Solid scan_scope order), then
    // refs/computeds/effects in declaration order.
    let mut prop_names: BTreeSet<String> = BTreeSet::new();
    let mut props_out: Vec<PropDecl> = Vec::new();
    for decl in &decls {
        if let ScriptDecl::Props(props) = decl {
            for (name, ty) in props {
                prop_names.insert(name.clone());
                let name_id = module.strings.intern(name.clone());
                module.states.push(StateDecl {
                    name: name_id,
                    owner: root,
                    kind: StateKind::Prop,
                    ty: ty.clone(),
                    init: None,
                });
                props_out.push(PropDecl {
                    name: name_id,
                    ty: ty.clone(),
                    optional: false,
                });
            }
        }
    }

    let component_names: Vec<String> = module
        .components
        .iter()
        .map(|c| module.strings.get(c.name).to_string())
        .collect();

    let mut comp = ComponentLowering {
        module,
        diags,
        source,
        component_names,
        state: prop_names.clone(),
        unwrapped_refs: BTreeSet::new(),
        local: BTreeSet::new(),
        index_accessors: BTreeSet::new(),
        handlers: BTreeMap::new(),
        branch_counter: 0,
        frames: vec![Frame {
            fragment: root,
            dynamics: vec![],
            anchors: vec![],
            children: vec![],
        }],
        slot_seen: false,
        props_out,
    };

    for decl in decls {
        match decl {
            ScriptDecl::Props(_) => {}
            // M12: a placeholder local joins the non-reactive locals — the
            // template's `ref` attribute validates its target against this
            // set, and the expression lowering already resolves locals to
            // `Expr::Local` (the shape the codegen Ref arm captures). No
            // state, no reactions: the generated unit owns the capture.
            ScriptDecl::Placeholder(name) => {
                comp.local.insert(name);
            }
            ScriptDecl::Ref(name, init_span) => {
                let name_id = comp.module.strings.intern(name.clone());
                comp.state.insert(name.clone());
                comp.unwrapped_refs.insert(name.clone());
                let init_expr = match comp.lower_expr_at(init_span, None) {
                    Ok(e) => Some(e),
                    Err(d) => {
                        comp.diags.push(d);
                        None
                    }
                };
                let ty = init_expr.map_or(Ty::Int, |e| ty_of_expr(comp.module, e));
                comp.module.states.push(StateDecl {
                    name: name_id,
                    owner: root,
                    kind: StateKind::Signal,
                    ty,
                    init: init_expr,
                });
            }
            ScriptDecl::Computed(name, body_span) => {
                let name_id = comp.module.strings.intern(name.clone());
                comp.state.insert(name.clone());
                comp.unwrapped_refs.insert(name.clone());
                comp.module.states.push(StateDecl {
                    name: name_id,
                    owner: root,
                    kind: StateKind::Memo,
                    ty: Ty::Int,
                    init: None,
                });
                match comp.lower_expr_at(body_span, None) {
                    Ok(body_id) => {
                        let ty = ty_of_expr(comp.module, body_id);
                        comp.module.states.last_mut().expect("just pushed").ty = ty;
                        let reads = lig_ir::expr::collect_reads(comp.module, body_id);
                        let owner = comp.frames[0].fragment;
                        comp.push_reaction(ReactionDecl {
                            kind: ReactionKind::Memo { target: name_id },
                            reads,
                            writes: vec![name_id],
                            body: body_id,
                            owner,
                        });
                    }
                    Err(d) => comp.diags.push(d),
                }
            }
            ScriptDecl::Effect(body_span) => match comp.lower_effect_body_at(body_span) {
                Ok(body_id) => {
                    let reads = lig_ir::expr::collect_reads(comp.module, body_id);
                    let owner = comp.frames[0].fragment;
                    comp.push_reaction(ReactionDecl {
                        kind: ReactionKind::Effect { tier: Tier::Post },
                        reads,
                        writes: vec![],
                        body: body_id,
                        owner,
                    });
                }
                Err(d) => comp.diags.push(d),
            },
            ScriptDecl::Handler(name, param, body_span) => {
                comp.handlers.insert(name, (param, body_span));
            }
        }
    }

    // Template walk.
    let tpl_nodes = match template::parse(source, section.template) {
        Ok(nodes) => nodes,
        Err(ds) => {
            comp.diags.extend(ds);
            return (root, comp.props_out);
        }
    };
    let root_node = comp.lower_root(&tpl_nodes);
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

    // The synthesized `children` prop rides last (§7.4 slot outlet).
    if comp.slot_seen {
        let children = comp.module.strings.intern("children");
        comp.module.states.push(StateDecl {
            name: children,
            owner: root,
            kind: StateKind::Prop,
            ty: Ty::Str,
            init: None,
        });
        comp.props_out.push(PropDecl {
            name: children,
            ty: Ty::Str,
            optional: false,
        });
    }
    (root, comp.props_out)
}

/// Walk the parsed script statements into span-based declarations.
fn collect_decls(
    source: &str,
    start: usize,
    end: usize,
    diags: &mut Vec<LigDiagnostic>,
) -> Vec<ScriptDecl> {
    let allocator = Allocator::default();
    let padded = blank_except(source, start, end);
    let parsed = Parser::new(&allocator, &padded, oxc_span::SourceType::ts()).parse();
    diags.extend(parse_diags(&parsed));
    let mut out = Vec::new();
    for stmt in &parsed.program.body {
        match stmt {
            Statement::ImportDeclaration(_) => {}
            Statement::VariableDeclaration(v) => {
                for d in &v.declarations {
                    collect_declarator(d, &mut out, diags);
                }
            }
            Statement::FunctionDeclaration(f) => {
                let Some(id) = &f.id else { continue };
                match (handler_body_span(f), fn_event_param(f)) {
                    (Some(body), param) => out.push(ScriptDecl::Handler(
                        id.name.as_str().to_string(),
                        param,
                        body,
                    )),
                    (None, _) => diags.push(
                        LigDiagnostic::error(
                            "LIG-E001",
                            "handler function bodies must be a single expression or return",
                            to_span(f.span),
                        )
                        .with_help("extract multi-statement logic into a computed"),
                    ),
                }
            }
            Statement::ExpressionStatement(es) => match &es.expression {
                Expression::CallExpression(call) if matches!(&call.callee, Expression::Identifier(id) if id.name == "defineOptions") =>
                {
                    // Consumed during section naming.
                }
                Expression::CallExpression(call) if matches!(&call.callee, Expression::Identifier(id) if id.name == "watchEffect") =>
                {
                    match call.arguments.first().and_then(|a| a.as_expression()) {
                        Some(Expression::ArrowFunctionExpression(arrow)) => {
                            if let Some(body) = arrow.body.as_expression() {
                                out.push(ScriptDecl::Effect((body.span().start, body.span().end)));
                            } else {
                                // Block body (M6): one effect reaction per
                                // expression statement — the Vue spelling of
                                // the Solid multi-statement `createEffect`.
                                let ArrowFunctionBody::FunctionBody(fb) = &arrow.body else {
                                    unreachable!("a non-expression arrow body is a FunctionBody");
                                };
                                let mut admitted = 0usize;
                                for stmt in &fb.statements {
                                    match stmt {
                                        Statement::ExpressionStatement(es) => {
                                            out.push(ScriptDecl::Effect((
                                                es.expression.span().start,
                                                es.expression.span().end,
                                            )));
                                            admitted += 1;
                                        }
                                        _ => diags.push(LigDiagnostic::error(
                                            "LIG-E001",
                                            "watchEffect bodies admit expression statements only (declarations, `if`, and `return` are outside the subset)",
                                            to_span(stmt.span()),
                                        )),
                                    }
                                }
                                if admitted == 0 {
                                    diags.push(LigDiagnostic::error(
                                        "LIG-E001",
                                        "watchEffect bodies admit at least one expression statement",
                                        to_span(arrow.span),
                                    ));
                                }
                            }
                        }
                        Some(other) => {
                            out.push(ScriptDecl::Effect((other.span().start, other.span().end)));
                        }
                        None => diags.push(LigDiagnostic::error(
                            "LIG-E001",
                            "watchEffect requires an argument",
                            to_span(call.span),
                        )),
                    }
                }
                other => {
                    diags.push(
                        LigDiagnostic::error(
                            "LIG-E001",
                            "statement outside the admitted <script setup> subset",
                            to_span(other.span()),
                        )
                        .with_help(
                            "admitted: ref/computed/defineProps declarations, watchEffect, named handlers, defineOptions",
                        ),
                    );
                }
            },
            _ => {
                diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "statement outside the admitted <script setup> subset",
                    to_span(stmt.span()),
                ));
            }
        }
    }
    out
}

/// One declarator → one collected declaration.
fn collect_declarator(
    d: &VariableDeclarator<'_>,
    out: &mut Vec<ScriptDecl>,
    diags: &mut Vec<LigDiagnostic>,
) {
    // `let x;` — the template-ref placeholder (M12). The Solid leg's bare
    // `let x;` lowers to nothing (the ref capture owns the name); the Vue
    // leg admits the declarator explicitly and records the local so the
    // template's `ref` attribute can validate its target against a real
    // declaration instead of silently accepting any spelling.
    let Some(init) = &d.init else {
        let BindingPattern::BindingIdentifier(name_id) = &d.id else {
            diags.push(LigDiagnostic::error(
                "LIG-E001",
                "declarations without initializers are outside the admitted subset",
                to_span(d.span),
            ));
            return;
        };
        out.push(ScriptDecl::Placeholder(name_id.name.as_str().to_string()));
        return;
    };
    let Expression::CallExpression(call) = init else {
        diags.push(
            LigDiagnostic::error(
                "LIG-E001",
                "only ref/computed/defineProps initializers are admitted in <script setup>",
                to_span(init.span()),
            )
            .with_help("other values belong in computeds or handlers (ADR-014)"),
        );
        return;
    };
    let Expression::Identifier(callee) = &call.callee else {
        diags.push(LigDiagnostic::error(
            "LIG-E001",
            "calls outside ref/computed/defineProps are outside the admitted subset",
            to_span(call.span),
        ));
        return;
    };
    let callee_name = callee.name.as_str();
    if matches!(&d.id, BindingPattern::ObjectPattern(_)) {
        if callee_name != "defineProps" {
            diags.push(LigDiagnostic::error(
                "LIG-E001",
                "destructuring is admitted only for defineProps",
                to_span(d.id.span()),
            ));
            return;
        }
        let Ok(props) = define_props_types(call, to_span(call.span)) else {
            return; // diagnostic already recorded
        };
        let BindingPattern::ObjectPattern(obj) = &d.id else {
            unreachable!("checked above");
        };
        let mut names: Vec<(String, Ty)> = Vec::new();
        for prop in &obj.properties {
            match &prop.value {
                BindingPattern::BindingIdentifier(id) => {
                    let n = id.name.as_str().to_string();
                    match props.get(&n) {
                        Some(ty) => names.push((n, ty.clone())),
                        None => diags.push(LigDiagnostic::error(
                            "LIG-E001",
                            format!("prop `{n}` is destructured but not declared in defineProps"),
                            to_span(prop.value.span()),
                        )),
                    }
                }
                _ => diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "props must destructure plain identifiers",
                    to_span(prop.value.span()),
                )),
            }
        }
        if !names.is_empty() {
            out.push(ScriptDecl::Props(names));
        }
        return;
    }
    let BindingPattern::BindingIdentifier(name_id) = &d.id else {
        diags.push(LigDiagnostic::error(
            "LIG-E001",
            "declarations must bind plain identifiers",
            to_span(d.id.span()),
        ));
        return;
    };
    let name = name_id.name.as_str().to_string();
    match callee_name {
        "ref" => {
            let Some(arg) = call.arguments.first().and_then(|a| a.as_expression()) else {
                diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "ref() requires an initializer expression",
                    to_span(call.span),
                ));
                return;
            };
            out.push(ScriptDecl::Ref(name, (arg.span().start, arg.span().end)));
        }
        "computed" => {
            let Some(arg) = call.arguments.first().and_then(|a| a.as_expression()) else {
                diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "computed() requires a getter argument",
                    to_span(call.span),
                ));
                return;
            };
            let Expression::ArrowFunctionExpression(arrow) = arg else {
                diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "computed() admits only `() => expr` getters",
                    to_span(arg.span()),
                ));
                return;
            };
            let Some(body) = arrow_body_expr(&arrow.body) else {
                diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "computed getters must be a single expression",
                    to_span(arrow.span),
                ));
                return;
            };
            out.push(ScriptDecl::Computed(
                name,
                (body.span().start, body.span().end),
            ));
        }
        other => {
            diags.push(
                LigDiagnostic::error(
                    "LIG-E001",
                    format!("`{other}(...)` is outside the admitted <script setup> subset"),
                    to_span(call.span),
                )
                .with_help("admitted initializers: ref, computed, defineProps"),
            );
        }
    }
}

/// The single-expression body span of a function declaration.
fn handler_body_span(f: &oxc_ast::ast::Function<'_>) -> Option<(u32, u32)> {
    let stmts = &f.body.as_ref()?.statements;
    let expr = match stmts.first()? {
        Statement::ReturnStatement(r) => r.argument.as_ref()?,
        Statement::ExpressionStatement(es) => {
            if stmts.len() != 1 {
                return None;
            }
            &es.expression
        }
        _ => return None,
    };
    Some((expr.span().start, expr.span().end))
}

/// A function declaration's first parameter name (the event parameter).
fn fn_event_param(f: &oxc_ast::ast::Function<'_>) -> Option<String> {
    let p = f.params.items.first()?;
    match &p.pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str().to_string()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Template walking (the dom-expressions pass structure over Vue syntax)
// ---------------------------------------------------------------------------

impl ComponentLowering<'_> {
    fn frame(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("open frame")
    }

    fn intern(&mut self, s: impl Into<String>) -> StrId {
        self.module.strings.intern(s)
    }

    fn scope(&self, event_param: Option<String>) -> Scope {
        Scope {
            state: self.state.clone(),
            local: self.local.clone(),
            // Vue synthesizes setters at assignment sites; no source-level
            // model-fn names exist.
            model_fns: BTreeSet::new(),
            setter_targets: BTreeMap::new(),
            event_param,
            index_accessors: self.index_accessors.clone(),
            unwrapped_refs: self.unwrapped_refs.clone(),
        }
    }

    /// Lower one oxc expression with a scope (`event_param` enables `$event`
    /// field reads inside handler statements, §6.1).
    fn lower_expr(
        &mut self,
        e: &Expression<'_>,
        event_param: Option<String>,
    ) -> Result<ExprId, LigDiagnostic> {
        let scope = self.scope(event_param);
        Lowering::new(self.module).lower(e, &scope)
    }

    /// Lower the expression at an absolute span of the `.vue` source.
    fn lower_expr_at(
        &mut self,
        span: (u32, u32),
        event_param: Option<String>,
    ) -> Result<ExprId, LigDiagnostic> {
        let source = self.source;
        with_parsed_expr(source, span, |expr| {
            let scope = self.scope(event_param);
            Lowering::new(self.module).lower(expr, &scope)
        })
    }

    fn push_reaction(&mut self, rx: ReactionDecl) {
        self.module.reactions.push(rx);
    }

    /// Push a dynamic into the open frame (+ its reaction, + the anchor
    /// path record) — the Solid frontend's discipline, kept identical.
    fn push_dynamic(
        &mut self,
        kind: DynamicKind,
        expr: ExprId,
        reads: Vec<StrId>,
        path: &[u32],
        writes: Vec<StrId>,
        reaction_kind: ReactionKind,
        span: (u32, u32),
    ) {
        let dynamic_index = self.frame().dynamics.len() as u32;
        let owner = self.frame().fragment;
        let reaction_id = self.module.reactions.len() as u32;
        self.push_reaction(ReactionDecl {
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
            reaction: Some(lig_core::id::ReactionId::new(reaction_id)),
            span: (span.0, span.1),
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

    /// Build a fresh fragment from one template element (branch/for/slot
    /// bodies) — enter, build, exit, and register under the parent frame.
    /// The structural directives (`v-if`/`v-else-if`/`v-else`/`v-for`) are
    /// stripped: the fragment body is the element *without* its directive.
    fn fragment_from_elem(&mut self, el: &TplElement, kind_name: &str) -> FragmentId {
        let stripped = strip_directives(el);
        let el = &stripped;
        let name = format!("{kind_name}_{}", self.branch_counter);
        self.branch_counter += 1;
        let name_id = self.intern(name);
        let frag = self.module.push_fragment(FragmentIr {
            name: name_id,
            template: TemplateId::new(0),
            dynamics: vec![],
            children: vec![],
            props: vec![],
            returns: vec![],
        });
        self.frames.push(Frame {
            fragment: frag,
            dynamics: vec![],
            anchors: vec![],
            children: vec![],
        });
        let mut path = Vec::new();
        // M10: the branch/body ROOT dispatches component and slot tags —
        // `lower_element` does not dispatch the tag, so a `v-if` on a
        // component use (or an outlet) lowered the branch root as a plain
        // element and the use/outlet silently vanished (the fail-open shape
        // the M9 content-root dispatch closed for content roots).
        let root = if is_component_tag(&el.tag) {
            self.lower_component_use(el, &[])
        } else if el.tag == "slot" {
            self.lower_slot(el, &[])
        } else {
            self.lower_element(el, &mut path)
        };
        let tpl = self.module.intern_template(TemplateIr::new(root));
        let fi = frag.get() as usize;
        self.module.fragments[fi].template = tpl;
        let frame = self.frames.last().expect("open frame");
        self.module.fragments[fi]
            .dynamics
            .clone_from(&frame.dynamics);
        self.module.fragments[fi]
            .children
            .clone_from(&frame.children);
        assign_anchor_paths(self.module, frag, &frame.anchors);
        self.frames.pop();
        self.frame().children.push(frag);
        frag
    }

    /// A holder fragment for nested `v-else-if` chain nodes: Marker template
    /// carrying one `If` dynamic (C4 — chains nest).
    fn if_holder_fragment(
        &mut self,
        cond: ExprId,
        then: FragmentId,
        else_: Option<FragmentId>,
        span: (u32, u32),
    ) -> FragmentId {
        let holder = self.new_fragment("branch");
        self.frames.push(Frame {
            fragment: holder,
            dynamics: vec![],
            anchors: vec![],
            children: vec![],
        });
        let reads = lig_ir::expr::collect_reads(self.module, cond);
        self.push_dynamic(
            DynamicKind::If(IfIr { cond, then, else_ }),
            cond,
            reads,
            &[],
            vec![],
            ReactionKind::Bind { dynamic: 0 },
            span,
        );
        let frame = self.frames.last().expect("open frame");
        let fi = holder.get() as usize;
        self.module.fragments[fi]
            .dynamics
            .clone_from(&frame.dynamics);
        self.module.fragments[fi]
            .children
            .clone_from(&frame.children);
        assign_anchor_paths(self.module, holder, &frame.anchors);
        self.module.fragments[fi].template =
            self.module.intern_template(TemplateIr::new(NodeIr::Marker));
        self.frames.pop();
        self.frame().children.push(holder);
        holder
    }

    // -- root ---------------------------------------------------------------

    /// The template root: exactly one logical root — a plain element, a
    /// `v-if` chain, a `v-for` element, a component use, or a slot outlet.
    fn lower_root(&mut self, nodes: &[TplNode]) -> NodeIr {
        let mut out: Vec<NodeIr> = Vec::new();
        let mut i = 0usize;
        let mut roots = 0usize;
        while i < nodes.len() {
            match &nodes[i] {
                TplNode::Text(t) if self.is_ws_text(t) => {
                    i += 1;
                }
                TplNode::Text(t) => {
                    roots += 1;
                    self.lower_text(t, &mut Vec::new(), &mut out);
                    i += 1;
                }
                TplNode::Elem(el) => {
                    roots += 1;
                    // M10: structural directives dispatch FIRST — on ANY tag
                    // (element, component, slot). Before M10 a `v-if`/`v-for`
                    // on a component or slot tag fell through to the
                    // component/outlet dispatch and the directive silently
                    // dropped (the same fail-open shape the content-root
                    // dispatch closed in M9).
                    if let Some(TplAttr::For { item, index, list }) =
                        el.attrs.iter().find(|a| matches!(a, TplAttr::For { .. }))
                    {
                        let (item, index, list) = (item.clone(), index.as_deref(), *list);
                        out.push(self.lower_for(el, &item, index, list, &[]));
                        i += 1;
                        continue;
                    }
                    if let Some(TplAttr::If { expr }) =
                        el.attrs.iter().find(|a| matches!(a, TplAttr::If { .. }))
                    {
                        let expr = *expr;
                        let consumed = self.lower_if_chain(el, expr, nodes, i, &[], &mut out);
                        i += consumed;
                        continue;
                    }
                    if el.tag == "slot" {
                        out.push(self.lower_slot(el, &[]));
                        i += 1;
                        continue;
                    }
                    if is_component_tag(&el.tag) {
                        out.push(self.lower_component_use(el, &[]));
                        i += 1;
                        continue;
                    }
                    let mut path = Vec::new();
                    out.push(self.lower_element(el, &mut path));
                    i += 1;
                }
            }
        }
        if roots != 1 || out.len() != 1 {
            self.diags.push(
                LigDiagnostic::error(
                    "LIG-E001",
                    "templates must have exactly one root element",
                    Span::PLACEHOLDER,
                )
                .with_help("wrap multiple roots in a single element"),
            );
            return NodeIr::Marker;
        }
        out.into_iter().next().expect("one root")
    }

    // -- children -----------------------------------------------------------

    /// Lower a child sequence (elements + text + directive chains).
    fn lower_child_seq(&mut self, nodes: &[TplNode], path: &mut Vec<u32>, out: &mut Vec<NodeIr>) {
        let mut i = 0usize;
        while i < nodes.len() {
            match &nodes[i] {
                TplNode::Text(t) => {
                    self.lower_text(t, path, out);
                    i += 1;
                }
                TplNode::Elem(el) => {
                    // M10: structural directives dispatch FIRST — on ANY tag
                    // (element, component, slot); see lower_root.
                    if let Some(TplAttr::For { item, index, list }) =
                        el.attrs.iter().find(|a| matches!(a, TplAttr::For { .. }))
                    {
                        let (item, index, list) = (item.clone(), index.as_deref(), *list);
                        path.push(out.len() as u32);
                        let node = self.lower_for(el, &item, index, list, path);
                        path.pop();
                        out.push(node);
                        i += 1;
                        continue;
                    }
                    if let Some(TplAttr::If { expr }) =
                        el.attrs.iter().find(|a| matches!(a, TplAttr::If { .. }))
                    {
                        let expr = *expr;
                        path.push(out.len() as u32);
                        let consumed = self.lower_if_chain(el, expr, nodes, i, path, out);
                        path.pop();
                        i += consumed;
                        continue;
                    }
                    if el.tag == "slot" {
                        path.push(out.len() as u32);
                        let node = self.lower_slot(el, path);
                        path.pop();
                        out.push(node);
                        i += 1;
                        continue;
                    }
                    if is_component_tag(&el.tag) {
                        path.push(out.len() as u32);
                        let node = self.lower_component_use(el, path);
                        path.pop();
                        out.push(node);
                        i += 1;
                        continue;
                    }
                    path.push(out.len() as u32);
                    let node = self.lower_element(el, path);
                    path.pop();
                    out.push(node);
                    i += 1;
                }
            }
        }
    }

    /// `v-if` / `v-else-if` / `v-else` chains (C4): the chain collapses to
    /// one marker + one `If` dynamic at the `v-if` element's position.
    /// Returns the number of sibling nodes consumed.
    fn lower_if_chain(
        &mut self,
        first: &TplElement,
        if_expr: (usize, usize),
        nodes: &[TplNode],
        start: usize,
        path: &[u32],
        out: &mut Vec<NodeIr>,
    ) -> usize {
        let Ok(cond) = self.lower_expr_at((if_expr.0 as u32, if_expr.1 as u32), None) else {
            out.push(NodeIr::Marker);
            return 1;
        };
        let then_frag = self.fragment_from_elem(first, "branch");
        let (else_frag, after) = self.lower_else_chain(nodes, start + 1);
        let reads = lig_ir::expr::collect_reads(self.module, cond);
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
            (if_expr.0 as u32, if_expr.1 as u32),
        );
        out.push(NodeIr::Marker);
        after - start
    }

    /// The else side of a chain: the next branch sibling (skipping
    /// whitespace) as a nested holder (`v-else-if`) or a plain fragment
    /// (`v-else`). Returns (fragment, index past the consumed branch).
    fn lower_else_chain(&mut self, nodes: &[TplNode], from: usize) -> (Option<FragmentId>, usize) {
        let mut j = from;
        while matches!(nodes.get(j), Some(n) if self.is_ws_node(n)) {
            j += 1;
        }
        let Some(TplNode::Elem(el)) = nodes.get(j) else {
            return (None, j);
        };
        if let Some(TplAttr::ElseIf { expr }) = el
            .attrs
            .iter()
            .find(|a| matches!(a, TplAttr::ElseIf { .. }))
        {
            let span = (expr.0 as u32, expr.1 as u32);
            let Ok(cond) = self.lower_expr_at(span, None) else {
                return (None, j);
            };
            let then = self.fragment_from_elem(el, "branch");
            let (else_, after) = self.lower_else_chain(nodes, j + 1);
            let holder = self.if_holder_fragment(cond, then, else_, span);
            (Some(holder), after)
        } else if el.attrs.iter().any(|a| matches!(a, TplAttr::Else)) {
            let frag = self.fragment_from_elem(el, "branch");
            (Some(frag), j + 1)
        } else {
            (None, j)
        }
    }

    fn is_ws_text(&self, t: &template::TplText) -> bool {
        t.parts.iter().all(|p| match p {
            TplTextPart::Str((s, e)) => self.source[*s..*e].trim().is_empty(),
            TplTextPart::Hole(_) => false,
        })
    }

    fn is_ws_node(&self, n: &TplNode) -> bool {
        match n {
            TplNode::Text(t) => self.is_ws_text(t),
            TplNode::Elem(_) => false,
        }
    }

    /// `v-for` (§7.5): the element becomes the item fragment; the loop site
    /// collapses to a marker + one `For` dynamic. A `:key` / `v-bind:key`
    /// attribute turns on keyed reconciliation (LIS over the key, §7.5) and
    /// is stripped from the item fragment — the key drives diffing, it is
    /// not an attribute of the item template.
    fn lower_for(
        &mut self,
        el: &TplElement,
        item: &str,
        index: Option<&str>,
        list: (usize, usize),
        path: &[u32],
    ) -> NodeIr {
        let Ok(source) = self.lower_expr_at((list.0 as u32, list.1 as u32), None) else {
            return NodeIr::Marker;
        };
        let item_id = self.intern(item.to_string());
        self.local.insert(item.to_string());
        let index_id = index.map(|i| {
            self.local.insert(i.to_string());
            self.index_accessors.insert(i.to_string());
            self.intern(i.to_string())
        });
        let body_frag = {
            let mut body_el = el.clone();
            body_el
                .attrs
                .retain(|a| !matches!(a, TplAttr::Bind { name, .. } if name == "key"));
            self.fragment_from_elem(&body_el, "items")
        };
        let key = match self.for_key_attr(el) {
            Ok(k) => k,
            Err(d) => {
                self.diags.push(d);
                return NodeIr::Marker;
            }
        };
        let reads = lig_ir::expr::collect_reads(self.module, source);
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
            (list.0 as u32, list.1 as u32),
        );
        NodeIr::Marker
    }

    /// The `v-for` key attribute (§7.5): `:key="expr"` / `v-bind:key="expr"`
    /// — a pure projection of the item (or the index accessor). Keys are
    /// identities: a state-dependent key re-identifies items on unrelated
    /// writes, and a static `key="…"` gives every item the same identity —
    /// both fail closed.
    fn for_key_attr(&mut self, el: &TplElement) -> Result<Option<ExprId>, LigDiagnostic> {
        let el_span = Span::new(el.span.0 as u32, el.span.1 as u32);
        let mut found: Option<ExprId> = None;
        for attr in &el.attrs {
            match attr {
                TplAttr::Static { name, .. } if name == "key" => {
                    return Err(LigDiagnostic::error(
                        "LIG-E001",
                        "a static `key` gives every item the same identity — use `:key`",
                        el_span,
                    ));
                }
                TplAttr::Bind { name, expr } if name == "key" => {
                    if found.is_some() {
                        return Err(LigDiagnostic::error(
                            "LIG-E001",
                            "v-for takes at most one `:key`",
                            el_span,
                        ));
                    }
                    let span = (expr.0 as u32, expr.1 as u32);
                    let e = self.lower_expr_at(span, None)?;
                    if !lig_ir::expr::collect_reads(self.module, e).is_empty() {
                        return Err(LigDiagnostic::error(
                            "LIG-E001",
                            "`:key` must be a pure function of the item — state reads \
                             re-identify items on unrelated writes",
                            Span::new(span.0, span.1),
                        ));
                    }
                    found = Some(e);
                }
                _ => {}
            }
        }
        Ok(found)
    }

    /// Text nodes: static segments trim like the Solid frontend (whitespace-
    /// only segments drop); holes become `Text` dynamics at markers.
    fn lower_text(&mut self, t: &template::TplText, path: &mut Vec<u32>, out: &mut Vec<NodeIr>) {
        for part in &t.parts {
            match part {
                TplTextPart::Str((s, e)) => {
                    let raw = &self.source[*s..*e];
                    if raw.trim().is_empty() {
                        continue;
                    }
                    let id = self.intern(raw.trim());
                    out.push(NodeIr::Text { value: id });
                }
                TplTextPart::Hole((s, e)) => {
                    path.push(out.len() as u32);
                    let span = (*s as u32, *e as u32);
                    match self.lower_expr_at(span, None) {
                        Ok(expr) => {
                            let reads = lig_ir::expr::collect_reads(self.module, expr);
                            self.push_dynamic(
                                DynamicKind::Text,
                                expr,
                                reads,
                                path,
                                vec![],
                                ReactionKind::Bind { dynamic: 0 },
                                span,
                            );
                        }
                        Err(d) => self.diags.push(d),
                    }
                    path.pop();
                    out.push(NodeIr::Marker);
                }
            }
        }
    }

    // -- elements -----------------------------------------------------------

    /// A plain element (no `v-if`/`v-for`): static shape + dynamics.
    fn lower_element(&mut self, el: &TplElement, path: &mut Vec<u32>) -> NodeIr {
        // `<template>` is admitted only as a named slot fill inside a
        // component use — anywhere else it is fail-closed (M6).
        if el.tag == "template" {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                "<template> elements are admitted only as named slot fills (`#name`) inside a component",
                Span::new(el.span.0 as u32, el.span.1 as u32),
            ));
            return NodeIr::Marker;
        }
        let mut static_attrs: Vec<StaticAttr> = Vec::new();
        let mut style: Option<StyleConstId> = None;
        let mut children_out: Vec<NodeIr> = Vec::new();

        for attr in &el.attrs {
            match attr {
                // M12: the template-ref attribute binds the element to a
                // placeholder local — the same `Ref` dynamic the Solid
                // leg's `ref={local}` lowers to. Both spellings (static
                // `ref="x"` and bound `:ref="expr"`) land here; the static
                // attribute must never render.
                TplAttr::Static { name, value, span } if name == "ref" => {
                    let target = value
                        .as_deref()
                        .map_or(RefTarget::Bare, |v| RefTarget::Name(v.to_string()));
                    self.lower_template_ref(target, (span.0 as u32, span.1 as u32), path);
                }
                TplAttr::Bind { name, expr } if name == "ref" => {
                    let espan = (expr.0 as u32, expr.1 as u32);
                    self.lower_template_ref(RefTarget::Expr(*expr), espan, path);
                }
                TplAttr::Static { name, value, .. } => {
                    let key = self.intern(name.clone());
                    let value = match value {
                        Some(v) => AttrValue::Str(self.intern(v.clone())),
                        None => AttrValue::Bool(true),
                    };
                    static_attrs.push(StaticAttr { key, value });
                }
                TplAttr::Bind { name, expr } => {
                    self.lower_bind(name, *expr, &mut static_attrs, &mut style, path);
                }
                TplAttr::On { event, stmt } => {
                    self.lower_event(event, *stmt, path);
                }
                TplAttr::Model { expr } => {
                    self.lower_v_model(el, *expr, path);
                }
                TplAttr::SlotName { .. } => {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        "`#name` slot fills are admitted only on a <template> child of a component",
                        Span::new(el.span.0 as u32, el.span.1 as u32),
                    ));
                }
                TplAttr::Html { expr } => {
                    let span = (expr.0 as u32, expr.1 as u32);
                    match self.lower_expr_at(span, None) {
                        Ok(e) => {
                            let reads = lig_ir::expr::collect_reads(self.module, e);
                            self.push_dynamic(
                                DynamicKind::Html,
                                e,
                                reads,
                                path,
                                vec![],
                                ReactionKind::Bind { dynamic: 0 },
                                span,
                            );
                        }
                        Err(d) => self.diags.push(d),
                    }
                }
                TplAttr::If { .. } | TplAttr::ElseIf { .. } | TplAttr::Else => {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        "v-if/v-else reached element lowering (chain bug)",
                        Span::new(el.span.0 as u32, el.span.1 as u32),
                    ));
                }
                TplAttr::For { .. } => {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        "v-for reached element lowering (chain bug)",
                        Span::new(el.span.0 as u32, el.span.1 as u32),
                    ));
                }
            }
        }

        self.lower_child_seq(&el.children, path, &mut children_out);

        let tag_id = self.intern(el.tag.clone());
        NodeIr::Element {
            tag: tag_id,
            static_attrs,
            style,
            children: children_out,
        }
    }

    /// The `ref` attribute lowers to the capture contract (M12): a plain
    /// `Local` read of a declared placeholder — never a rendered attribute,
    /// never a signal. Both spellings converge here; every other shape is
    /// fail-closed with the precise code (the codegen Ref arm admits only
    /// `Local` targets, so admission owns the diagnosis).
    fn lower_template_ref(&mut self, target: RefTarget, span: (u32, u32), path: &[u32]) {
        let expr: ExprId = match target {
            RefTarget::Bare => {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "the `ref` attribute requires a target — `ref=\"x\"` or `:ref=\"x\"` naming a \
                     placeholder local",
                    Span::new(span.0, span.1),
                ));
                return;
            }
            RefTarget::Name(n) => {
                if self.state.contains(&n) {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        format!(
                            "template ref `{n}` targets a signal — refs capture elements; state \
                             writes go through the generated setters"
                        ),
                        Span::new(span.0, span.1),
                    ));
                    return;
                }
                if !self.local.contains(&n) {
                    self.diags.push(
                        LigDiagnostic::error(
                            "LIG-E100",
                            format!("unknown identifier `{n}` in view position"),
                            Span::new(span.0, span.1),
                        )
                        .with_help(
                            "declare the placeholder local in <script setup> (`let x;`) — the \
                             Solid leg's bare binding is the same declaration",
                        ),
                    );
                    return;
                }
                let name_id = self.intern(n);
                self.module.insert_expr(Expr::Local(name_id))
            }
            RefTarget::Expr(espan) => {
                let espan = (espan.0 as u32, espan.1 as u32);
                let e = match self.lower_expr_at(espan, None) {
                    Ok(e) => e,
                    Err(d) => {
                        self.diags.push(d);
                        return;
                    }
                };
                let Expr::Local(id) = self.module.exprs.get(e).clone() else {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        "template refs bind a placeholder local (`:ref=\"x\"` with `let x;` in \
                         <script setup>) — signals, members, and calls are outside the capture \
                         contract",
                        Span::new(espan.0, espan.1),
                    ));
                    return;
                };
                let n = self.module.strings.get(id).to_string();
                if self.state.contains(&n) {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        format!(
                            "template ref `{n}` targets a signal — refs capture elements; state \
                             writes go through the generated setters"
                        ),
                        Span::new(espan.0, espan.1),
                    ));
                    return;
                }
                e
            }
        };
        let reads = lig_ir::expr::collect_reads(self.module, expr);
        self.push_dynamic(
            DynamicKind::Ref,
            expr,
            reads,
            path,
            vec![],
            ReactionKind::Bind { dynamic: 0 },
            span,
        );
    }

    /// `:k="expr"` — static literals inline into the template (the
    /// dom-expressions `isDynamic` decision); the rest lower per-key.
    fn lower_bind(
        &mut self,
        name: &str,
        expr: (usize, usize),
        static_attrs: &mut Vec<StaticAttr>,
        style: &mut Option<StyleConstId>,
        path: &[u32],
    ) {
        let span = (expr.0 as u32, expr.1 as u32);
        if name == "style" && self.starts_with_brace(expr) {
            match self.lower_style_object(expr, path) {
                Ok(folded) => {
                    *style = folded;
                    return;
                }
                Err(d) => {
                    self.diags.push(d);
                    return;
                }
            }
        }
        let source = self.source;
        let outcome = with_parsed_expr(source, span, |parsed| {
            if !lig_expr::is_dynamic(parsed) {
                // Static-by-syntax: inline into the template (§6.1).
                return Ok(match parsed {
                    Expression::StringLiteral(s) => Some(StaticInline::Str(
                        s.value.as_str().unwrap_or("").to_string(),
                    )),
                    Expression::BooleanLiteral(b) => Some(StaticInline::Bool(b.value)),
                    Expression::NumericLiteral(n) => Some(StaticInline::Num(n.value)),
                    _ => None,
                });
            }
            let e = self.lower_expr(parsed, None)?;
            let kind = match name {
                "class" => DynamicKind::Class,
                "style" => DynamicKind::Style,
                other => DynamicKind::Attr {
                    key: self.intern(other),
                },
            };
            let reads = lig_ir::expr::collect_reads(self.module, e);
            self.push_dynamic(
                kind,
                e,
                reads,
                path,
                vec![],
                ReactionKind::Bind { dynamic: 0 },
                span,
            );
            Ok(None)
        });
        match outcome {
            Ok(Some(StaticInline::Str(v))) => {
                let key = self.intern(name);
                let v = self.intern(v);
                static_attrs.push(StaticAttr {
                    key,
                    value: AttrValue::Str(v),
                });
            }
            Ok(Some(StaticInline::Bool(b))) => {
                let key = self.intern(name);
                static_attrs.push(StaticAttr {
                    key,
                    value: AttrValue::Bool(b),
                });
            }
            Ok(Some(StaticInline::Num(n))) => {
                let key = self.intern(name);
                static_attrs.push(StaticAttr {
                    key,
                    value: AttrValue::Num(n),
                });
            }
            Ok(None) => {}
            Err(d) => self.diags.push(d),
        }
    }

    fn starts_with_brace(&self, expr: (usize, usize)) -> bool {
        self.source[expr.0..expr.1].trim_start().starts_with('{')
    }

    /// `:style="{ … }"` — the §8.2 decision tree, byte-compatible with the
    /// Solid frontend: all-literal objects fold into the element's const
    /// style; mixed objects carry LIG-W102 and fold their static subset.
    fn lower_style_object(
        &mut self,
        expr: (usize, usize),
        path: &[u32],
    ) -> Result<Option<StyleConstId>, LigDiagnostic> {
        let props = parse_style_object(self.source, expr)?;
        // Static pass first (all-literal ⇒ const style, no lint).
        let all_static = props
            .iter()
            .all(|(_, _, v)| matches!(v, StyleValue::Str(_)));
        if all_static {
            let mut css = String::new();
            for (key, _, value) in &props {
                let StyleValue::Str(v) = value else {
                    continue;
                };
                css.push_str(&lig_style::camel_to_kebab(key));
                css.push(':');
                css.push_str(v);
                css.push(';');
            }
            let ir = lig_style::parse_declarations(&css, Span::PLACEHOLDER)?;
            let mut strings = std::mem::take(&mut self.module.strings);
            let style_const = lig_style::to_style_const(&ir, &mut strings);
            self.module.strings = strings;
            return Ok(Some(self.module.intern_style(style_const)));
        }
        // Mixed object: LIG-W102 + static fold + per-property StyleProp.
        self.module.lints.push(lig_ir::module::LintNote {
            code: "LIG-W102".to_string(),
            message:
                "style object with dynamic values re-created per update — prefer static classes"
                    .to_string(),
            start: expr.0 as u32,
            end: expr.1 as u32,
        });
        let mut static_css = String::new();
        for (key, _value_span, value) in &props {
            let kebab = lig_style::camel_to_kebab(key);
            match value {
                StyleValue::Str(v) => {
                    static_css.push_str(&kebab);
                    static_css.push(':');
                    static_css.push_str(v);
                    static_css.push(';');
                }
                StyleValue::Expr(span) => {
                    let Ok(e) = self.lower_expr_at((span.0 as u32, span.1 as u32), None) else {
                        continue;
                    };
                    // v1: CSS values are strings (§8.1 longhand values).
                    if value_kind_is_str(self.module, e) {
                        let prop_id = self.intern(kebab);
                        let reads = lig_ir::expr::collect_reads(self.module, e);
                        self.push_dynamic(
                            DynamicKind::StyleProp { prop: prop_id },
                            e,
                            reads,
                            path,
                            vec![],
                            ReactionKind::Bind { dynamic: 0 },
                            (span.0 as u32, span.1 as u32),
                        );
                    } else {
                        self.diags.push(LigDiagnostic::error(
                            "LIG-E001",
                            "style property values must lower to strings in v1 (format with a template literal)",
                            Span::new(span.0 as u32, span.1 as u32),
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
            return Ok(Some(self.module.intern_style(style_const)));
        }
        Ok(None)
    }

    /// `@ev="stmt"` — inline `state = expr` statements (synthesized model
    /// fns, §6.3) or handler references (the body lowers at the site).
    fn lower_event(&mut self, event: &str, stmt: (usize, usize), path: &[u32]) {
        let Some(event_name) = EventName::from_name(event) else {
            self.diags.push(
                LigDiagnostic::error(
                    "LIG-E001",
                    format!("event `{event}` is not in the delegatable table"),
                    Span::new(stmt.0 as u32, stmt.1 as u32),
                )
                .with_help("see the fixed event-name table (C7)"),
            );
            return;
        };
        let span = (stmt.0 as u32, stmt.1 as u32);
        let source = self.source;
        let lowered: Result<(ExprId, Vec<StrId>), LigDiagnostic> = with_parsed_expr(
            source,
            span,
            |parsed| {
                match parsed {
                Expression::Identifier(id) => {
                    // A handler reference: lower its body here.
                    let name = id.name.as_str().to_string();
                    let Some((param, body_span)) =
                        self.handlers.get(&name).map(|(p, b)| (p.clone(), *b))
                    else {
                        return Err(LigDiagnostic::error(
                            "LIG-E001",
                            format!("unknown handler `{name}`"),
                            to_span(parsed.span()),
                        )
                        .with_help(
                            "define it in <script setup> as a single-expression function",
                        ));
                    };
                    self.lower_expr_at(body_span, param)
                        .map(|expr| (expr, Vec::new()))
                }
                Expression::AssignmentExpression(assign) => self.lower_assignment(assign),
                other => {
                    Err(LigDiagnostic::error(
                        "LIG-E001",
                        "event statements must be `state = expr` or a handler reference",
                        to_span(other.span()),
                    )
                    .with_help(
                        "admitted: `count = count + 1`, `query = $event.currentTarget.value`, `@click=\"add\"`",
                    ))
                }
            }
            },
        );
        let (expr, writes) = match lowered {
            Ok(v) => v,
            Err(d) => {
                self.diags.push(d);
                return;
            }
        };
        self.push_dynamic(
            DynamicKind::Event {
                event: event_name,
                delegate: true, // the IR-level contract (C7); hosts honor it
            },
            expr,
            vec![], // dispatch-driven: never scheduled by reads (§7.1)
            path,
            writes,
            ReactionKind::Effect { tier: Tier::Post },
            span,
        );
    }

    /// `v-model="state"` (§7.6 sugar, M6): the two-way binding on an
    /// `<input>` lowers to exactly the IR the explicit spelling produces —
    /// a `value` Attr dynamic reading the state, plus an `input` Event
    /// dynamic writing the synthesized setter (`$event.currentTarget.value`).
    /// Admitted on `<input>` only; the bound expression must be a state ref.
    fn lower_v_model(&mut self, el: &TplElement, expr: (usize, usize), path: &[u32]) {
        let el_span = Span::new(el.span.0 as u32, el.span.1 as u32);
        if el.tag != "input" {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                format!(
                    "v-model is admitted on <input> only (found `<{}>`) — spell `:value` + `@input` explicitly elsewhere",
                    el.tag
                ),
                el_span,
            ));
            return;
        }
        let span = (expr.0 as u32, expr.1 as u32);
        let source = self.source;
        let lowered: Result<String, LigDiagnostic> = with_parsed_expr(source, span, |parsed| {
            match parsed {
                Expression::Identifier(id) => Ok(id.name.as_str().to_string()),
                _ => Err(LigDiagnostic::error(
                    "LIG-E001",
                    "v-model must bind a state ref (`v-model=\"state\"`)",
                    to_span(parsed.span()),
                )
                .with_help("two-way bindings to projections are not admitted — bind `:value` and write the setter yourself")),
            }
        });
        let name = match lowered {
            Ok(n) => n,
            Err(d) => {
                self.diags.push(d);
                return;
            }
        };
        if !self.state.contains(&name) {
            self.diags.push(
                LigDiagnostic::error(
                    "LIG-E100",
                    format!("unknown identifier `{name}`"),
                    Span::new(expr.0 as u32, expr.1 as u32),
                )
                .with_help("bind a ref declared in <script setup>"),
            );
            return;
        }
        let target_id = self.intern(name.clone());
        // `:value="state"` half.
        let value_key = self.intern("value");
        let state_expr = self.module.insert_expr(Expr::State(target_id));
        self.push_dynamic(
            DynamicKind::Attr { key: value_key },
            state_expr,
            vec![target_id],
            path,
            vec![],
            ReactionKind::Bind { dynamic: 0 },
            span,
        );
        // `@input="state = $event.currentTarget.value"` half — the same
        // synthesized model-fn call the explicit spelling produces.
        let setter = setter_name(&name);
        let sid = self.intern(setter);
        let fid = self
            .module
            .intern_pure_fn(sid, lig_ir::expr::PureFnKind::ModelFn { target: target_id });
        let ev_arg = self
            .module
            .insert_expr(Expr::Event(lig_ir::expr::EvField::CurrentTargetValue));
        let ev_arg_clone = self.module.exprs.get(ev_arg).clone();
        let call = self.module.insert_expr(Expr::Call(fid, vec![ev_arg_clone]));
        self.push_dynamic(
            DynamicKind::Event {
                event: EventName::Input,
                delegate: true, // the IR-level contract (C7)
            },
            call,
            vec![], // dispatch-driven: never scheduled by reads (§7.1)
            path,
            vec![target_id],
            ReactionKind::Effect { tier: Tier::Post },
            span,
        );
    }

    /// An effect body: a plain expression or a `state = expr` assignment
    /// (the Vue spelling of a setter call — `log.value = x` ≡ `setLog(x)`).
    fn lower_effect_body_at(&mut self, span: (u32, u32)) -> Result<ExprId, LigDiagnostic> {
        let source = self.source;
        with_parsed_expr(source, span, |parsed| match parsed {
            Expression::AssignmentExpression(assign) => {
                self.lower_assignment(assign).map(|(e, _)| e)
            }
            other => self.lower_expr(other, None),
        })
    }

    /// `state = expr` / compound-assign → a synthesized model-fn call (§6.3).
    fn lower_assignment(
        &mut self,
        assign: &oxc_ast::ast::AssignmentExpression<'_>,
    ) -> Result<(ExprId, Vec<StrId>), LigDiagnostic> {
        use oxc_ast::ast::AssignmentTarget;
        let name = match &assign.left {
            AssignmentTarget::AssignmentTargetIdentifier(id) => id.name.as_str().to_string(),
            // `log.value = …` — the script-side ref write (§6.3 unwrap).
            other
                if other.as_simple_assignment_target().is_some_and(|t| {
                    matches!(
                        t,
                        oxc_ast::ast::SimpleAssignmentTarget::StaticMemberExpression(m)
                            if m.property.name == "value"
                                && matches!(
                                    &m.object,
                                    Expression::Identifier(o)
                                        if self.unwrapped_refs.contains(o.name.as_str())
                                    )
                    )
                }) =>
            {
                match other.as_simple_assignment_target() {
                    Some(oxc_ast::ast::SimpleAssignmentTarget::StaticMemberExpression(m)) => {
                        match &m.object {
                            Expression::Identifier(o) => o.name.as_str().to_string(),
                            _ => unreachable!("guarded above"),
                        }
                    }
                    _ => unreachable!("guarded above"),
                }
            }
            _ => {
                return Err(LigDiagnostic::error(
                    "LIG-E001",
                    "assignments must target a state ref (`state = expr`, or `state.value = expr` in script)",
                    to_span(assign.span),
                ));
            }
        };
        if !self.state.contains(&name) {
            return Err(LigDiagnostic::error(
                "LIG-E100",
                format!("unknown identifier `{name}`"),
                to_span(assign.span),
            )
            .with_help("assign to a ref declared in <script setup>"));
        }
        let target_id = self.intern(name.clone());
        let rhs = self.lower_expr(&assign.right, Some("$event".to_string()))?;
        let state_expr = self.module.insert_expr(Expr::State(target_id));
        let state_clone = self.module.exprs.get(state_expr).clone();
        let rhs_value = self.module.exprs.get(rhs).clone();
        let value = match assign.operator {
            AssignmentOperator::Assign => rhs,
            AssignmentOperator::Addition => self.module.insert_expr(Expr::Binary(
                lig_ir::expr::BinOp::Add,
                Box::new(state_clone.clone()),
                Box::new(rhs_value.clone()),
            )),
            AssignmentOperator::Subtraction => self.module.insert_expr(Expr::Binary(
                lig_ir::expr::BinOp::Sub,
                Box::new(state_clone.clone()),
                Box::new(rhs_value.clone()),
            )),
            AssignmentOperator::Multiplication => self.module.insert_expr(Expr::Binary(
                lig_ir::expr::BinOp::Mul,
                Box::new(state_clone.clone()),
                Box::new(rhs_value.clone()),
            )),
            AssignmentOperator::Division => self.module.insert_expr(Expr::Binary(
                lig_ir::expr::BinOp::Div,
                Box::new(state_clone),
                Box::new(rhs_value),
            )),
            op => {
                return Err(LigDiagnostic::error(
                    "LIG-E001",
                    format!(
                        "compound assignment `{}` is outside the admitted subset",
                        op.as_str()
                    ),
                    to_span(assign.span),
                )
                .with_help("write it out as `state = state op value`"));
            }
        };
        let setter = setter_name(&name);
        let sid = self.intern(setter);
        let fid = self
            .module
            .intern_pure_fn(sid, lig_ir::expr::PureFnKind::ModelFn { target: target_id });
        let call = self
            .module
            .insert_expr(Expr::Call(fid, vec![self.module.exprs.get(value).clone()]));
        Ok((call, vec![target_id]))
    }

    /// Component use (§7.6): props bind by name; single-element children
    /// become the default-slot fill.
    fn lower_component_use(&mut self, el: &TplElement, path: &[u32]) -> NodeIr {
        let Some(component_idx) = self.component_names.iter().position(|n| *n == el.tag) else {
            self.diags.push(
                LigDiagnostic::error(
                    "LIG-E100",
                    format!("unknown component `{}`", el.tag),
                    Span::new(el.span.0 as u32, el.span.1 as u32),
                )
                .with_help(
                    "define it as a section of this module (single-component SFCs use one file each)",
                ),
            );
            return NodeIr::Marker;
        };
        let mut props: Vec<PropBinding> = Vec::new();
        for attr in &el.attrs {
            match attr {
                TplAttr::Static {
                    name,
                    value: Some(v),
                    ..
                } => {
                    let v = self.intern(v.clone());
                    let value = self.module.insert_expr(Expr::Lit(Lit::Str(v)));
                    props.push(PropBinding {
                        prop: self.intern(name.clone()),
                        value,
                    });
                }
                TplAttr::Static {
                    name, value: None, ..
                } => {
                    let value = self.module.insert_expr(Expr::Lit(Lit::Bool(true)));
                    props.push(PropBinding {
                        prop: self.intern(name.clone()),
                        value,
                    });
                }
                TplAttr::Bind { name, expr } => {
                    let span = (expr.0 as u32, expr.1 as u32);
                    match self.lower_expr_at(span, None) {
                        Ok(value) => props.push(PropBinding {
                            prop: self.intern(name.clone()),
                            value,
                        }),
                        Err(d) => self.diags.push(d),
                    }
                }
                TplAttr::On { event, .. } => {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        format!(
                            "event bindings on components are outside the admitted subset (`@{event}`)"
                        ),
                        Span::new(el.span.0 as u32, el.span.1 as u32),
                    ));
                }
                _ => {}
            }
        }
        let mut reads: Vec<StrId> = props
            .iter()
            .flat_map(|p| lig_ir::expr::collect_reads(self.module, p.value))
            .collect();
        reads.sort();
        reads.dedup();
        let prop_expr = props.first().map_or_else(
            || self.module.insert_expr(Expr::Lit(Lit::Null)),
            |p| p.value,
        );
        // Slot fills (§7.4): `<template #name>` children fill the named
        // slots (multiple admitted, in child order); a single plain element
        // child stays the default-slot fill.
        let mut slots: Vec<SlotIr> = Vec::new();
        let mut plain_element: Option<&TplElement> = None;
        for child in el.children.iter().filter_map(|c| c.as_elem()) {
            if child.tag == "template" {
                let Some(TplAttr::SlotName { name }) = child
                    .attrs
                    .iter()
                    .find(|a| matches!(a, TplAttr::SlotName { .. }))
                else {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        "a <template> slot fill requires a slot name (`#name` or `v-slot:name`)",
                        Span::new(el.span.0 as u32, el.span.1 as u32),
                    ));
                    continue;
                };
                if name == "default" {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        "the default slot fills from plain element children, not `<template #default>`",
                        Span::new(el.span.0 as u32, el.span.1 as u32),
                    ));
                    continue;
                }
                // The `<template>` wrapper is the fill container, not the
                // content: the fragment's root is the wrapper's single static
                // element child — the same shape the Solid leg lowers from
                // its element-valued prop.
                let fill_root: Vec<&TplElement> =
                    child.children.iter().filter_map(|c| c.as_elem()).collect();
                let has_visible_text = child
                    .children
                    .iter()
                    .any(|c| matches!(c, TplNode::Text(t) if !self.is_ws_text(t)));
                if fill_root.len() != 1 || has_visible_text {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        "a named slot fill must wrap exactly one static element (`<template #name><el/></template>`)",
                        Span::new(el.span.0 as u32, el.span.1 as u32),
                    ));
                    continue;
                }
                self.deny_structural_content_root(fill_root[0]);
                let fill = self.fragment_from_content_root(fill_root[0], "slotfill");
                self.ensure_static_slot_content(fill, (el.span.0 as u32, el.span.1 as u32));
                slots.push(SlotIr {
                    name: self.intern(name.clone()),
                    fallback: Some(fill),
                });
                continue;
            }
            match plain_element {
                None => plain_element = Some(child),
                Some(_) => {
                    self.diags.push(LigDiagnostic::error(
                        "LIG-E001",
                        "multi-element slot fills are outside the admitted subset (wrap them in one element)",
                        Span::new(el.span.0 as u32, el.span.1 as u32),
                    ));
                }
            }
        }
        if let Some(child_el) = plain_element {
            self.deny_structural_content_root(child_el);
            let fill = self.fragment_from_content_root(child_el, "slotfill");
            self.ensure_static_slot_content(fill, (el.span.0 as u32, el.span.1 as u32));
            slots.push(SlotIr {
                name: self.intern("default"),
                fallback: Some(fill),
            });
        }
        let component = ComponentIr {
            component: lig_core::id::ComponentId::new(component_idx as u32),
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
            (el.span.0 as u32, el.span.1 as u32),
        );
        NodeIr::Marker
    }

    /// `<slot />` / `<slot name="header">fallback</slot>` (§7.4, M6 — the
    /// outlet gained its name and its fallback content this milestone). A
    /// named outlet does not synthesize the `children` prop; only the
    /// default outlet (`<slot />`) does.
    fn lower_slot(&mut self, el: &TplElement, path: &[u32]) -> NodeIr {
        let slot_name = match el.attrs.iter().find_map(|a| match a {
            TplAttr::Static {
                name,
                value: Some(v),
                ..
            } if name == "name" => Some(v.clone()),
            TplAttr::Bind { name, .. } if name == "name" => {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "slot names are identities — `<slot name=\"…\">` must be a static string",
                    Span::new(el.span.0 as u32, el.span.1 as u32),
                ));
                None
            }
            _ => None,
        }) {
            Some(n) => n,
            None => "default".to_string(),
        };
        // Any other directive on the outlet is fail-closed.
        for attr in &el.attrs {
            if matches!(attr, TplAttr::Static { name, .. } if name == "name") {
                continue;
            }
            if !matches!(attr, TplAttr::Static { .. }) {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "directives on a <slot> outlet are outside the admitted subset",
                    Span::new(el.span.0 as u32, el.span.1 as u32),
                ));
                return NodeIr::Marker;
            }
        }
        // Fallback content: one static element child, or none.
        let fallback = self.slot_fallback_from_children(el);
        if slot_name == "default" {
            self.slot_seen = true;
        }
        let null = self.module.insert_expr(Expr::Lit(Lit::Null));
        let slot_name_id = self.intern(slot_name);
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
            (el.span.0 as u32, el.span.1 as u32),
        );
        NodeIr::Marker
    }

    /// The outlet-side fallback fragment: one element child of the outlet,
    /// static-only (the v1 slot-content contract). Whitespace text is
    /// ignored; visible text or multiple elements are fail-closed.
    fn slot_fallback_from_children(&mut self, el: &TplElement) -> Option<FragmentId> {
        let element_children: Vec<&TplElement> =
            el.children.iter().filter_map(|c| c.as_elem()).collect();
        let has_visible_text = el
            .children
            .iter()
            .any(|c| matches!(c, TplNode::Text(t) if !self.is_ws_text(t)));
        if element_children.is_empty() {
            if has_visible_text {
                self.diags.push(LigDiagnostic::error(
                    "LIG-E001",
                    "slot fallback content must be a single element (wrap the content in one element)",
                    Span::new(el.span.0 as u32, el.span.1 as u32),
                ));
            }
            return None;
        }
        if element_children.len() > 1 {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                "multi-element slot fallbacks are outside the admitted subset (wrap them in one element)",
                Span::new(el.span.0 as u32, el.span.1 as u32),
            ));
            return None;
        }
        self.deny_structural_content_root(element_children[0]);
        let fill = self.fragment_from_content_root(element_children[0], "slotfallback");
        self.ensure_static_slot_content(fill, (el.span.0 as u32, el.span.1 as u32));
        Some(fill)
    }

    /// Chain-tail directives (`v-else-if`/`v-else`) on a slot-content ROOT
    /// stay fail-closed (M8): a root chain tail has no chain to join — the
    /// fill mounts unconditionally (put the whole chain on the content's
    /// children instead). Since M9, component tags and `<slot>` tags on
    /// content roots are ADMITTED: `fragment_from_content_root` dispatches
    /// them onto the content fragment (the M8 denials existed because
    /// `lower_element` does not dispatch the tag — the same fail-open shape
    /// the root-`<slot>` check closed by denying; M9 lowers honestly
    /// instead). Structural ROOTS (`v-if`/`v-for`) are ADMITTED since M8 —
    /// `fragment_from_content_root` lowers them onto the content fragment.
    fn deny_structural_content_root(&mut self, el: &TplElement) {
        if el
            .attrs
            .iter()
            .any(|a| matches!(a, TplAttr::ElseIf { .. } | TplAttr::Else))
        {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                "`v-else-if`/`v-else` on slot-content roots have no chain to join — put the whole chain on the content's children",
                Span::new(el.span.0 as u32, el.span.1 as u32),
            ));
        }
        // M9: a structural directive on a slot/component content root has no
        // element to lower onto (the dispatch lowers the outlet/use itself,
        // not a wrapper) — fail-closed instead of silently dropping the
        // directive; wrap the outlet/component in one element.
        if (el.tag == "slot" || is_component_tag(&el.tag))
            && el
                .attrs
                .iter()
                .any(|a| matches!(a, TplAttr::If { .. } | TplAttr::For { .. }))
        {
            self.diags.push(LigDiagnostic::error(
                "LIG-E001",
                "`v-if`/`v-for` on a slot/component content root has no element to lower onto — wrap the outlet/component in one element",
                Span::new(el.span.0 as u32, el.span.1 as u32),
            ));
        }
    }

    /// The M9 slot-content contract: fills and fallbacks mount as live
    /// fragments — scalar binds (text/class/attr/style-prop/html), events,
    /// refs, If/For dynamics (M8), AND component uses and nested outlets
    /// (M9: grandchild prop-pass rides the author's field + a mount-time
    /// insert; nested outlets mount their fills/own fallbacks through the
    /// composed `Ctx` walks). Component uses and nested outlets admit inside
    /// BRANCH bodies of content since M10 (per-mount instance identity + the
    /// branch-time slot assembly). Still fail-closed inside LIST bodies:
    /// per-item instance identity is the keyed-component story.
    /// Children/prop-pass/whole-style dynamics stay denied (M2), and event
    /// handlers inside list item bodies stay denied (the LIG-E118 discipline
    /// — per-item dispatch needs keyed components).
    fn ensure_static_slot_content(&mut self, frag: FragmentId, span: (u32, u32)) {
        self.content_contract(frag, false, span);
    }

    /// The recursive content walk: `in_list_item` marks fragments mounted as
    /// For bodies (their binds are the per-item replay's; events there are
    /// the LIG-E118 denial; components and outlets there are the per-item
    /// identity denial).
    fn content_contract(&mut self, frag: FragmentId, in_list_item: bool, span: (u32, u32)) {
        let (dynamics, children) = {
            let f = &self.module.fragments[frag.get() as usize];
            (f.dynamics.clone(), f.children.clone())
        };
        for d in &dynamics {
            let denied: Option<String> = match &d.kind {
                // Scalar binds, refs, the structural constructs (M8),
                // component uses + nested outlets on content (M9) and inside
                // content BRANCHES (M10 — per-mount identity) are admitted.
                DynamicKind::Text
                | DynamicKind::Html
                | DynamicKind::Attr { .. }
                | DynamicKind::Class
                | DynamicKind::StyleProp { .. }
                | DynamicKind::Ref
                | DynamicKind::If(_)
                | DynamicKind::For(_) => None,
                DynamicKind::Event { .. } => in_list_item.then(|| {
                    "event handlers inside list item bodies are not admitted in v1 — hoist the \
                     handler to the list level (the LIG-E118 discipline)"
                        .to_string()
                }),
                DynamicKind::Component(_) => in_list_item.then(|| {
                    "component uses inside list item bodies of slot content are not admitted — \
                     per-item instance identity is the keyed-component story (hoist the use out \
                     of the body)"
                        .to_string()
                }),
                DynamicKind::Slot { .. } => in_list_item.then(|| {
                    "slot outlets inside list item bodies of slot content are not admitted — \
                     per-item slot assembly is the keyed-component story (hoist the outlet out \
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
                    Span::new(span.0, span.1),
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

    /// The fill/fallback fragment from its root element (M8): a structural
    /// root (`v-if`/`v-for` on the content root element) lowers ONTO the
    /// content fragment — the If/For dynamic and its branch/list bodies hang
    /// on the content, which is exactly the composed-context shape the
    /// codegen addresses. Plain roots lower through `fragment_from_elem`.
    fn fragment_from_content_root(&mut self, el: &TplElement, kind_name: &str) -> FragmentId {
        // M9: component tags and `<slot>` tags on content roots dispatch
        // onto the content fragment (a root `<slot>` IS a nested outlet, a
        // root component tag IS a grandchild use). The M8 denials existed
        // because `lower_element` does not dispatch the tag — the honest
        // fix is dispatching, not denying. Directives on the root outside
        // the chain check stay fail-closed (`lower_slot` denies them).
        if el.tag == "slot" || is_component_tag(&el.tag) {
            let name = format!("{kind_name}_{}", self.branch_counter);
            self.branch_counter += 1;
            let name_id = self.intern(name);
            let frag = self.module.push_fragment(FragmentIr {
                name: name_id,
                template: TemplateId::new(0),
                dynamics: vec![],
                children: vec![],
                props: vec![],
                returns: vec![],
            });
            self.frames.push(Frame {
                fragment: frag,
                dynamics: vec![],
                anchors: vec![],
                children: vec![],
            });
            let root = if el.tag == "slot" {
                self.lower_slot(el, &[])
            } else {
                self.lower_component_use(el, &[])
            };
            let tpl = self.module.intern_template(TemplateIr::new(root));
            let fi = frag.get() as usize;
            self.module.fragments[fi].template = tpl;
            let frame = self.frames.last().expect("open frame");
            self.module.fragments[fi]
                .dynamics
                .clone_from(&frame.dynamics);
            self.module.fragments[fi]
                .children
                .clone_from(&frame.children);
            assign_anchor_paths(self.module, frag, &frame.anchors);
            self.frames.pop();
            self.frame().children.push(frag);
            return frag;
        }
        let has_if = el.attrs.iter().any(|a| matches!(a, TplAttr::If { .. }));
        let has_for = el.attrs.iter().any(|a| matches!(a, TplAttr::For { .. }));
        if !has_if && !has_for {
            return self.fragment_from_elem(el, kind_name);
        }
        let name = format!("{kind_name}_{}", self.branch_counter);
        self.branch_counter += 1;
        let name_id = self.intern(name);
        let frag = self.module.push_fragment(FragmentIr {
            name: name_id,
            template: TemplateId::new(0),
            dynamics: vec![],
            children: vec![],
            props: vec![],
            returns: vec![],
        });
        self.frames.push(Frame {
            fragment: frag,
            dynamics: vec![],
            anchors: vec![],
            children: vec![],
        });
        let mut path = Vec::new();
        let root = if let Some(TplAttr::For { item, index, list }) =
            el.attrs.iter().find(|a| matches!(a, TplAttr::For { .. }))
        {
            let (item, index, list) = (item.clone(), index.clone(), *list);
            self.lower_for(el, &item, index.as_deref(), list, &path)
        } else if let Some(TplAttr::If { expr }) =
            el.attrs.iter().find(|a| matches!(a, TplAttr::If { .. }))
        {
            // No siblings to join: a content root carries its own chain.
            let expr = *expr;
            let nodes = [TplNode::Elem(el.clone())];
            let mut out = Vec::new();
            let _consumed = self.lower_if_chain(el, expr, &nodes, 0, &path, &mut out);
            out.into_iter().next().unwrap_or(NodeIr::Marker)
        } else {
            self.lower_element(el, &mut path)
        };
        let tpl = self.module.intern_template(TemplateIr::new(root));
        let fi = frag.get() as usize;
        self.module.fragments[fi].template = tpl;
        let frame = self.frames.last().expect("open frame");
        self.module.fragments[fi]
            .dynamics
            .clone_from(&frame.dynamics);
        self.module.fragments[fi]
            .children
            .clone_from(&frame.children);
        assign_anchor_paths(self.module, frag, &frame.anchors);
        self.frames.pop();
        self.frame().children.push(frag);
        frag
    }
}

/// A static-by-syntax attribute value awaiting inlining.
enum StaticInline {
    Str(String),
    Bool(bool),
    Num(f64),
}

/// PascalCase tags are component uses.
fn is_component_tag(tag: &str) -> bool {
    tag.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

/// A copy of the element without its structural directives.
fn strip_directives(el: &TplElement) -> TplElement {
    TplElement {
        tag: el.tag.clone(),
        attrs: el
            .attrs
            .iter()
            .filter(|a| {
                !matches!(
                    a,
                    TplAttr::If { .. }
                        | TplAttr::ElseIf { .. }
                        | TplAttr::Else
                        | TplAttr::For { .. }
                )
            })
            .cloned()
            .collect(),
        children: el.children.clone(),
        span: el.span,
    }
}

/// A style-object property value.
enum StyleValue {
    /// A quoted CSS string.
    Str(String),
    /// A dynamic sub-expression (absolute span into the source).
    Expr((usize, usize)),
}

/// One `:style="{ … }"` object row: (key, value span, value).
type StyleRow = (String, (usize, usize), StyleValue);

/// Split a `:style="{ … }"` object literal into rows without a full JS
/// parse — keys are identifiers or quoted strings, values are quoted
/// strings or balanced sub-expressions.
fn parse_style_object(source: &str, expr: (usize, usize)) -> Result<Vec<StyleRow>, LigDiagnostic> {
    let bad = |msg: &str| {
        LigDiagnostic::error(
            "LIG-E001",
            format!("style object: {msg}"),
            Span::new(expr.0 as u32, expr.1 as u32),
        )
        .with_help("admitted: `{ color: color, \"font-size\": \"14px\" }`")
    };
    let text = &source[expr.0..expr.1];
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if bytes.get(i) != Some(&b'{') {
        return Err(bad("expected an object literal"));
    }
    i += 1;
    let mut rows: Vec<(String, (usize, usize), StyleValue)> = Vec::new();
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if bytes.get(i).is_none_or(|b| *b == b'}') {
            break;
        }
        // Key: identifier or quoted string.
        let key = if bytes.get(i) == Some(&b'\'') || bytes.get(i) == Some(&b'"') {
            let quote = bytes[i];
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            if i >= bytes.len() {
                return Err(bad("unterminated key"));
            }
            let k = text[start..i].to_string();
            i += 1;
            k
        } else {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric()
                    || bytes[i] == b'_'
                    || bytes[i] == b'$'
                    || bytes[i] == b'-')
            {
                i += 1;
            }
            if start == i {
                return Err(bad("expected a property key"));
            }
            text[start..i].to_string()
        };
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if bytes.get(i) != Some(&b':') {
            return Err(bad("expected `:` after a style key"));
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        // Value: quoted string or balanced expression up to `,`/`}`.
        if bytes.get(i) == Some(&b'\'') || bytes.get(i) == Some(&b'"') {
            let quote = bytes[i];
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            if i >= bytes.len() {
                return Err(bad("unterminated value string"));
            }
            let v = text[start..i].to_string();
            i += 1;
            rows.push((key, (expr.0, expr.0), StyleValue::Str(v)));
        } else {
            let start = i;
            let mut depth = 0usize;
            while i < bytes.len() {
                match bytes[i] {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    b'}' => break,
                    b',' if depth == 0 => break,
                    _ => {}
                }
                i += 1;
            }
            if start == i {
                return Err(bad("expected a value"));
            }
            rows.push((
                key,
                (expr.0 + start, expr.0 + i),
                StyleValue::Expr((expr.0 + start, expr.0 + i)),
            ));
        }
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if bytes.get(i) == Some(&b',') {
            i += 1;
        } else if bytes.get(i) != Some(&b'}') && i < bytes.len() {
            return Err(bad("expected `,` or `}`"));
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::lower_module;

    const COUNTER: &str = r#"<script setup lang="ts">
import { ref, computed } from "vue";

const count = ref(0);
const doubled = computed(() => count.value * 2);
</script>

<template>
  <div class="counter p-4" :style="{ display: 'flex', padding: '16px' }">
    <button @click="count = count + 1">Add</button>
    <span>{{ count }}</span>
    <p v-if="doubled > 10" class="hot">Big: {{ doubled }}</p>
  </div>
</template>
"#;

    #[test]
    fn counter_sfc_lowers() {
        let m = lower_module(COUNTER, "counter.vue").expect("lowers");
        assert_eq!(m.frontend, lig_ir::Frontend::Vue);
        assert_eq!(m.components.len(), 1);
        assert_eq!(m.states.len(), 2, "count + doubled");
        assert_eq!(m.reactions.len(), 5, "memo + event + 3 binds");
    }

    #[test]
    fn getter_only_refs_and_for_index_lower() {
        let src = r#"<script setup lang="ts">
const rows = ref(["alpha", "beta"]);
</script>
<template>
  <ul>
    <li v-for="(r, i) in rows">{{ i }} {{ r }}</li>
  </ul>
</template>
"#;
        let m = lower_module(src, "rows.vue").expect("lowers");
        assert_eq!(m.states.len(), 1);
        let names: Vec<String> = m
            .fragments
            .iter()
            .map(|f| m.strings.get(f.name).to_string())
            .collect();
        assert!(names.iter().any(|n| n == "items_0"), "{names:?}");
    }

    #[test]
    fn multi_section_modules_lower_two_components() {
        let src = r#"<script setup>
defineOptions({ name: "Panel" });
const { title } = defineProps<{ title: string }>();
</script>
<template>
  <section class="panel">
    <h1>{{ title }}</h1>
    <slot />
  </section>
</template>
<script setup>
defineOptions({ name: "App" });
</script>
<template>
  <Panel title="Status">
    <p>all green</p>
  </Panel>
</template>
"#;
        let m = lower_module(src, "component-use.vue").expect("lowers");
        assert_eq!(m.components.len(), 2);
        let names: Vec<String> = m
            .components
            .iter()
            .map(|c| m.strings.get(c.name).to_string())
            .collect();
        assert!(names.contains(&"Panel".to_string()) && names.contains(&"App".to_string()));
    }

    #[test]
    fn outside_the_subset_fails_closed() {
        let bad = [
            // v-model is admitted on <input> only
            "<script setup>const a = ref(0);</script>\n<template><p v-model=\"a\"/></template>",
            // v-model must bind a state ref
            "<script setup>const a = ref(0);</script>\n<template><input v-model=\"b\" /></template>",
            // v-model must bind a plain identifier
            "<script setup>const a = ref(0);</script>\n<template><input v-model=\"a + 1\" /></template>",
            // script statements outside the subset
            "<script setup>let x = 1;</script>\n<template><p/></template>",
            // multi-root templates
            "<script setup></script>\n<template><p/><i/></template>",
        ];
        for src in bad {
            assert!(
                lower_module(src, "bad.vue").is_err(),
                "expected rejection: {src}"
            );
        }
    }

    /// `v-model` on an `<input>` lowers to exactly the IR the explicit
    /// `:value` + `@input` spelling produces — one Attr dynamic reading the
    /// state and one input Event dynamic writing the synthesized setter.
    #[test]
    fn v_model_sugar_matches_the_explicit_spelling() {
        fn parity_view(m: &lig_ir::module::ViewModule) -> String {
            fn strip(v: &mut serde_json::Value) {
                match v {
                    serde_json::Value::Object(map) => {
                        map.remove("span");
                        map.remove("spans");
                        map.insert("frontend".into(), "x".into());
                        map.insert("source_files".into(), serde_json::Value::Array(vec![]));
                        for (_, child) in map.iter_mut() {
                            strip(child);
                        }
                    }
                    serde_json::Value::Array(items) => {
                        for item in items {
                            strip(item);
                        }
                    }
                    _ => {}
                }
            }
            let mut val: serde_json::Value =
                serde_json::from_str(&m.canonical_json()).expect("canonical json");
            strip(&mut val);
            val.to_string()
        }
        let sugared = lower_module(
            "<script setup>import { ref } from \"vue\"; const q = ref(\"\");</script>\
             <template><input v-model=\"q\" /></template>",
            "model.vue",
        )
        .expect("lowers");
        let explicit = lower_module(
            "<script setup>import { ref } from \"vue\"; const q = ref(\"\");</script>\
             <template><input :value=\"q\" @input=\"q = $event.currentTarget.value\" /></template>",
            "model.vue",
        )
        .expect("lowers");
        assert_eq!(
            parity_view(&sugared),
            parity_view(&explicit),
            "v-model sugar must equal the explicit spelling"
        );
    }

    /// `:key` on a `v-for` element lowers to the For's key expression — a
    /// `Local` read of the item binding — and is stripped from the item
    /// fragment (no dynamic `key` attribute on the `li`).
    #[test]
    fn vfor_key_lowers_and_is_stripped_from_the_item_fragment() {
        let src = r#"<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Tags" });

const tags = ref(["alpha", "beta"]);
</script>

<template>
  <ul>
    <li v-for="t in tags" :key="t">{{ t }}</li>
  </ul>
</template>
"#;
        let m = lower_module(src, "tags.vue").expect("lowers");
        let (key, item, body) = m
            .fragments
            .iter()
            .flat_map(|f| f.dynamics.iter())
            .find_map(|d| match &d.kind {
                lig_ir::DynamicKind::For(f) => Some((f.key, f.item, f.body)),
                _ => None,
            })
            .expect("one For dynamic");
        let key = key.expect("keyed v-for carries a key");
        assert_eq!(
            m.exprs.get(key),
            &lig_ir::expr::Expr::Local(item),
            "identity key reads the item binding"
        );
        let body_dyns = m.fragments[body.get() as usize]
            .dynamics
            .iter()
            .filter(|d| matches!(d.kind, lig_ir::DynamicKind::Attr { .. }))
            .count();
        assert_eq!(body_dyns, 0, "`:key` must not become an item attribute");
        assert_eq!(
            lig_ir::validate::validate(&m),
            [] as [lig_ir::validate::Violation; 0]
        );
    }

    /// `v-bind:key` is the same construct spelled longhand.
    #[test]
    fn vbind_key_is_the_same_construct() {
        let src = r#"<script setup lang="ts">
import { ref } from "vue";

const tags = ref(["alpha"]);
</script>

<template>
  <ul>
    <li v-for="t in tags" v-bind:key="t">{{ t }}</li>
  </ul>
</template>
"#;
        let m = lower_module(src, "tags.vue").expect("lowers");
        let key = m
            .fragments
            .iter()
            .flat_map(|f| f.dynamics.iter())
            .find_map(|d| match &d.kind {
                lig_ir::DynamicKind::For(f) => f.key,
                _ => None,
            })
            .expect("key expression present");
        assert!(matches!(m.exprs.get(key), lig_ir::expr::Expr::Local(_)));
    }

    /// A key reading component state re-identifies items on unrelated
    /// writes — fail-closed at admission.
    #[test]
    fn vfor_key_reading_state_is_e001() {
        let src = r#"<script setup lang="ts">
import { ref } from "vue";

const tags = ref(["alpha"]);
const salt = ref(1);
</script>

<template>
  <ul>
    <li v-for="t in tags" :key="salt">{{ t }}</li>
  </ul>
</template>
"#;
        let err = lower_module(src, "tags.vue").expect_err("must fail");
        assert!(err.iter().any(|d| d.code == "LIG-E001"), "{err:?}");
    }

    /// A static `key="…"` gives every item the same identity — fail-closed.
    #[test]
    fn vfor_static_key_is_e001() {
        let src = r#"<script setup lang="ts">
import { ref } from "vue";

const tags = ref(["alpha"]);
</script>

<template>
  <ul>
    <li v-for="t in tags" key="t">{{ t }}</li>
  </ul>
</template>
"#;
        let err = lower_module(src, "tags.vue").expect_err("must fail");
        assert!(err.iter().any(|d| d.code == "LIG-E001"), "{err:?}");
    }

    /// The M7 slot-content contract (Vue leg): scalar binds and events
    /// directly on fill/fallback fragments are admitted — the runtime
    /// addresses them through the mounted content (`slot_fill_mut` /
    /// `slot_fallback_mut` run arms + `attach_foreign`).
    #[test]
    fn slot_content_admits_scalar_binds_and_events() {
        let src = r#"<script setup lang="ts">
defineOptions({ name: "Panel" });
const { title } = defineProps<{ title: string }>();
</script>
<template>
  <section>
    <slot name="header">
      <em class="fb">{{ title }}</em>
    </slot>
    <slot name="foot"><i class="fb">{{ title }}</i></slot>
  </section>
</template>
<script setup lang="ts">
defineOptions({ name: "App" });
const label = ref("live");
</script>
<template>
  <Panel title="Status">
    <template #header>
      <button @click="label = 'next'">{{ label }}</button>
    </template>
  </Panel>
</template>
"#;
        let m = lower_module(src, "slots_dynamic.vue").expect("lowers");
        let v = lig_ir::validate::validate(&m);
        assert!(v.is_empty(), "invariants must hold: {v:?}");
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

    /// The M8 slot-content contract (Vue leg): the content may be STRUCTURAL
    /// — `v-if`/`v-for` on content roots lower ONTO the content fragment
    /// (`fragment_from_content_root`), descendants keep the full lowering,
    /// and the branch/list tables address through composed `Ctx` walks.
    #[test]
    fn slot_content_admits_structural_constructs() {
        // v-if on the fill root lowers onto the fill fragment.
        let src = r#"<script setup lang="ts">
defineOptions({ name: "App" });
const on = ref(true);
</script>
<template>
  <Panel title="x">
    <template #header><p v-if="on">hi</p></template>
  </Panel>
</template>
<script setup>
defineOptions({ name: "Panel" });
</script>
<template><section><slot name="header" /><slot name="foot" /></section></template>
"#;
        let m = lower_module(src, "structural_fill.vue").expect("lowers");
        let v = lig_ir::validate::validate(&m);
        assert!(v.is_empty(), "invariants must hold: {v:?}");
        let fill = m
            .fragments
            .iter()
            .find(|f| m.strings.get(f.name).starts_with("slotfill"))
            .expect("fill fragment");
        assert!(
            fill.dynamics
                .iter()
                .any(|d| matches!(d.kind, lig_ir::DynamicKind::If(_))),
            "the v-if lowers onto the fill fragment: {fill:?}"
        );
        // v-for on the fallback root lowers onto the fallback fragment.
        let src = r#"<script setup lang="ts">
defineOptions({ name: "Panel" });
const rows = ref(["a"]);
</script>
<template>
  <section>
    <slot name="foot">
      <i v-for="r in rows">{{ r }}</i>
    </slot>
  </section>
</template>
"#;
        let m = lower_module(src, "structural_fallback.vue").expect("lowers");
        let fb = m
            .fragments
            .iter()
            .find(|f| m.strings.get(f.name).starts_with("slotfallback"))
            .expect("fallback fragment");
        assert!(
            fb.dynamics
                .iter()
                .any(|d| matches!(d.kind, lig_ir::DynamicKind::For(_))),
            "the v-for lowers onto the fallback fragment: {fb:?}"
        );
    }

    /// The M9 slot-content contract (Vue leg): component uses and nested
    /// outlets admit DIRECTLY on fill/fallback content — including content
    /// ROOTS (a root component tag lowers through
    /// `fragment_from_content_root`, a root `<slot>` is a nested outlet on
    /// the content). Grandchild prop-pass and composed fill contexts are
    /// the M9 codegen surface.
    #[test]
    fn slot_content_admits_component_uses_and_nested_outlets() {
        // A component use on the fill ROOT (dispatched, not lowered as an
        // element) and a nested outlet inside another fill.
        let src = r#"<script setup lang="ts">
defineOptions({ name: "App" });
const n = ref(1);
</script>
<template>
  <Panel title="x">
    <template #header><Badge :count="n" /></template>
    <template #foot><div><slot name="inner"><em>none</em></slot></div></template>
    <template #inner><b>inner filled</b></template>
  </Panel>
</template>
<script setup lang="ts">
defineOptions({ name: "Badge" });
const { count } = defineProps<{ count: number }>();
</script>
<template><b class="badge">{{ count }}</b></template>
<script setup>
defineOptions({ name: "Panel" });
</script>
<template><section><slot name="header" /><slot name="foot" /></section></template>
"#;
        let m = lower_module(src, "component_in_fill.vue").expect("lowers");
        let v = lig_ir::validate::validate(&m);
        assert!(v.is_empty(), "invariants must hold: {v:?}");
        let fill_with_component = m.fragments.iter().any(|f| {
            m.strings.get(f.name).starts_with("slotfill")
                && f.dynamics
                    .iter()
                    .any(|d| matches!(d.kind, lig_ir::DynamicKind::Component(_)))
        });
        assert!(
            fill_with_component,
            "the component use lowers onto a fill root: {:?}",
            m.fragments
                .iter()
                .map(|f| (m.strings.get(f.name).to_string(), f.dynamics.len()))
                .collect::<Vec<_>>()
        );
        let nested_outlet = m.fragments.iter().any(|f| {
            m.strings.get(f.name).starts_with("slotfill")
                && f.dynamics.iter().any(|d| {
                    matches!(&d.kind, lig_ir::DynamicKind::Slot { name, .. }
                        if m.strings.get(*name) == "inner")
                })
        });
        assert!(
            nested_outlet,
            "the nested `inner` outlet lowers onto a fill fragment"
        );
    }

    /// The M10 fail-closed boundary (Vue leg): component uses and nested
    /// outlets inside LIST bodies of content stay denied with LIG-E001 —
    /// per-item instance identity remains the keyed-component story. The
    /// BRANCH cases M9 denied admit since M10 (per-mount identity + the
    /// branch-time slot assembly).
    #[test]
    fn slot_content_stays_fail_closed_on_structural_instances() {
        let cases: [(&str, &str); 2] = [
            // Component use inside a v-for body on content
            (
                r#"<script setup lang="ts">
defineOptions({ name: "App" });
const rows = ref(["a"]);
</script>
<template>
  <Panel title="x">
    <template #header><Badge v-for="r in rows" :count="1" /></template>
  </Panel>
</template>
<script setup lang="ts">
defineOptions({ name: "Badge" });
const { count } = defineProps<{ count: number }>();
</script>
<template><b>{{ count }}</b></template>
"#,
                "Component in content list",
            ),
            // Nested outlet inside a v-for body on content
            (
                r#"<script setup lang="ts">
defineOptions({ name: "App" });
const rows = ref(["a"]);
</script>
<template>
  <Panel title="x">
    <template #header><slot v-for="r in rows" name="inner" /></template>
  </Panel>
</template>
"#,
                "Outlet in content list",
            ),
        ];
        for (body, what) in cases {
            let src = format!(
                "{body}\n<script setup>defineOptions({{ name: \"Panel\" }});</script>\n<template><section><slot name=\"foot\" /></section></template>\n"
            );
            let src = src.replace("\\n", "\n");
            match lower_module(&src, "bad.vue") {
                Ok(_) => panic!("{what}: expected LIG-E001, got success"),
                Err(diags) => assert!(
                    diags.iter().any(|d| d.code == "LIG-E001"),
                    "{what}: expected LIG-E001, got {diags:?}"
                ),
            }
        }
    }

    /// The M10 lift (Vue leg): component uses and nested outlets admit
    /// DIRECTLY inside v-if branch bodies of slot content — a remounted
    /// branch rebuilds its per-mount component instance, and the outlet
    /// mounts its fill/fallback with the branch.
    #[test]
    fn content_branches_admit_components_and_outlets() {
        let cases: [(&str, &str); 2] = [
            // Component use inside a v-if branch hanging on content
            (
                r#"<script setup lang="ts">
defineOptions({ name: "App" });
const on = ref(true);
</script>
<template>
  <Panel title="x">
    <template #header><span v-if="on"><Badge :count="1" /></span></template>
  </Panel>
</template>
<script setup lang="ts">
defineOptions({ name: "Badge" });
const { count } = defineProps<{ count: number }>();
</script>
<template><b>{{ count }}</b></template>
"#,
                "Component in content branch",
            ),
            // Nested outlet inside a v-if branch hanging on content
            (
                r#"<script setup lang="ts">
defineOptions({ name: "App" });
const on = ref(true);
</script>
<template>
  <Panel title="x">
    <template #header><span v-if="on"><slot name="inner" /></span></template>
  </Panel>
</template>
"#,
                "Outlet in content branch",
            ),
        ];
        for (body, what) in cases {
            let src = format!(
                "{body}\n<script setup>defineOptions({{ name: \"Panel\" }});</script>\n<template><section><slot name=\"foot\" /></section></template>\n"
            );
            let src = src.replace("\\n", "\n");
            lower_module(&src, "branch.vue")
                .unwrap_or_else(|diags| panic!("{what}: expected admission, got {diags:?}"));
        }
    }

    /// The M10 own-tree contract (Vue leg): component uses and outlets
    /// inside v-for bodies of the component's OWN template deny with one
    /// precise reason (per-item identity is the keyed-component story) —
    /// closing two silent fail-opens where the item template lowered with a
    /// never-mounted outlet hole and a single detached component instance.
    #[test]
    fn own_tree_for_bodies_deny_components_and_outlets() {
        let cases: [(&str, &str); 2] = [
            // Outlet inside a v-for body of the component's own template
            (
                r#"<script setup lang="ts">
defineOptions({ name: "Inner" });
const items = ref(["a"]);
</script>
<template>
  <ul>
    <span v-for="item in items" :key="item"><slot name="a"><em>{{ item }}</em></slot></span>
  </ul>
</template>
"#,
                "Outlet in own-tree list",
            ),
            // Component use inside a v-for body of the component's own template
            (
                r#"<script setup lang="ts">
defineOptions({ name: "App" });
const items = ref(["a"]);
</script>
<template>
  <main><span v-for="item in items" :key="item"><Badge :count="item" /></span></main>
</template>
<script setup lang="ts">
defineOptions({ name: "Badge" });
const { count } = defineProps<{ count: string }>();
</script>
<template><b>{{ count }}</b></template>
"#,
                "Component in own-tree list",
            ),
        ];
        for (src, what) in cases {
            match lower_module(src, "own_list.vue") {
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

    // -- M12: template refs on the Vue leg ----------------------------------

    /// The static spelling `ref="box"` binds the element to a placeholder
    /// local declared in `<script setup>` (`let box;`) — the same `Ref`
    /// dynamic the Solid leg's `ref={box}` lowers to: the target is a plain
    /// `Local` read, no state is created for the name, and the element
    /// carries no static `ref` attribute (the pre-M12 fail-open).
    /// Collect the static attribute keys of a template tree (the fail-open
    /// check walks every element of every template).
    fn attr_keys(node: &lig_ir::NodeIr, out: &mut Vec<lig_ir::StrId>) {
        if let lig_ir::NodeIr::Element {
            static_attrs,
            children,
            ..
        } = node
        {
            out.extend(static_attrs.iter().map(|a| a.key));
            for c in children {
                attr_keys(c, out);
            }
        }
    }

    #[test]
    fn template_ref_static_binds_a_placeholder_local() {
        let src = r#"<script setup lang="ts">
defineOptions({ name: "Marked" });

let box;
</script>

<template>
  <section>
    <input ref="box" />
    <p>static</p>
  </section>
</template>
"#;
        let m = lower_module(src, "template-ref.vue").expect("lowers");
        assert!(
            !m.states.iter().any(|s| m.strings.get(s.name) == "box"),
            "the placeholder must not become a state"
        );
        let refs: Vec<lig_ir::ExprId> = m
            .fragments
            .iter()
            .flat_map(|f| f.dynamics.iter())
            .filter_map(|d| match &d.kind {
                lig_ir::DynamicKind::Ref => Some(d.expr),
                _ => None,
            })
            .collect();
        assert_eq!(refs.len(), 1, "one Ref dynamic: {refs:?}");
        let lig_ir::expr::Expr::Local(name) = m.exprs.get(refs[0]) else {
            panic!(
                "the ref target must be a Local read, got {:?}",
                m.exprs.get(refs[0])
            );
        };
        assert_eq!(m.strings.get(*name), "box");
        // The element must not carry the fail-open static `ref` attribute.
        let mut keys: Vec<lig_ir::StrId> = Vec::new();
        for t in &m.templates {
            attr_keys(&t.root, &mut keys);
        }
        let templates = keys.iter().filter(|k| m.strings.get(**k) == "ref").count();
        assert_eq!(templates, 0, "the static `ref` attribute must not render");
        assert_eq!(
            lig_ir::validate::validate(&m),
            [] as [lig_ir::validate::Violation; 0]
        );
    }

    /// `:ref="box"` and `v-bind:ref="box"` are the same construct spelled
    /// longhand — identical IR to the static `ref="box"` form.
    #[test]
    fn template_ref_bound_spelling_lowers_the_same_dynamic() {
        let base = r#"<script setup lang="ts">
defineOptions({ name: "Marked" });

let box;
</script>

<template>
  <input {}="box" />
</template>
"#;
        let ref_of = |m: &lig_ir::module::ViewModule| {
            m.fragments
                .iter()
                .flat_map(|f| f.dynamics.iter())
                .find(|d| matches!(d.kind, lig_ir::DynamicKind::Ref))
                .expect("one Ref dynamic")
                .expr
        };
        let static_form = lower_module(&base.replace("{}", "ref"), "t.vue").expect("lowers");
        let bound_form = lower_module(&base.replace("{}", ":ref"), "t.vue").expect("lowers");
        let longhand = lower_module(&base.replace("{}", "v-bind:ref"), "t.vue").expect("lowers");
        for (what, m) in [("bound", &bound_form), ("longhand", &longhand)] {
            let lig_ir::expr::Expr::Local(name) = m.exprs.get(ref_of(m)) else {
                panic!("{what}: ref target must be a Local read");
            };
            assert_eq!(m.strings.get(*name), "box");
        }
        let lig_ir::expr::Expr::Local(sname) = static_form.exprs.get(ref_of(&static_form)) else {
            panic!("static: ref target must be a Local read");
        };
        assert_eq!(static_form.strings.get(*sname), "box");
    }

    /// A placeholder local is admitted with no initializer and creates no
    /// state — the Vue spelling of the Solid leg's bare `let box;`.
    #[test]
    fn placeholder_declarator_needs_no_initializer() {
        let src = r#"<script setup lang="ts">
defineOptions({ name: "Solo" });

let box;
</script>

<template>
  <p>ok</p>
</template>
"#;
        let m = lower_module(src, "solo.vue").expect("lowers");
        assert_eq!(m.states.len(), 0, "a placeholder declares no state");
    }

    /// A template ref targeting a signal is a category error (the capture
    /// would shadow the setter write path) — fail-closed with LIG-E001.
    #[test]
    fn template_ref_targeting_a_state_is_e001() {
        let src = r#"<script setup lang="ts">
import { ref } from "vue";

const box = ref(0);
</script>

<template>
  <input ref="box" />
</template>
"#;
        let err = lower_module(src, "bad.vue").expect_err("must fail");
        assert!(
            err.iter().any(|d| d.code == "LIG-E001"),
            "expected LIG-E001, got {err:?}"
        );
    }

    /// An unknown template-ref target is the unknown-identifier denial —
    /// the same code the bound form's expression lowering produces.
    #[test]
    fn template_ref_unknown_target_is_e100() {
        let src = r#"<script setup lang="ts">
defineOptions({ name: "Ghost" });
</script>

<template>
  <input ref="ghost" />
</template>
"#;
        let err = lower_module(src, "ghost.vue").expect_err("must fail");
        assert!(
            err.iter().any(|d| d.code == "LIG-E100"),
            "expected LIG-E100, got {err:?}"
        );
    }

    /// A bound template ref must be a plain identifier — a computed-over or
    /// state-shaped expression is outside the capture contract (codegen
    /// admits only `Local` targets; admission denies with the precise shape
    /// instead of the pre-M12 silent `Attr` mis-lowering).
    #[test]
    fn template_ref_non_local_expr_is_e001() {
        let binary = r#"<script setup lang="ts">
defineOptions({ name: "Marked" });

let box;
</script>

<template>
  <input :ref="box + ''" />
</template>
"#;
        let err = lower_module(binary, "bad.vue").expect_err("must fail");
        assert!(
            err.iter().any(|d| d.code == "LIG-E001"),
            "expected LIG-E001 for the binary target, got {err:?}"
        );
        let state = r#"<script setup lang="ts">
import { ref } from "vue";

const box = ref(0);
</script>

<template>
  <input :ref="box" />
</template>
"#;
        let err = lower_module(state, "bad.vue").expect_err("must fail");
        assert!(
            err.iter().any(|d| d.code == "LIG-E001"),
            "expected LIG-E001 for the state target, got {err:?}"
        );
    }

    /// A bare `ref` attribute carries no target — fail-closed.
    #[test]
    fn template_ref_bare_attr_is_e001() {
        let src = r#"<script setup lang="ts">
defineOptions({ name: "Bare" });

let box;
</script>

<template>
  <input ref />
</template>
"#;
        let err = lower_module(src, "bare.vue").expect_err("must fail");
        assert!(
            err.iter()
                .any(|d| d.code == "LIG-E001" && d.message.contains("target")),
            "expected the bare-ref LIG-E001, got {err:?}"
        );
    }

    /// The end-to-end parity pair: the Vue template-ref spelling lowers to
    /// exactly the IR the Solid leg's `ref={box}` produces (modulo the
    /// parity-ignored fields) — the contract the fixture pins.
    #[test]
    fn template_ref_both_legs_parity() {
        fn parity_view(m: &lig_ir::module::ViewModule) -> String {
            fn strip(v: &mut serde_json::Value) {
                match v {
                    serde_json::Value::Object(map) => {
                        map.remove("span");
                        map.remove("spans");
                        map.insert("frontend".into(), "x".into());
                        map.insert("source_files".into(), serde_json::Value::Array(vec![]));
                        for (_, child) in map.iter_mut() {
                            strip(child);
                        }
                    }
                    serde_json::Value::Array(items) => {
                        for item in items {
                            strip(item);
                        }
                    }
                    _ => {}
                }
            }
            let mut val: serde_json::Value =
                serde_json::from_str(&m.canonical_json()).expect("canonical json");
            strip(&mut val);
            val.to_string()
        }
        let solid = lig_parse_solid::lower_module(
            r"export function Marked() {
  let box;
  return (
    <section>
      <input ref={box} />
      <p>static</p>
    </section>
  );
}
",
            "template-ref.tsx",
        )
        .expect("lowers");
        let vue = lower_module(
            r#"<script setup lang="ts">
defineOptions({ name: "Marked" });

let box;
</script>

<template>
  <section>
    <input ref="box" />
    <p>static</p>
  </section>
</template>
"#,
            "template-ref.vue",
        )
        .expect("lowers");
        assert_eq!(
            parity_view(&solid),
            parity_view(&vue),
            "both legs must agree on the template-ref IR"
        );
    }
}
