//! Readability budget enforcement (NFR-4, §9.3): a syn visitor that walks the
//! generated file and enforces the per-function complexity caps. Violations
//! are codegen bugs, not lint warnings — `generate_component` fails closed.

use syn::visit::{self, Visit};

/// One budget violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The function that exceeded the budget.
    pub function: String,
    /// What limit and by how much.
    pub message: String,
}

/// Measured complexity of one function.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Complexity {
    /// Branch points: `if`, `match` arms, `&&`/`||`, ternaries.
    pub branches: usize,
    /// Current closure nesting depth (walking state).
    pub closure_depth: usize,
    /// Maximum closure nesting depth observed.
    pub closure_max: usize,
}

/// The §9.3 caps.
pub const MAX_BRANCHES: usize = 15;
/// The §9.3 closure cap (tightened from the draft's 3 — apply fns never need
/// more).
pub const MAX_CLOSURE_DEPTH: usize = 2;

/// Check a generated file; empty result = within budget.
#[must_use]
pub fn check(file: &syn::File) -> Vec<Violation> {
    let mut v = Budget {
        current: String::new(),
        cx: Complexity::default(),
        out: Vec::new(),
        finished: Vec::new(),
    };
    v.visit_file(file);
    v.out
}

/// Measure a single item (exposed for tests).
#[must_use]
pub fn measure(item_fn: &syn::ItemFn) -> Complexity {
    let mut v = Budget {
        current: String::new(),
        cx: Complexity::default(),
        out: Vec::new(),
        finished: Vec::new(),
    };
    v.visit_item_fn(item_fn);
    v.finished.last().copied().unwrap_or_default()
}

struct Budget {
    current: String,
    cx: Complexity,
    out: Vec<Violation>,
    finished: Vec<Complexity>,
}

impl Budget {
    fn note(&mut self) {
        self.finished.push(self.cx);
        if self.cx.branches > MAX_BRANCHES {
            self.out.push(Violation {
                function: self.current.clone(),
                message: format!(
                    "branches {} > {MAX_BRANCHES} (cyclomatic budget, §9.3)",
                    self.cx.branches
                ),
            });
        }
        if self.cx.closure_max > MAX_CLOSURE_DEPTH {
            self.out.push(Violation {
                function: self.current.clone(),
                message: format!(
                    "closure depth {} > {MAX_CLOSURE_DEPTH} (§9.3)",
                    self.cx.closure_max
                ),
            });
        }
    }
}

impl Visit<'_> for Budget {
    fn visit_item_fn(&mut self, node: &syn::ItemFn) {
        let saved = (self.current.clone(), self.cx);
        self.current = node.sig.ident.to_string();
        self.cx = Complexity::default();
        visit::visit_item_fn(self, node);
        self.note();
        let (name, cx) = saved;
        self.current = name;
        self.cx = cx;
    }

    fn visit_expr_if(&mut self, node: &syn::ExprIf) {
        self.cx.branches += 1;
        visit::visit_expr_if(self, node);
    }

    fn visit_arm(&mut self, node: &syn::Arm) {
        self.cx.branches += 1;
        visit::visit_arm(self, node);
    }

    fn visit_expr_binary(&mut self, node: &syn::ExprBinary) {
        if matches!(node.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) {
            self.cx.branches += 1;
        }
        visit::visit_expr_binary(self, node);
    }

    fn visit_expr_closure(&mut self, node: &syn::ExprClosure) {
        self.cx.closure_depth += 1;
        self.cx.closure_max = self.cx.closure_max.max(self.cx.closure_depth);
        visit::visit_expr_closure(self, node);
        self.cx.closure_depth -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{Complexity, check, measure};
    use quote::quote;
    use syn::parse_quote;

    #[test]
    fn simple_fn_within_budget() {
        let f: syn::ItemFn = parse_quote! {
            fn simple(x: i64) -> i64 {
                if x > 0 { x } else { -x }
            }
        };
        let cx = measure(&f);
        assert_eq!(cx.branches, 1);
        assert_eq!(cx.closure_max, 0);
    }

    #[test]
    fn excessive_branches_violate() {
        // Linear if-chain (a doubling loop would build 2^n nodes).
        let mut body = quote! { x };
        for _ in 0..20 {
            body = quote! { if x > 0 { #body } else { x } };
        }
        let f: syn::ItemFn = syn::parse2(quote! {
            fn hairy(x: i64) -> i64 { #body }
        })
        .expect("parses");
        let violations = check(&syn::File {
            shebang: None,
            attrs: vec![],
            items: vec![syn::Item::Fn(f)],
            frontmatter: None,
        });
        assert!(
            violations.iter().any(|v| v.function == "hairy"),
            "{violations:?}"
        );
    }

    #[test]
    fn deep_closures_violate() {
        let f: syn::ItemFn = parse_quote! {
            fn nested() -> i64 {
                (0..3).map(|a| (0..3).map(|b| (0..3).map(|c| a + b + c).sum::<i64>()).sum::<i64>()).sum()
            }
        };
        let cx = measure(&f);
        assert!(cx.closure_max > 2, "{cx:?}");
        assert_eq!(
            cx,
            Complexity {
                branches: 0,
                closure_depth: 0,
                closure_max: 3
            }
        );
    }
}
