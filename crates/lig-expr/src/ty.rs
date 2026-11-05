//! Expression typing over the small lattice (§5.5): shared by both
//! frontends — the canonical IR's `states[].ty` must not depend on which
//! surface syntax produced a state.

use lig_core::id::ExprId;
use lig_ir::ViewModule;
use lig_ir::expr::Expr;
use lig_ir::expr::Ty;

/// Infer the small-lattice type of a lowered expression (§5.5; unknown ⇒
/// `Int` — the numeric default for the arithmetic-first subset).
#[must_use]
pub fn ty_of(module: &ViewModule, e: &Expr) -> Ty {
    match e {
        Expr::Lit(lig_ir::expr::Lit::Float(_)) => Ty::Float,
        Expr::Lit(lig_ir::expr::Lit::Bool(_)) | Expr::Unary(lig_ir::expr::UnOp::Not, _) => Ty::Bool,
        // For-item/index locals type by use; strings dominate fixtures (v1).
        Expr::Lit(lig_ir::expr::Lit::Str(_))
        | Expr::Local(_)
        | Expr::Event(_)
        | Expr::Tpl(_)
        | Expr::Native(_) => Ty::Str,
        Expr::State(s) => module.state_by_name(*s).map_or(Ty::Int, |d| d.ty.clone()),
        Expr::Field(obj, f) => {
            if module.strings.get(*f) == "length" {
                Ty::Int
            } else {
                ty_of(module, obj)
            }
        }
        Expr::Array(items) => Ty::List(Box::new(
            items.first().map_or(Ty::Int, |i| ty_of(module, i)),
        )),
        Expr::Ternary(_, a, _) => ty_of(module, a),
        Expr::Binary(op, _, _) if is_bool_binop(*op) => Ty::Bool,
        // Int/Null literals and everything unlisted fall to the numeric
        // default (§5.5: unknown ⇒ Int).
        _ => Ty::Int,
    }
}

/// [`ty_of`] over an expression id.
#[must_use]
pub fn ty_of_expr(module: &ViewModule, id: ExprId) -> Ty {
    ty_of(module, module.exprs.get(id))
}

/// Whether an expression lowers to a string-ish value (for §8.2 style
/// property lowering, where v1 CSS values are strings).
#[must_use]
pub fn value_kind_is_str(module: &ViewModule, id: ExprId) -> bool {
    matches!(
        ty_of(module, module.exprs.get(id)),
        Ty::Str | Ty::List(_) | Ty::Bool
    )
}

fn is_bool_binop(op: lig_ir::expr::BinOp) -> bool {
    use lig_ir::expr::BinOp as B;
    matches!(
        op,
        B::Eq | B::Neq | B::Lt | B::Lte | B::Gt | B::Gte | B::And | B::Or
    )
}

#[cfg(test)]
mod tests {
    use super::{ty_of, value_kind_is_str};
    use lig_core::id::StrId;
    use lig_ir::expr::{Expr, Lit};
    use lig_ir::fragment::{FragmentIr, StateDecl, StateKind};
    use lig_ir::{Frontend, TemplateId, ViewModule};

    #[test]
    fn states_type_from_their_declaration() {
        let mut m = ViewModule::new(Frontend::Solid);
        m.fragments.push(FragmentIr {
            name: StrId::new(0),
            template: TemplateId::new(0),
            dynamics: vec![],
            children: vec![],
            props: vec![],
            returns: vec![],
        });
        let items = m.strings.intern("items");
        m.states.push(StateDecl {
            name: items,
            owner: lig_ir::FragmentId::new(0),
            kind: StateKind::Signal,
            ty: lig_ir::Ty::List(Box::new(lig_ir::Ty::Str)),
            init: None,
        });
        assert!(matches!(
            ty_of(&m, &Expr::State(items)),
            lig_ir::Ty::List(_)
        ));
        let id = m.insert_expr(Expr::State(items));
        assert!(value_kind_is_str(&m, id));
    }

    #[test]
    fn array_literals_type_as_lists() {
        let m = ViewModule::new(Frontend::Vue);
        let e = Expr::Array(vec![Expr::Lit(Lit::Int(1))]);
        assert!(
            matches!(ty_of(&m, &e), lig_ir::Ty::List(inner) if matches!(*inner, lig_ir::Ty::Int))
        );
    }
}
