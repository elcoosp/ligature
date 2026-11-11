//! The `isDynamic` heuristic — a port of dom-expressions
//! `shared/utils.js:111-207` semantics (§2.1, §6.1 `dyn.rs`): call
//! expressions, member expressions, spreads, and `in` expressions are
//! dynamic; functions are static; literals are static; identifiers are
//! dynamic (they read reactive state).

use oxc_ast::ast::Expression;

/// Whether an expression is dynamic (needs a runtime update) under the
/// dom-expressions syntax heuristic.
#[must_use]
pub fn is_dynamic(expr: &Expression<'_>) -> bool {
    match expr {
        // functions are static (event handlers pass through untouched);
        // literals are static
        Expression::ArrowFunctionExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::RegExpLiteral(_) => false,
        // static template literals (all quasis, no holes)
        Expression::TemplateLiteral(t) => !t.expressions.is_empty(),
        // parens are transparent
        Expression::ParenthesizedExpression(p) => is_dynamic(&p.expression),
        // everything else — identifiers, calls, members, spreads, `in` —
        // is dynamic
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use crate::dyn_check::is_dynamic;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    fn expr_of(src: &str) -> String {
        // Embed in an initializer so program-level directives cannot swallow
        // the probe expression.
        let full = format!("let q = {src};");
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, &full, SourceType::mjs()).parse();
        let stmt = ret
            .program
            .body
            .first()
            .unwrap_or_else(|| panic!("empty body for {src:?}, diags={:?}", ret.diagnostics));
        let oxc_ast::ast::Statement::VariableDeclaration(decl) = stmt else {
            panic!("expected variable declaration");
        };
        let init = decl.declarations[0].init.as_ref().expect("initializer");
        format!("{:?}", is_dynamic(init))
    }

    #[test]
    fn calls_members_spreads_are_dynamic() {
        assert_eq!(expr_of("count()"), "true");
        assert_eq!(expr_of("obj.prop"), "true");
        assert_eq!(expr_of("'a' in obj"), "true");
    }

    #[test]
    fn functions_and_literals_are_static() {
        assert_eq!(expr_of("(() => 1)"), "false");
        assert_eq!(expr_of("42"), "false");
        assert_eq!(expr_of("'hello'"), "false");
        assert_eq!(expr_of("true"), "false");
        assert_eq!(expr_of("`plain`"), "false");
    }

    #[test]
    fn templates_with_holes_are_dynamic() {
        assert_eq!(expr_of("`a${b}c`"), "true");
    }
}
