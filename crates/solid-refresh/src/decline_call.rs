//! `@refresh reload` handling per bundler.
//!
//! Builds the HMR decline/invalidate statement:
//! - Vite: `if (import.meta.hot) { import.meta.hot.accept(() => import.meta.hot.invalidate()) }`
//! - Others: `if (import.meta.hot) { $$decline("bundler", import.meta.hot) }`

use oxc_ast::ast::{Argument, Expression, FormalParameterKind, Statement};
use oxc_ast::{AstBuilder, NONE};
use oxc_span::SPAN;

use crate::constants::IMPORT_DECLINE;
use crate::hot_identifier::build_hot_identifier;
use crate::import_identifier::get_import_identifier;
use crate::types::{RuntimeType, StateContext};

/// Builds the HMR decline/invalidate statement for `@refresh reload`.
///
/// For Vite bundler, generates:
/// ```js
/// if (import.meta.hot) {
///   import.meta.hot.accept(() => import.meta.hot.invalidate());
/// }
/// ```
///
/// For all other bundlers, generates:
/// ```js
/// if (import.meta.hot) {
///   $$decline("bundler", import.meta.hot);
/// }
/// ```
pub fn build_hmr_decline_call<'a>(
    state: &mut StateContext<'_>,
    ast: AstBuilder<'a>,
) -> Statement<'a> {
    // test expression for if-statement: import.meta.hot / module.hot
    let test = build_hot_identifier(ast, state.bundler);

    if state.bundler == RuntimeType::Vite {
        build_vite_decline(ast, state.bundler)
    } else {
        build_standard_decline(state, ast)
    }
    .build_if_statement(ast, test)
}

/// Vite path: `import.meta.hot.accept(() => import.meta.hot.invalidate())`
fn build_vite_decline<'a>(ast: AstBuilder<'a>, bundler: RuntimeType) -> Expression<'a> {
    // import.meta.hot.invalidate()
    let hot_for_invalidate = build_hot_identifier(ast, bundler);
    let invalidate_callee = Expression::StaticMemberExpression(ast.alloc_static_member_expression(
        SPAN,
        hot_for_invalidate,
        ast.identifier_name(SPAN, "invalidate"),
        false,
    ));
    let invalidate_call = ast.expression_call(SPAN, invalidate_callee, NONE, ast.vec(), false);

    // () => import.meta.hot.invalidate()
    let params = ast.alloc_formal_parameters(
        SPAN,
        FormalParameterKind::ArrowFormalParameters,
        ast.vec(),
        NONE,
    );
    let body = ast.alloc_function_body(
        SPAN,
        ast.vec(),
        ast.vec1(ast.statement_expression(SPAN, invalidate_call)),
    );
    let arrow = ast.expression_arrow_function(SPAN, true, false, NONE, params, NONE, body);

    // import.meta.hot.accept(() => ...)
    let hot_for_accept = build_hot_identifier(ast, bundler);
    let accept_callee = Expression::StaticMemberExpression(ast.alloc_static_member_expression(
        SPAN,
        hot_for_accept,
        ast.identifier_name(SPAN, "accept"),
        false,
    ));
    ast.expression_call(
        SPAN,
        accept_callee,
        NONE,
        ast.vec1(Argument::from(arrow)),
        false,
    )
}

/// Standard path: `$$decline("bundler", import.meta.hot)`
fn build_standard_decline<'a>(state: &mut StateContext<'_>, ast: AstBuilder<'a>) -> Expression<'a> {
    let decline_name = get_import_identifier(state, &IMPORT_DECLINE);
    let decline_callee = ast.expression_identifier(SPAN, ast.allocator.alloc_str(&decline_name));

    let hot_arg = build_hot_identifier(ast, state.bundler);
    let bundler_str =
        ast.expression_string_literal(SPAN, ast.allocator.alloc_str(state.bundler.as_str()), None);

    let mut args = ast.vec_with_capacity(2);
    args.push(Argument::from(bundler_str));
    args.push(Argument::from(hot_arg));

    ast.expression_call(SPAN, decline_callee, NONE, args, false)
}

/// Helper trait to wrap a call expression in `if (test) { <expr>; }`.
trait IntoIfStatement<'a> {
    fn build_if_statement(self, ast: AstBuilder<'a>, test: Expression<'a>) -> Statement<'a>;
}

impl<'a> IntoIfStatement<'a> for Expression<'a> {
    fn build_if_statement(self, ast: AstBuilder<'a>, test: Expression<'a>) -> Statement<'a> {
        let expr_stmt = ast.statement_expression(SPAN, self);
        let body = Statement::BlockStatement(ast.alloc_block_statement(SPAN, ast.vec1(expr_stmt)));
        ast.statement_if(SPAN, test, body, None)
    }
}
