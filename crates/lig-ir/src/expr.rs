//! Typed expression forest (§5.5) — lowered, typed, and scope-resolved at IR
//! time; the backend never sees JS/TS.

use crate::ShapeId;
use lig_core::id::{ExprId, PureFnId, StrId};
use serde::{Deserialize, Serialize};

/// The expression forest table (§5.1 `exprs`), with the parallel source-span
/// table (`spans[i]` is the span of `exprs[i]` — the src→IR map's backbone,
/// §9.4).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExprTable {
    exprs: Vec<Expr>,
    spans: Vec<SpanEntryLite>,
}

/// A lightweight span record parallel to the expr table (file-relative bytes;
/// the module has exactly one source file in v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanEntryLite {
    /// Start byte.
    pub start: u32,
    /// End byte.
    pub end: u32,
}

impl ExprTable {
    /// An empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            exprs: Vec::new(),
            spans: Vec::new(),
        }
    }

    /// Insert an expression, returning its id.
    pub fn insert(&mut self, e: Expr) -> ExprId {
        let id = ExprId::new(self.exprs.len().try_into().expect("expr table overflow"));
        self.exprs.push(e);
        self.spans.push(SpanEntryLite { start: 0, end: 0 });
        id
    }

    /// Insert with a source span (the lowering driver's path, §9.4).
    pub fn insert_with_span(&mut self, e: Expr, start: u32, end: u32) -> ExprId {
        let id = ExprId::new(self.exprs.len().try_into().expect("expr table overflow"));
        self.exprs.push(e);
        self.spans.push(SpanEntryLite { start, end });
        id
    }

    /// The recorded span of an expression.
    #[must_use]
    pub fn span_of(&self, id: ExprId) -> SpanEntryLite {
        self.spans
            .get(id.get() as usize)
            .copied()
            .unwrap_or(SpanEntryLite { start: 0, end: 0 })
    }

    /// Resolve an id.
    #[must_use]
    pub fn get(&self, id: ExprId) -> &Expr {
        self.exprs
            .get(id.get() as usize)
            .unwrap_or_else(|| panic!("dangling {id}"))
    }

    /// All expressions in table order.
    #[must_use]
    pub fn as_slice(&self) -> &[Expr] {
        &self.exprs
    }

    /// Number of expressions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.exprs.len()
    }

    /// Whether the table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.exprs.is_empty()
    }
}

/// An expression (§5.5). `State` reads are the explicit dataflow edges (D-3):
/// dependencies are a property of the program, not something a runtime
/// discovers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    /// Literal.
    Lit(Lit),
    /// Non-reactive local binding.
    Local(StrId),
    /// Signal/memo/prop read — a dataflow edge.
    State(StrId),
    /// Event-object field read inside a handler (`e.currentTarget.value`).
    Event(EvField),
    /// Field access.
    Field(Box<Expr>, StrId),
    /// Index access.
    Index(Box<Expr>, Box<Expr>),
    /// Array literal (For sources, list-valued state initializers).
    Array(Vec<Expr>),
    /// Unary op.
    Unary(UnOp, Box<Expr>),
    /// Binary op.
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// Conditional.
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// Template literal parts (text segments and holes).
    Tpl(Vec<TplPart>),
    /// Whitelisted pure builtin or model fn call.
    Call(PureFnId, Vec<Expr>),
    /// Inline Rust escape hatch (`native("…")`, §6.5) — interned source text,
    /// Lint-tagged at admission, opaque to the optimizer.
    Native(StrId),
}

/// Admitted event-object fields (§6.1: handlers compile to model functions
/// with `&mut Ctx, Ev` signatures; only these fields are visible).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvField {
    /// `e.currentTarget.value`
    CurrentTargetValue,
    /// `e.target.value`
    TargetValue,
    /// `e.key`
    Key,
}

impl EvField {
    /// The Rust field on `lig_runtime::EvData`.
    #[must_use]
    pub const fn rust_field(self) -> &'static str {
        match self {
            EvField::CurrentTargetValue => "current_target_value",
            EvField::TargetValue => "target_value",
            EvField::Key => "key",
        }
    }
}

/// A template-literal part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TplPart {
    /// Static text segment.
    Text(StrId),
    /// Interpolated sub-expression.
    Hole(Box<Expr>),
}

/// Literals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Lit {
    /// Integer literal.
    Int(i64),
    /// Floating literal.
    Float(f64),
    /// String literal (interned).
    Str(StrId),
    /// Boolean literal.
    Bool(bool),
    /// `null`.
    Null,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnOp {
    /// `-x`
    Neg,
    /// `!x`
    Not,
}

/// Binary operators (arithmetic, comparison, logic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Mod,
    /// `===`/`==`
    Eq,
    /// `!==`/`!=`
    Neq,
    /// `<`
    Lt,
    /// `<=`
    Lte,
    /// `>`
    Gt,
    /// `>=`
    Gte,
    /// `&&`
    And,
    /// `||`
    Or,
}

/// A whitelisted pure builtin or model function (§5.5 `Call`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PureFnDecl {
    /// Lookup name: `Math.abs` for builtins, `setCount` for model fns.
    pub name: StrId,
    /// What kind of callable.
    pub kind: PureFnKind,
}

/// Callable kinds behind `PureFnId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PureFnKind {
    /// A whitelisted pure builtin (`Math.*`, `String.prototype.*`, …).
    Builtin,
    /// A model function generated from `createSignal` setters (§6.3); the
    /// written state field is explicit so codegen maps `setCount(x)` to the
    /// `set_count` setter without name-mangling conventions.
    ModelFn {
        /// The state field this setter writes.
        target: StrId,
    },
}

/// The small type lattice (§5.5); unknown ⇒ Deny with a "move to model"
/// suggestion (admission expands this in M1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ty {
    /// Integer.
    Int,
    /// Floating point.
    Float,
    /// String.
    Str,
    /// Boolean.
    Bool,
    /// Homogeneous list.
    List(Box<Ty>),
    /// Optional.
    Option(Box<Ty>),
    /// Structural record.
    Struct(ShapeId),
    /// Bottom.
    Never,
}

impl Ty {
    /// Human-readable name for diagnostics and codegen.
    #[must_use]
    pub fn rust_name(&self) -> &str {
        match self {
            Ty::Int => "i64",
            Ty::Float => "f64",
            Ty::Str => "String",
            Ty::Bool => "bool",
            Ty::List(inner) if **inner == Ty::Int => "Vec<i64>",
            Ty::List(_) => "Vec<String>",
            Ty::Option(_) => "Option<i32>",
            Ty::Struct(_) => "Shape",
            Ty::Never => "!",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BinOp, Expr, ExprTable, Lit, Ty};
    use lig_core::id::StrId;

    #[test]
    fn insert_and_get_roundtrip() {
        let mut t = ExprTable::new();
        let id = t.insert(Expr::Lit(Lit::Int(2)));
        let id2 = t.insert(Expr::Binary(
            BinOp::Mul,
            Box::new(Expr::Lit(Lit::Int(2))),
            Box::new(Expr::State(StrId::new(0))),
        ));
        assert_eq!(id.get(), 0);
        assert_eq!(t.get(id), &Expr::Lit(Lit::Int(2)));
        assert!(matches!(t.get(id2), Expr::Binary(BinOp::Mul, _, _)));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn ty_names_cover_the_lattice() {
        assert_eq!(Ty::Int.rust_name(), "i64");
        assert_eq!(Ty::Str.rust_name(), "String");
        assert_eq!(Ty::Bool.rust_name(), "bool");
        assert_eq!(Ty::List(Box::new(Ty::Int)).rust_name(), "Vec<i64>");
        assert_eq!(Ty::Never.rust_name(), "!");
    }

    #[test]
    fn expr_json_is_externally_tagged() {
        let e = Expr::Binary(
            BinOp::Add,
            Box::new(Expr::Lit(Lit::Bool(true))),
            Box::new(Expr::Lit(Lit::Null)),
        );
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.starts_with("{\"Binary\""), "{json}");
    }
}
