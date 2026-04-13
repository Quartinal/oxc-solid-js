use oxc_ast::ast::{AssignmentTarget, Expression};

pub(crate) fn peel_wrapped_expression<'a, 'b>(expr: &'b Expression<'a>) -> &'b Expression<'a> {
    match expr {
        Expression::ParenthesizedExpression(paren) => peel_wrapped_expression(&paren.expression),
        Expression::TSAsExpression(ts) => peel_wrapped_expression(&ts.expression),
        Expression::TSSatisfiesExpression(ts) => peel_wrapped_expression(&ts.expression),
        Expression::TSNonNullExpression(ts) => peel_wrapped_expression(&ts.expression),
        Expression::TSTypeAssertion(ts) => peel_wrapped_expression(&ts.expression),
        _ => expr,
    }
}

pub(crate) fn expression_to_assignment_target<'a>(
    expr: Expression<'a>,
) -> Option<AssignmentTarget<'a>> {
    match expr {
        Expression::Identifier(ident) => Some(AssignmentTarget::AssignmentTargetIdentifier(ident)),
        Expression::StaticMemberExpression(m) => Some(AssignmentTarget::StaticMemberExpression(m)),
        Expression::ComputedMemberExpression(m) => {
            Some(AssignmentTarget::ComputedMemberExpression(m))
        }
        Expression::PrivateFieldExpression(m) => Some(AssignmentTarget::PrivateFieldExpression(m)),
        Expression::ParenthesizedExpression(e) => {
            expression_to_assignment_target(e.unbox().expression)
        }
        // Strip TS type wrappers — the inner expression is the actual assignment target.
        // Keeping the `as`/`satisfies` in assignment position produces invalid syntax
        // (e.g. `local.ref as SomeType = r$` is ambiguous/invalid).
        Expression::TSAsExpression(e) => expression_to_assignment_target(e.unbox().expression),
        Expression::TSSatisfiesExpression(e) => {
            expression_to_assignment_target(e.unbox().expression)
        }
        Expression::TSNonNullExpression(e) => expression_to_assignment_target(e.unbox().expression),
        Expression::TSTypeAssertion(e) => expression_to_assignment_target(e.unbox().expression),
        _ => None,
    }
}
