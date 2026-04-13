use oxc_ast::ast::Expression;

/// Returns true if the expression is a type-wrapping expression
/// (parenthesized, TS cast, etc.) that should be unwrapped to find the "real" expression.
#[inline]
pub fn is_nested_expression(expr: &Expression<'_>) -> bool {
    matches!(
        expr,
        Expression::ParenthesizedExpression(_)
            | Expression::TSAsExpression(_)
            | Expression::TSSatisfiesExpression(_)
            | Expression::TSNonNullExpression(_)
            | Expression::TSInstantiationExpression(_)
            | Expression::TSTypeAssertion(_)
    )
}

/// Recursively unwraps parenthesized/TS type expressions to find the inner expression.
/// Returns a reference to the innermost non-wrapper expression.
pub fn unwrap_expression<'a, 'b>(expr: &'b Expression<'a>) -> &'b Expression<'a> {
    match expr {
        Expression::ParenthesizedExpression(e) => unwrap_expression(&e.expression),
        Expression::TSAsExpression(e) => unwrap_expression(&e.expression),
        Expression::TSSatisfiesExpression(e) => unwrap_expression(&e.expression),
        Expression::TSNonNullExpression(e) => unwrap_expression(&e.expression),
        Expression::TSInstantiationExpression(e) => unwrap_expression(&e.expression),
        Expression::TSTypeAssertion(e) => unwrap_expression(&e.expression),
        _ => expr,
    }
}
