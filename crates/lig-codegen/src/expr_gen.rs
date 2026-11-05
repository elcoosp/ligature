//! Expression → Rust token generation (§5.5: the backend never sees JS/TS —
//! it sees the typed Expr forest).

use lig_core::id::StrId;
use lig_ir::ViewModule;
use lig_ir::expr::{BinOp, Expr, Lit, TplPart, UnOp};
use quote::quote;

/// Value kind of an expression, for apply-fn signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// Integer.
    Int,
    /// String.
    Str,
    /// Boolean.
    Bool,
}

/// Infer an expression's value kind from state types and literals.
#[must_use]
/// Whether an operator yields a boolean.
fn is_bool_op(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Eq
            | BinOp::Neq
            | BinOp::Lt
            | BinOp::Lte
            | BinOp::Gt
            | BinOp::Gte
            | BinOp::And
            | BinOp::Or
    )
}

/// Infer an expression's value kind from state types and literals.
#[must_use]
pub fn value_kind(module: &ViewModule, expr: &Expr) -> ValueKind {
    match expr {
        Expr::Lit(Lit::Str(_)) | Expr::Event(_) | Expr::Native(_) | Expr::Array(_) => {
            ValueKind::Str
        }
        Expr::Lit(Lit::Bool(_)) => ValueKind::Bool,
        Expr::State(name) => module
            .state_by_name(*name)
            .map_or(ValueKind::Int, |s| match s.ty {
                lig_ir::expr::Ty::Str => ValueKind::Str,
                lig_ir::expr::Ty::Bool => ValueKind::Bool,
                _ => ValueKind::Int,
            }),
        Expr::Field(_, f) if module.strings.get(*f) == "length" => ValueKind::Int,
        Expr::Ternary(_, a, _) => value_kind(module, a),
        Expr::Binary(op, ..) if is_bool_op(*op) => ValueKind::Bool,
        _ => ValueKind::Int,
    }
}

/// Generate Rust tokens for an expression.
///
/// # Errors
/// [`CodegenError::Unsupported`] when a `native("…")` snippet does not parse
/// as Rust tokens (fail-closed — BR-1 applies to emission).
///
/// # Panics
/// Only on internal invariant breaks (dangling ids) — the IR is validated
/// upstream.
pub fn gen_expr(
    module: &ViewModule,
    id: lig_core::id::ExprId,
) -> Result<proc_macro2::TokenStream, crate::CodegenError> {
    let expr = module.exprs.get(id);
    gen_expr_value(module, expr)
}

fn gen_expr_value(
    module: &ViewModule,
    expr: &Expr,
) -> Result<proc_macro2::TokenStream, crate::CodegenError> {
    Ok(match expr {
        Expr::Lit(lit) => gen_lit(module, lit),
        Expr::Local(name) => {
            let n = ident_of(module, *name);
            quote!(#n)
        }
        Expr::State(name) => {
            let n = ident_of(module, *name);
            quote!(self.#n)
        }
        Expr::Event(field) => {
            // Event payloads live on the component's `ev` slot (§6.1); reads
            // produce an owned String (deref-coerces at `&str` use sites).
            let f = syn::Ident::new(field.rust_field(), proc_macro2::Span::call_site());
            quote!(self.ev.#f.clone().unwrap_or_default())
        }
        Expr::Native(snippet) => {
            let src = module.strings.get(*snippet);
            let tokens: proc_macro2::TokenStream = syn::parse_str(src).map_err(|_| {
                crate::CodegenError::Unsupported("native(…) snippet does not parse as Rust tokens")
            })?;
            quote!(#tokens)
        }
        Expr::Array(items) => {
            // String literals widen to String so `vec![…]` matches the
            // declared `Vec<String>` (list initializers + For sources).
            let elems = items
                .iter()
                .map(|i| match i {
                    Expr::Lit(Lit::Str(_)) => {
                        let v = gen_expr_value(module, i)?;
                        Ok(quote!(::alloc::string::String::from(#v)))
                    }
                    other => gen_expr_value(module, other),
                })
                .collect::<Result<Vec<_>, _>>()?;
            quote!(::alloc::vec![#(#elems),*])
        }
        Expr::Field(inner, f) => {
            let obj = gen_expr_value(module, inner)?;
            if module.strings.get(*f) == "length" {
                quote!((#obj .len() as i64))
            } else {
                let field = ident_of(module, *f);
                quote!(#obj . #field)
            }
        }
        Expr::Index(a, b) => {
            let obj = gen_expr_value(module, a)?;
            let idx = gen_expr_value(module, b)?;
            quote!((#obj [#idx as usize]))
        }
        Expr::Unary(op, inner) => {
            let v = gen_expr_value(module, inner)?;
            match op {
                UnOp::Neg => quote!((-#v)),
                UnOp::Not => quote!((!#v)),
            }
        }
        Expr::Binary(op, a, b) => {
            let l = gen_expr_value(module, a)?;
            let r = gen_expr_value(module, b)?;
            let op_tok = binop(*op);
            quote!((#l #op_tok #r))
        }
        Expr::Ternary(c, a, b) => {
            let cv = gen_expr_value(module, c)?;
            let av = gen_expr_value(module, a)?;
            let bv = gen_expr_value(module, b)?;
            quote!(if #cv { #av } else { #bv })
        }
        Expr::Tpl(parts) => {
            let mut fmt = String::new();
            let mut args = Vec::new();
            for p in parts {
                match p {
                    TplPart::Text(s) => fmt.push_str(module.strings.get(*s)),
                    TplPart::Hole(h) => {
                        fmt.push_str("{}");
                        args.push(gen_expr_value(module, h)?);
                    }
                }
            }
            let lit = syn::LitStr::new(&fmt, proc_macro2::Span::call_site());
            quote!(::alloc::format!(#lit, #(#args),*))
        }
        Expr::Call(fid, args) => {
            let decl = module
                .pure_fns
                .get(fid.get() as usize)
                .expect("validated pure fn");
            let name = module.strings.get(decl.name).to_string();
            let call_args = args
                .iter()
                .map(|a| gen_expr_value(module, a))
                .collect::<Result<Vec<_>, _>>()?;
            match decl.kind {
                lig_ir::expr::PureFnKind::ModelFn { target } => {
                    // setCount(x) → self.set_count(x) (target explicit, §6.3);
                    // string literals coerce to the setter's String parameter.
                    let setter = syn::Ident::new(
                        &format!("set_{}", raw_ident(module.strings.get(target))),
                        proc_macro2::Span::call_site(),
                    );
                    let coerced: Vec<_> = args
                        .iter()
                        .zip(call_args)
                        .map(|(a, tok)| match a {
                            Expr::Lit(Lit::Str(_)) => {
                                quote!(::alloc::string::String::from(#tok))
                            }
                            _ => tok,
                        })
                        .collect();
                    quote!(self.#setter(#(#coerced),*))
                }
                lig_ir::expr::PureFnKind::Builtin => {
                    // Math.abs(x) → (x).abs() etc. (whitelist table, §6.1)
                    gen_builtin(&name, &call_args)
                }
            }
        }
    })
}

fn gen_builtin(name: &str, argv: &[proc_macro2::TokenStream]) -> proc_macro2::TokenStream {
    let first = argv.first().cloned().unwrap_or_default();
    match name {
        "Math.abs" => quote!((#first .abs())),
        "Math.min" => {
            let second = argv.get(1).cloned().unwrap_or_default();
            quote!((#first .min(#second)))
        }
        "Math.max" => {
            let second = argv.get(1).cloned().unwrap_or_default();
            quote!((#first .max(#second)))
        }
        "Math.floor" => quote!((#first .floor())),
        "Math.ceil" => quote!((#first .ceil())),
        "Math.round" => quote!((#first .round())),
        "Math.sqrt" => quote!((#first .sqrt())),
        "num.toString" => {
            quote!({
                let mut buf = [0u8; 20];
                ::alloc::string::String::from(::lig_runtime::format_i64(&mut buf, #first))
            })
        }
        "num.toFixed" => {
            let second = argv.get(1).cloned().unwrap_or_default();
            quote!(::lig_runtime::pure::f64_to_fixed(#first, (#second) as u32))
        }
        "arr.join" => {
            let second = argv.get(1).cloned().unwrap_or_default();
            quote!(::lig_runtime::pure::join(&(#first), &(#second)))
        }
        "arr.concat" => {
            let second = argv.get(1).cloned().unwrap_or_default();
            quote!(::lig_runtime::pure::concat(&(#first), &(#second)))
        }
        "str.toUpperCase" | "str.toLowerCase" | "str.trim" => {
            let method = syn::Ident::new(
                match name {
                    "str.toUpperCase" => "to_uppercase",
                    "str.toLowerCase" => "to_lowercase",
                    _ => "trim",
                },
                proc_macro2::Span::call_site(),
            );
            quote!(::alloc::string::String::from(#first . #method()))
        }
        "str.slice" => {
            let a = argv.get(1).cloned().unwrap_or_default();
            let b = argv.get(2).cloned().unwrap_or_default();
            quote!(::alloc::string::String::from((#first).get(((#a) as usize)..((#b) as usize)).unwrap_or("")))
        }
        "str.includes" => {
            let second = argv.get(1).cloned().unwrap_or_default();
            quote!((#first .contains(#second)))
        }
        "str.startsWith" => {
            let second = argv.get(1).cloned().unwrap_or_default();
            quote!((#first .starts_with(#second)))
        }
        "str.endsWith" => {
            let second = argv.get(1).cloned().unwrap_or_default();
            quote!((#first .ends_with(#second)))
        }
        _ => quote!(#first),
    }
}

fn gen_lit(module: &ViewModule, lit: &Lit) -> proc_macro2::TokenStream {
    match lit {
        Lit::Int(v) => quote!(#v),
        Lit::Float(v) => quote!(#v),
        Lit::Bool(v) => quote!(#v),
        Lit::Null => quote!(0),
        Lit::Str(s) => {
            let v = module.strings.get(*s);
            let lit = syn::LitStr::new(v, proc_macro2::Span::call_site());
            quote!(#lit)
        }
    }
}

fn binop(op: BinOp) -> proc_macro2::TokenStream {
    match op {
        BinOp::Add => quote!(+),
        BinOp::Sub => quote!(-),
        BinOp::Mul => quote!(*),
        BinOp::Div => quote!(/),
        BinOp::Mod => quote!(%),
        BinOp::Eq => quote!(==),
        BinOp::Neq => quote!(!=),
        BinOp::Lt => quote!(<),
        BinOp::Lte => quote!(<=),
        BinOp::Gt => quote!(>),
        BinOp::Gte => quote!(>=),
        BinOp::And => quote!(&&),
        BinOp::Or => quote!(||),
    }
}

fn raw_ident(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn ident_of(module: &ViewModule, id: StrId) -> syn::Ident {
    let raw = module.strings.get(id);
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let candidate = if cleaned.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("r#_{cleaned}")
    } else {
        match cleaned.as_str() {
            "fn" | "match" | "if" | "else" | "for" | "while" | "loop" | "return" | "const"
            | "let" | "type" | "impl" | "use" | "mod" | "pub" | "self" | "Self" | "true"
            | "false" | "as" | "in" | "struct" | "enum" | "where" => format!("r#{cleaned}"),
            _ => cleaned,
        }
    };
    syn::Ident::new(&candidate, proc_macro2::Span::call_site())
}

/// Compile-time smoke test of the quote→syn chain on this toolchain.
#[cfg(test)]
mod tests {
    use super::{ValueKind, gen_expr, value_kind};
    use lig_core::id::StrId;
    use lig_ir::expr::{Expr, Lit};
    use lig_ir::fragment::{FragmentIr, StateDecl, StateKind};
    use lig_ir::{Frontend, TemplateId, Ty};

    #[test]
    fn state_reads_become_field_access() {
        let mut m = lig_ir::ViewModule::new(Frontend::Solid);
        m.fragments.push(FragmentIr {
            name: StrId::new(0),
            template: TemplateId::new(0),
            dynamics: vec![],
            children: vec![],
            props: vec![],
            returns: vec![],
        });
        let count = m.strings.intern("count");
        m.states.push(StateDecl {
            name: count,
            owner: lig_ir::FragmentId::new(0),
            kind: StateKind::Signal,
            ty: Ty::Int,
            init: None,
        });
        let e = m.insert_expr(Expr::Binary(
            lig_ir::expr::BinOp::Mul,
            Box::new(Expr::State(count)),
            Box::new(Expr::Lit(Lit::Int(2))),
        ));
        let tokens = gen_expr(&m, e).expect("generates");
        assert_eq!(tokens.to_string(), "(self . count * 2i64)");
        assert_eq!(value_kind(&m, m.exprs.get(e)), ValueKind::Int);
    }

    #[test]
    fn setter_calls_map_to_methods() {
        let mut m = lig_ir::ViewModule::new(Frontend::Solid);
        m.fragments.push(FragmentIr {
            name: StrId::new(0),
            template: TemplateId::new(0),
            dynamics: vec![],
            children: vec![],
            props: vec![],
            returns: vec![],
        });
        let set_count = m.strings.intern("setCount");
        let count = m.strings.intern("count");
        m.states.push(StateDecl {
            name: count,
            owner: lig_ir::FragmentId::new(0),
            kind: StateKind::Signal,
            ty: Ty::Int,
            init: None,
        });
        let fid = m.intern_pure_fn(
            set_count,
            lig_ir::expr::PureFnKind::ModelFn { target: count },
        );
        let call = m.insert_expr(Expr::Call(fid, vec![Expr::Lit(Lit::Int(1))]));
        let tokens = gen_expr(&m, call).expect("generates");
        assert_eq!(tokens.to_string(), "self . set_count (1i64)");
    }
}
