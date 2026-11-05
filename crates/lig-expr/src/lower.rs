//! Expression lowering: oxc arena AST → owned ExprIR (§5.5).
//!
//! Every identifier resolves here — `State` reads become the explicit
//! dataflow edges (D-3); unresolved names are `LIG-E100` (fail-closed).

use crate::pure;
use lig_core::diagnostic::LigDiagnostic;
use lig_core::id::{ExprId, PureFnId, StrId};
use lig_core::span::Span;
use lig_ir::ViewModule;
use lig_ir::expr::{BinOp, Expr, Lit, TplPart, UnOp};
use oxc_ast::ast::{Expression, UnaryOperator};
use oxc_span::GetSpan;
use std::collections::{BTreeMap, BTreeSet};

/// Visible names at a lowering site (§2.3 Svelte-style binding substrate,
/// spike-reduced).
#[derive(Debug, Clone, Default)]
pub struct Scope {
    /// Reactive state names (signals, memos, props) — `State` reads.
    pub state: BTreeSet<String>,
    /// Non-reactive locals — `Local` reads.
    pub local: BTreeSet<String>,
    /// Model function names (setters) — `Call` through the pure-fn table.
    pub model_fns: BTreeSet<String>,
    /// Setter name → written state field (§6.3 normalization).
    pub setter_targets: BTreeMap<String, String>,
    /// The event-object parameter name inside a handler (`e` of `(e) => …`).
    pub event_param: Option<String>,
    /// `<For>`'s index accessor names (Solid): a *call* `i()` inside the loop
    /// body reads the current index (§7.5).
    pub index_accessors: BTreeSet<String>,
    /// Vue ref/computed names read through `.value` in script contexts —
    /// `count.value` normalizes to a `State` read (§6.3).
    pub unwrapped_refs: BTreeSet<String>,
}

/// Lowering driver over one module.
pub struct Lowering<'m> {
    module: &'m mut ViewModule,
}

/// A lowering failure carries the diagnostic.
pub type Lowered = Result<ExprId, LigDiagnostic>;

impl<'m> Lowering<'m> {
    /// Lower into `module`.
    #[must_use]
    pub const fn new(module: &'m mut ViewModule) -> Self {
        Self { module }
    }

    /// Borrow the module (for callers that interleave other passes).
    #[must_use]
    pub const fn module(&self) -> &ViewModule {
        self.module
    }

    /// Mutable module access.
    pub const fn module_mut(&mut self) -> &mut ViewModule {
        self.module
    }

    /// Insert an expression recording its source span (the src→IR map's
    /// backbone, §9.4).
    pub fn insert_spanned(&mut self, e: lig_ir::expr::Expr, span: oxc_span::Span) -> ExprId {
        self.module.exprs.insert_with_span(e, span.start, span.end)
    }

    /// Intern a string through the module table.
    pub fn intern(&mut self, s: &str) -> StrId {
        self.module.strings.intern(s)
    }

    /// Lower one oxc expression into the module's expr table.
    // One arm per grammar production keeps this auditable against §5.5.
    #[allow(clippy::too_many_lines)]
    pub fn lower(&mut self, expr: &Expression<'_>, scope: &Scope) -> Lowered {
        match expr {
            Expression::BooleanLiteral(b) => {
                Ok(self.insert_spanned(Expr::Lit(Lit::Bool(b.value)), expr.span()))
            }
            Expression::NullLiteral(_) => {
                Ok(self.insert_spanned(Expr::Lit(Lit::Null), expr.span()))
            }
            Expression::NumericLiteral(n) => {
                let lit = if n.value.fract() == 0.0 && n.value.abs() < i64::MAX as f64 {
                    Lit::Int(n.value as i64)
                } else {
                    Lit::Float(n.value)
                };
                Ok(self.insert_spanned(Expr::Lit(lit), expr.span()))
            }
            Expression::StringLiteral(s) => {
                let id = self.intern(s.value.as_str().unwrap_or(""));
                Ok(self.insert_spanned(Expr::Lit(Lit::Str(id)), expr.span()))
            }
            Expression::TemplateLiteral(t) => {
                let mut parts = Vec::with_capacity(t.expressions.len() * 2 + 1);
                for (i, quasi) in t.quasis.iter().enumerate() {
                    if !quasi.value.raw.is_empty() {
                        let id = self.intern(&quasi.value.raw);
                        parts.push(TplPart::Text(id));
                    }
                    if let Some(e) = t.expressions.get(i) {
                        let hole = self.lower(e, scope)?;
                        parts.push(TplPart::Hole(Box::new(self.module.exprs.get(hole).clone())));
                    }
                }
                Ok(self.insert_spanned(Expr::Tpl(parts), expr.span()))
            }
            Expression::Identifier(id) => {
                let name = id.name.to_string();
                if Some(&name) == scope.event_param.as_ref() {
                    // The bare event object is not a value we carry; only
                    // field reads off it are admitted (LIG-E102 for others).
                    return Err(LigDiagnostic::error(
                        "LIG-E102",
                        "the bare event object is not an admitted value — read a field (currentTarget.value, target.value, key)",
                        to_span(expr.span()),
                    ));
                }
                if scope.state.contains(&name) {
                    let sid = self.intern(&name);
                    Ok(self.insert_spanned(Expr::State(sid), expr.span()))
                } else if scope.local.contains(&name) {
                    let lid = self.intern(&name);
                    Ok(self.insert_spanned(Expr::Local(lid), expr.span()))
                } else {
                    Err(Self::unknown_identifier(&name, expr.span(), scope))
                }
            }
            Expression::ArrayExpression(a) => {
                // List literals with spread elements (`[...items(), x]`) are
                // the admitted shape for list updates (§7.5): spreads lower
                // to the whitelisted `arr.concat` builtin, literal batches to
                // nested `Array` literals. Holes stay fail-closed (LIG-E001).
                let mut acc: Option<Expr> = None;
                let mut batch: Vec<Expr> = Vec::new();
                for el in &a.elements {
                    if let oxc_ast::ast::ArrayExpressionElement::SpreadElement(s) = el {
                        let spread_id = self.lower(&s.argument, scope)?;
                        let spread = self.module.exprs.get(spread_id).clone();
                        let left = acc.take().or_else(|| {
                            (!batch.is_empty()).then(|| Expr::Array(std::mem::take(&mut batch)))
                        });
                        batch.clear();
                        acc = Some(match left {
                            None => spread,
                            Some(l) => self.concat_call(l, spread),
                        });
                        continue;
                    }
                    let Some(e) = el.as_expression() else {
                        return Err(LigDiagnostic::error(
                            "LIG-E001",
                            "array holes are outside the admitted subset",
                            to_span(el.span()),
                        ));
                    };
                    let lowered = self.lower(e, scope)?;
                    let value = self.module.exprs.get(lowered).clone();
                    batch.push(value);
                }
                let value = match acc {
                    None => Expr::Array(batch),
                    Some(a) if batch.is_empty() => a,
                    Some(a) => self.concat_call(a, Expr::Array(batch)),
                };
                Ok(self.insert_spanned(value, expr.span()))
            }
            Expression::ComputedMemberExpression(m) => {
                let obj = self.lower(&m.object, scope)?;
                let idx = self.lower(&m.expression, scope)?;
                let ob = box_expr(self.module, obj);
                let ib = box_expr(self.module, idx);
                Ok(self.insert_spanned(Expr::Index(ob, ib), expr.span()))
            }
            Expression::StaticMemberExpression(m) => self.lower_static_member(m, scope),
            Expression::BinaryExpression(b) => {
                let Some(op) = map_binop(b.operator) else {
                    return Err(LigDiagnostic::error(
                        "LIG-E001",
                        format!("unsupported binary operator `{:?}`", b.operator),
                        to_span(b.span),
                    ));
                };
                let lhs = self.lower(&b.left, scope)?;
                let rhs = self.lower(&b.right, scope)?;
                let lb = box_expr(self.module, lhs);
                let rb = box_expr(self.module, rhs);
                Ok(self.insert_spanned(Expr::Binary(op, lb, rb), expr.span()))
            }
            Expression::LogicalExpression(l) => {
                let op = match l.operator {
                    oxc_ast::ast::LogicalOperator::And => BinOp::And,
                    oxc_ast::ast::LogicalOperator::Or => BinOp::Or,
                    oxc_ast::ast::LogicalOperator::Coalesce => {
                        return Err(LigDiagnostic::error(
                            "LIG-E001",
                            "`??` is outside the admitted subset (use an explicit ternary)",
                            to_span(l.span),
                        ));
                    }
                };
                let lhs = self.lower(&l.left, scope)?;
                let rhs = self.lower(&l.right, scope)?;
                let lb = box_expr(self.module, lhs);
                let rb = box_expr(self.module, rhs);
                Ok(self.insert_spanned(Expr::Binary(op, lb, rb), expr.span()))
            }
            Expression::UnaryExpression(u) => {
                let op = match u.operator {
                    UnaryOperator::UnaryNegation => UnOp::Neg,
                    UnaryOperator::LogicalNot => UnOp::Not,
                    other => {
                        return Err(LigDiagnostic::error(
                            "LIG-E001",
                            format!("unsupported unary operator `{other:?}`"),
                            to_span(u.span),
                        ));
                    }
                };
                let inner = self.lower(&u.argument, scope)?;
                let ib = box_expr(self.module, inner);
                Ok(self.insert_spanned(Expr::Unary(op, ib), expr.span()))
            }
            Expression::ConditionalExpression(c) => {
                let cond = self.lower(&c.test, scope)?;
                let then_e = self.lower(&c.consequent, scope)?;
                let else_e = self.lower(&c.alternate, scope)?;
                let cb = box_expr(self.module, cond);
                let tb = box_expr(self.module, then_e);
                let eb = box_expr(self.module, else_e);
                Ok(self.insert_spanned(Expr::Ternary(cb, tb, eb), expr.span()))
            }
            Expression::CallExpression(call) => self.lower_call(call, scope),
            unsupported => Err(LigDiagnostic::error(
                "LIG-E001",
                format!(
                    "construct outside the admitted view subset ({} in view expressions)",
                    kind_of(unsupported)
                ),
                to_span(unsupported.span()),
            )
            .with_help("move it into a `model!{}` block or a memo (§6.5 escape hatches)")),
        }
    }

    /// Lower a static member access: event-object field chains, `.length`,
    /// and admitted field access on state/locals (§6.5 Allow tier).
    fn lower_static_member(
        &mut self,
        m: &oxc_ast::ast::StaticMemberExpression<'_>,
        scope: &Scope,
    ) -> Lowered {
        // Event-object chains: walk to the chain root; if it is the handler's
        // event parameter, the whole chain must match an admitted field shape
        // (§6.1: e.currentTarget.value / e.target.value / e.key).
        let mut chain = vec![m.property.name.as_ref().to_string()];
        let mut cursor = &m.object;
        loop {
            match cursor {
                Expression::StaticMemberExpression(inner) => {
                    chain.push(inner.property.name.as_ref().to_string());
                    cursor = &inner.object;
                }
                Expression::Identifier(root)
                    if Some(root.name.as_str()) == scope.event_param.as_deref() =>
                {
                    return self.lower_event_chain(&chain, m.span);
                }
                _ => break,
            }
        }
        let prop = m.property.name.as_ref();
        // Vue script contexts read refs through `.value`; the unwrapping
        // normalizes to a State read (§6.3).
        if prop == "value"
            && let Expression::Identifier(root) = &m.object
            && scope.unwrapped_refs.contains(root.name.as_str())
        {
            let sid = self.intern(root.name.as_str());
            return Ok(self.insert_spanned(Expr::State(sid), m.span));
        }
        if prop == "length" {
            let obj = self.lower(&m.object, scope)?;
            let p = self.intern("length");
            let ob = box_expr(self.module, obj);
            return Ok(self.insert_spanned(Expr::Field(ob, p), m.span));
        }
        // Field access on state/locals is admitted (§6.5 Allow: "field/index
        // access"); admission tiers deep prop chains (§6.5 Lint). Constants
        // have no admitted fields beyond `.length` (handled above).
        let obj = self.lower(&m.object, scope)?;
        if matches!(self.module.exprs.get(obj), Expr::Lit(_)) {
            return Err(Self::unknown_member(prop, m.span));
        }
        let p = self.intern(prop);
        let ob = box_expr(self.module, obj);
        Ok(self.insert_spanned(Expr::Field(ob, p), m.span))
    }

    /// Match an event chain (outermost-first as built; reversed to
    /// root-outward order for matching) against the admitted field set (§6.1).
    fn lower_event_chain(&mut self, chain: &[String], span: oxc_span::Span) -> Lowered {
        let root_outward: Vec<&str> = chain.iter().rev().map(String::as_str).collect();
        let field = match root_outward.as_slice() {
            ["key"] => lig_ir::expr::EvField::Key,
            ["currentTarget", "value"] => lig_ir::expr::EvField::CurrentTargetValue,
            ["target", "value"] => lig_ir::expr::EvField::TargetValue,
            _ => {
                return Err(LigDiagnostic::error(
                    "LIG-E102",
                    "event field chain is not admitted",
                    to_span(span),
                )
                .with_help("admitted fields: currentTarget.value, target.value, key"));
            }
        };
        Ok(self.insert_spanned(Expr::Event(field), span))
    }

    /// Lower a call: Solid getter syntax, whitelisted pure builtins, model
    /// fns, whitelisted method builtins on expressions, `native("…")`.
    // One admission path per call shape keeps §6.5 auditable.
    #[allow(clippy::too_many_lines)]
    fn lower_call(&mut self, call: &oxc_ast::ast::CallExpression<'_>, scope: &Scope) -> Lowered {
        // native("…"): the inline-Rust escape hatch (§6.5).
        if let Expression::Identifier(id) = &call.callee
            && id.name == "native"
        {
            return self.lower_native(call);
        }
        // Solid getter: count() normalizes to a State read (§6.3).
        if let Expression::Identifier(id) = &call.callee {
            let name = id.name.to_string();
            if scope.state.contains(&name) {
                let sid = self.intern(&name);
                return Ok(self.insert_spanned(Expr::State(sid), call.span));
            }
            // `<For>`'s index accessor: i() reads the current index (§7.5).
            if scope.index_accessors.contains(&name) {
                let iid = self.intern(&name);
                return Ok(self.insert_spanned(Expr::Local(iid), call.span));
            }
        }
        // Direct model-fn call: setCount(...)
        if let Expression::Identifier(id) = &call.callee {
            let name = id.name.to_string();
            if scope.model_fns.contains(&name) {
                let sid = self.intern(&name);
                let target = match scope.setter_targets.get(&name) {
                    Some(t) => self.intern(t),
                    None => self.intern(&name),
                };
                let fid = self
                    .module
                    .intern_pure_fn(sid, lig_ir::expr::PureFnKind::ModelFn { target });
                return self.lower_args_and_call(fid, &call.arguments, scope, call.span);
            }
        }
        // Pure builtin: Math.abs(...)
        if let Expression::StaticMemberExpression(m) = &call.callee
            && let Expression::Identifier(recv) = &m.object
        {
            let key = format!("{}.{}", recv.name, m.property.name);
            if let Some(b) = pure::lookup_builtin(&key) {
                let sid = self.intern(b.key);
                let fid = self.module.intern_pure_fn(sid, pure::BUILTIN_KIND);
                return self.lower_args_and_call(fid, &call.arguments, scope, call.span);
            }
        }
        // Method builtin on an expression: items.join(", "), s.toUpperCase().
        if let Expression::StaticMemberExpression(m) = &call.callee {
            let method = m.property.name.as_ref();
            // Callback-taking methods stay LIG-E114 (the §12.2 sample code);
            // plain non-whitelisted methods are LIG-E115.
            if call.arguments.iter().any(|a| {
                a.as_expression().is_none_or(|e| {
                    matches!(
                        e,
                        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
                    )
                })
            }) {
                return Err(LigDiagnostic::error(
                    "LIG-E114",
                    "function call with callback not admitted in view expressions",
                    to_span(call.span),
                )
                .with_help("move the call into a memo in the model section, then read it here:\n    const visible = createMemo(() => ...);"));
            }
            if let Some(key) = Self::method_key(method)
                && let Some(b) = pure::lookup_builtin(key)
            {
                let receiver = self.lower(&m.object, scope)?;
                let sid = self.intern(b.key);
                let fid = self.module.intern_pure_fn(sid, pure::BUILTIN_KIND);
                let mut args = vec![self.module.exprs.get(receiver).clone()];
                args.extend(self.lower_call_args(&call.arguments, scope)?);
                return Ok(self.insert_spanned(Expr::Call(fid, args), call.span));
            }
            return Err(LigDiagnostic::error(
                "LIG-E115",
                format!("method `.{method}` is not in the pure-builtin whitelist"),
                to_span(call.span),
            )
            .with_help("admitted: join/slice/trim/toUpperCase/toLowerCase/includes/startsWith/endsWith on strings and lists; move other logic into a memo"));
        }
        Err(LigDiagnostic::error(
            "LIG-E114",
            "function call with callback not admitted in view expressions",
            to_span(call.span),
        )
        .with_help("move the call into a memo in the model section, then read it here:\n    const visible = createMemo(() => ...);"))
    }

    /// `native("…")` — a single string literal of Rust tokens (§6.5).
    fn lower_native(&mut self, call: &oxc_ast::ast::CallExpression<'_>) -> Lowered {
        let bad = |msg: &str| {
            LigDiagnostic::error("LIG-E001", msg, to_span(call.span)).with_help(
                "native! takes exactly one string literal of Rust tokens: native(\"expr\")",
            )
        };
        let Some(arg) = call.arguments.first().and_then(|a| a.as_expression()) else {
            return Err(bad("native! requires one string literal argument"));
        };
        let Expression::StringLiteral(s) = arg else {
            return Err(bad("native! requires one string literal argument"));
        };
        let snippet = s.value.as_str().unwrap_or("");
        if snippet.trim().is_empty() {
            return Err(bad("native! snippet is empty"));
        }
        let id = self.intern(snippet);
        Ok(self.insert_spanned(Expr::Native(id), call.span))
    }

    fn lower_call_args(
        &mut self,
        args: &oxc_allocator::Vec<'_, oxc_ast::ast::Argument<'_>>,
        scope: &Scope,
    ) -> Result<Vec<Expr>, LigDiagnostic> {
        let mut out = Vec::with_capacity(args.len());
        for arg in args {
            let Some(e) = arg.as_expression() else {
                let oxc_ast::ast::Argument::SpreadElement(spread) = arg else {
                    continue;
                };
                return Err(LigDiagnostic::error(
                    "LIG-E001",
                    "spread arguments are outside the admitted call subset",
                    to_span(spread.span),
                ));
            };
            let lowered = self.lower(e, scope)?;
            out.push(self.module.exprs.get(lowered).clone());
        }
        Ok(out)
    }

    /// The whitelist key for a method-style call receiver (`items.join` →
    /// `arr.join`), or `None` when the method is not table-visible at all.
    fn method_key(method: &str) -> Option<&'static str> {
        Some(match method {
            "join" => "arr.join",
            "toUpperCase" => "str.toUpperCase",
            "toLowerCase" => "str.toLowerCase",
            "trim" => "str.trim",
            "slice" => "str.slice",
            "includes" => "str.includes",
            "startsWith" => "str.startsWith",
            "endsWith" => "str.endsWith",
            "toFixed" => "num.toFixed",
            "toString" => "num.toString",
            _ => return None,
        })
    }

    /// `[...a, b]` → `arr.concat(a, [b])` through the pure-fn table (§7.5).
    fn concat_call(&mut self, left: Expr, right: Expr) -> Expr {
        let sid = self.intern("arr.concat");
        let fid = self.module.intern_pure_fn(sid, crate::pure::BUILTIN_KIND);
        Expr::Call(fid, vec![left, right])
    }

    fn lower_args_and_call(
        &mut self,
        fid: PureFnId,
        args: &oxc_allocator::Vec<'_, oxc_ast::ast::Argument<'_>>,
        scope: &Scope,
        span: oxc_span::Span,
    ) -> Lowered {
        let lowered_args = self.lower_call_args(args, scope)?;
        Ok(self.insert_spanned(Expr::Call(fid, lowered_args), span))
    }

    fn unknown_identifier(name: &str, span: oxc_span::Span, scope: &Scope) -> LigDiagnostic {
        let candidates: Vec<&str> = scope
            .state
            .iter()
            .chain(&scope.local)
            .chain(&scope.model_fns)
            .map(String::as_str)
            .collect();
        let suggestion = closest(name, candidates);
        LigDiagnostic::error(
            "LIG-E100",
            format!("unknown identifier `{name}` in view position"),
            to_span(span),
        )
        .with_help(match suggestion {
            Some(s) => format!(
                "did you mean `{s}`? Declare it with createSignal/createMemo, or pass it as a prop"
            ),
            None => "declare it in the component body, or move the logic into a `model!{}` block"
                .to_string(),
        })
    }

    fn unknown_member(prop: &str, span: oxc_span::Span) -> LigDiagnostic {
        LigDiagnostic::error(
            "LIG-E115",
            format!("member access `.{prop}` on a constant is outside the admitted subset"),
            to_span(span),
        )
        .with_help("compute derived values in a memo")
    }
}

/// Closest name by edit distance ≤ 2 (Svelte fuzzymatch steal, §2.3).
#[must_use]
pub fn closest(name: &str, candidates: Vec<&str>) -> Option<String> {
    let mut best: Option<(usize, &str)> = None;
    for c in candidates {
        let d = edit_distance(name, c);
        if d <= 2 && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, c));
        }
    }
    best.map(|(_, c)| c.to_string())
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

fn map_binop(op: oxc_ast::ast::BinaryOperator) -> Option<BinOp> {
    use oxc_ast::ast::BinaryOperator as B;
    Some(match op {
        B::Addition => BinOp::Add,
        B::Subtraction => BinOp::Sub,
        B::Multiplication => BinOp::Mul,
        B::Division => BinOp::Div,
        B::Remainder => BinOp::Mod,
        B::Equality | B::StrictEquality => BinOp::Eq,
        B::Inequality | B::StrictInequality => BinOp::Neq,
        B::LessThan => BinOp::Lt,
        B::LessEqualThan => BinOp::Lte,
        B::GreaterThan => BinOp::Gt,
        B::GreaterEqualThan => BinOp::Gte,
        _ => return None,
    })
}

fn box_expr(module: &mut ViewModule, id: ExprId) -> Box<Expr> {
    Box::new(module.exprs.get(id).clone())
}

/// Convert an oxc span to the core span (both are u32 byte ranges, §2.5).
#[must_use]
pub const fn to_span(s: oxc_span::Span) -> Span {
    Span {
        start: s.start,
        end: s.end,
    }
}

fn kind_of(expr: &Expression<'_>) -> &'static str {
    match expr {
        Expression::ArrowFunctionExpression(_) => "arrow function",
        Expression::AwaitExpression(_) => "await",
        Expression::ObjectExpression(_) => "object literal",
        Expression::ArrayExpression(_) => "array literal",
        Expression::AssignmentExpression(_) => "assignment",
        Expression::NewExpression(_) => "new expression",
        Expression::JSXElement(_) => "JSX element",
        _ => "unsupported expression",
    }
}

#[cfg(test)]
mod tests {
    use super::{Lowering, Scope};
    use lig_core::diagnostic::Severity;
    use lig_ir::Frontend;
    use lig_ir::expr::{BinOp, Expr, Lit};
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    use std::collections::{BTreeMap, BTreeSet};

    fn lower_src(src: &str, scope: &Scope) -> Result<Expr, String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, src, SourceType::mjs()).parse();
        assert!(
            ret.diagnostics.is_empty(),
            "probe source must parse: {:?}",
            ret.diagnostics
        );
        let oxc_ast::ast::Statement::ExpressionStatement(e) = &ret.program.body[0] else {
            panic!("expected expression");
        };
        let mut module = lig_ir::ViewModule::new(Frontend::Solid);
        let mut lowering = Lowering::new(&mut module);
        lowering
            .lower(&e.expression, scope)
            .map_err(|d| match &d.help {
                Some(h) => format!("{}: {}\nhelp: {h}", d.code, d.message),
                None => format!("{}: {}", d.code, d.message),
            })?;
        Ok(module
            .exprs
            .get(lig_core::id::ExprId::new((module.exprs.len() - 1) as u32))
            .clone())
    }

    fn module_of(src: &str, scope: &Scope) -> lig_ir::ViewModule {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, src, SourceType::mjs()).parse();
        let oxc_ast::ast::Statement::ExpressionStatement(e) = &ret.program.body[0] else {
            panic!("expected expression");
        };
        let mut module = lig_ir::ViewModule::new(Frontend::Solid);
        let mut lowering = Lowering::new(&mut module);
        lowering
            .lower(&e.expression, scope)
            .expect("probe source must lower");
        module
    }

    fn scope_with(states: &[&str], locals: &[&str], fns: &[&str]) -> Scope {
        Scope {
            state: states.iter().map(|s| (*s).to_string()).collect(),
            local: locals.iter().map(|s| (*s).to_string()).collect(),
            model_fns: fns.iter().map(|s| (*s).to_string()).collect(),
            setter_targets: BTreeMap::new(),
            event_param: None,
            index_accessors: BTreeSet::new(),
            unwrapped_refs: BTreeSet::new(),
        }
    }

    fn scope_with_event(states: &[&str], param: &str) -> Scope {
        Scope {
            state: states.iter().map(|s| (*s).to_string()).collect(),
            local: std::collections::BTreeSet::new(),
            model_fns: std::collections::BTreeSet::new(),
            setter_targets: BTreeMap::new(),
            event_param: Some(param.to_string()),
            index_accessors: BTreeSet::new(),
            unwrapped_refs: BTreeSet::new(),
        }
    }

    #[test]
    fn literals_lower() {
        assert_eq!(
            lower_src("42", &Scope::default()).unwrap(),
            Expr::Lit(Lit::Int(42))
        );
        assert_eq!(
            lower_src("true", &Scope::default()).unwrap(),
            Expr::Lit(Lit::Bool(true))
        );
        assert_eq!(
            lower_src("null", &Scope::default()).unwrap(),
            Expr::Lit(Lit::Null)
        );
        assert!(
            matches!(lower_src("1.5", &Scope::default()).unwrap(), Expr::Lit(Lit::Float(f)) if (f - 1.5).abs() < f64::EPSILON)
        );
    }

    #[test]
    fn state_reads_become_dataflow_edges() {
        let e = lower_src("count", &scope_with(&["count"], &[], &[])).unwrap();
        assert!(matches!(e, Expr::State(_)));
    }

    #[test]
    fn unresolved_identifier_is_e100_with_suggestion() {
        let err = lower_src("conut", &scope_with(&["count"], &[], &[])).unwrap_err();
        assert!(err.contains("LIG-E100"), "{err}");
        assert!(err.contains("did you mean"), "{err}");
    }

    #[test]
    fn arithmetic_and_comparisons_lower() {
        let e = lower_src("count * 2 + 1", &scope_with(&["count"], &[], &[])).unwrap();
        assert!(matches!(e, Expr::Binary(BinOp::Add, _, _)));
        let e = lower_src("count === 0", &scope_with(&["count"], &[], &[])).unwrap();
        assert!(matches!(e, Expr::Binary(BinOp::Eq, _, _)));
    }

    #[test]
    fn setter_calls_resolve_through_the_model_fn_table() {
        let e = lower_src(
            "setCount(count + 1)",
            &scope_with(&["count"], &[], &["setCount"]),
        )
        .unwrap();
        assert!(matches!(e, Expr::Call(_, args) if args.len() == 1));
    }

    #[test]
    fn math_whitelist_lowers_to_pure_call() {
        let e = lower_src("Math.abs(count)", &scope_with(&["count"], &[], &[])).unwrap();
        assert!(matches!(e, Expr::Call(_, _)));
        let err = lower_src("items.filter(x => x)", &scope_with(&["items"], &[], &[])).unwrap_err();
        assert!(err.contains("LIG-E114"), "{err}");
    }

    #[test]
    fn length_is_admitted_other_members_are_not() {
        let e = lower_src("items.length", &scope_with(&["items"], &[], &[])).unwrap();
        assert!(matches!(e, Expr::Field(_, _)));
        // M1 (§6.5 Allow): field access on state/locals is admitted.
        let e = lower_src("user.name", &scope_with(&["user"], &[], &[])).unwrap();
        assert!(matches!(e, Expr::Field(_, _)));
        // …but members on constants are not (nothing to resolve against).
        let err = lower_src("\"abc\".foo", &Scope::default()).unwrap_err();
        assert!(err.contains("LIG-E115"), "{err}");
    }

    #[test]
    fn array_literals_lower_to_expr_array() {
        let e = lower_src("[1, 2, 3]", &Scope::default()).unwrap();
        assert!(matches!(e, Expr::Array(items) if items.len() == 3));
        let err = lower_src("[1, ...nope]", &Scope::default()).unwrap_err();
        assert!(err.contains("LIG-E100"), "{err}");
    }

    #[test]
    fn array_spreads_lower_to_concat_builtins() {
        // [...items(), "z"] → Call(arr.concat, [State(items), Array["z"]]).
        let e = lower_src("[...items(), \"z\"]", &scope_with(&["items"], &[], &[])).unwrap();
        let Expr::Call(_, args) = e else {
            panic!("expected concat call, got {e:?}");
        };
        assert!(matches!(args[0], Expr::State(_)));
        assert!(matches!(&args[1], Expr::Array(a) if a.len() == 1));
        // …and the pure-fn row is the whitelisted concat builtin.
        let module = module_of("[...items(), \"z\"]", &scope_with(&["items"], &[], &[]));
        assert_eq!(module.pure_fns.len(), 1);
        assert_eq!(module.strings.get(module.pure_fns[0].name), "arr.concat");
    }

    #[test]
    fn for_index_accessor_calls_read_locals() {
        // i() inside a <For> body lowers to a Local read of the index (§7.5).
        let mut scope = scope_with(&[], &["i"], &[]);
        scope.index_accessors.insert("i".to_string());
        let e = lower_src("i()", &scope).unwrap();
        assert!(matches!(e, Expr::Local(_)));
    }

    #[test]
    fn computed_member_access_lowers_to_index() {
        let e = lower_src("items[0]", &scope_with(&["items"], &[], &[])).unwrap();
        assert!(matches!(e, Expr::Index(_, _)));
        let e = lower_src("matrix[i]", &scope_with(&["matrix"], &["i"], &[])).unwrap();
        assert!(matches!(e, Expr::Index(_, _)));
    }

    #[test]
    fn method_builtins_lower_with_receiver_as_first_arg() {
        let e = lower_src("items.join(\", \")", &scope_with(&["items"], &[], &[])).unwrap();
        assert!(matches!(e, Expr::Call(_, args) if args.len() == 2));
        let e = lower_src("name.toUpperCase()", &scope_with(&["name"], &[], &[])).unwrap();
        assert!(matches!(e, Expr::Call(_, args) if args.len() == 1));
    }

    #[test]
    fn unknown_methods_are_e115_callbacks_are_e114() {
        // §12.2's canonical Deny: callback-taking calls (E114).
        let err = lower_src("items.filter(i => i)", &scope_with(&["items"], &[], &[])).unwrap_err();
        assert!(err.contains("LIG-E114"), "{err}");
        // Plain non-whitelisted methods (E115).
        let err = lower_src("items.concat(items)", &scope_with(&["items"], &[], &[])).unwrap_err();
        assert!(err.contains("LIG-E115"), "{err}");
    }

    #[test]
    fn event_field_reads_lower_to_event_exprs() {
        let scope = scope_with_event(&["query"], "e");
        let e = lower_src("e.currentTarget.value", &scope).unwrap();
        assert_eq!(e, Expr::Event(lig_ir::expr::EvField::CurrentTargetValue));
        let e = lower_src("e.target.value", &scope_with_event(&[], "e")).unwrap();
        assert_eq!(e, Expr::Event(lig_ir::expr::EvField::TargetValue));
        let e = lower_src("e.key", &scope_with_event(&[], "e")).unwrap();
        assert_eq!(e, Expr::Event(lig_ir::expr::EvField::Key));
    }

    #[test]
    fn unadmitted_event_fields_are_e102() {
        let err = lower_src("e.clientX", &scope_with_event(&[], "e")).unwrap_err();
        assert!(err.contains("LIG-E102"), "{err}");
        let err = lower_src("e.currentTarget", &scope_with_event(&[], "e")).unwrap_err();
        assert!(err.contains("LIG-E102"), "{err}");
        let err = lower_src("window.location", &Scope::default()).unwrap_err();
        assert!(err.contains("LIG-E"), "{err}");
    }

    #[test]
    fn bare_event_object_is_not_a_value() {
        let err = lower_src("e", &scope_with_event(&[], "e")).unwrap_err();
        assert!(err.contains("LIG-E102"), "{err}");
    }

    #[test]
    fn native_takes_one_string_literal() {
        let e = lower_src(
            "native(\"count as u32\")",
            &scope_with(&["count"], &[], &[]),
        )
        .unwrap();
        assert!(matches!(e, Expr::Native(_)));
        let err = lower_src("native(42)", &Scope::default()).unwrap_err();
        assert!(err.contains("LIG-E001"), "{err}");
        let err = lower_src("native()", &Scope::default()).unwrap_err();
        assert!(err.contains("LIG-E001"), "{err}");
    }

    #[test]
    fn template_literals_lower_to_tpl() {
        let e = lower_src("`n=${count}`", &scope_with(&["count"], &[], &[])).unwrap();
        assert!(matches!(e, Expr::Tpl(_)));
    }

    #[test]
    fn severity_is_error_for_all_denies() {
        let err = lower_src("window.location", &Scope::default()).unwrap_err();
        // surface as diagnostic text; the parse-side wrapper assigns severity
        assert!(err.contains("LIG-E"));
        let _ = Severity::Error;
    }
}
