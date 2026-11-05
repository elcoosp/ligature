//! oxc arena AST → ExprIR lowering; scope resolution; the pure-builtin
//! whitelist; the `isDynamic` heuristic port (§6.1, §5.5).
//!
//! Spike scope (M0): the expression grammar of §6.5's Allow tier — literals,
//! state/local reads, arithmetic, comparisons, ternaries, template literals,
//! `.length`, and whitelisted pure calls. Model functions (setters) resolve
//! through the module's pure-fn table. The full Svelte-style Scope/Binding
//! substrate lands with M1 admission.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod dyn_check;
pub mod lower;
pub mod pure;
pub mod ty;

pub use dyn_check::is_dynamic;
pub use lower::{Lowering, Scope};
pub use pure::{PURE_BUILTINS, PureBuiltin};
pub use ty::{ty_of, ty_of_expr, value_kind_is_str};
