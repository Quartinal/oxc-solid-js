//! SSR template lowering helpers.
//!
//! Converts `SSRResult` into Babel-parity-ish `_$ssr(_tmpl$, ...values)` expressions.

use oxc_allocator::CloneIn;
use oxc_ast::ast::{
    Argument, Expression, FormalParameterKind, NumberBase, Statement, VariableDeclarationKind,
};
use oxc_ast::NONE;
use oxc_span::SPAN;

use crate::ir::{
    helper_ident_expr, template_var_name, GroupState, HoistedDeclarator, SSRContext, SSRResult,
};

fn is_function_expression(expr: &Expression<'_>) -> bool {
    matches!(
        expr,
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
    )
}

pub fn hoist_expression<'a>(
    context: &SSRContext<'a>,
    result: &mut SSRResult<'a>,
    mut expr: Expression<'a>,
    group: bool,
    post: bool,
    skip_wrap: bool,
) -> Expression<'a> {
    let ast = context.ast();

    if group && !post {
        let group_id = context.ensure_group_id();
        let index = context.push_group_dynamic(expr);
        return Expression::ComputedMemberExpression(ast.alloc_computed_member_expression(
            SPAN,
            ast.expression_identifier(SPAN, ast.allocator.alloc_str(&group_id)),
            ast.expression_numeric_literal(SPAN, index as f64, None, NumberBase::Decimal),
            false,
        ));
    }

    if !skip_wrap && is_function_expression(&expr) {
        context.register_helper("ssrRunInScope");
        let callee = helper_ident_expr(ast, SPAN, "ssrRunInScope");
        expr = ast.expression_call(
            SPAN,
            callee,
            None::<oxc_ast::ast::TSTypeParameterInstantiation<'a>>,
            ast.vec1(Argument::from(expr)),
            false,
        );
    }

    let name = context.generate_uid("v$");
    if post {
        result
            .post_declarations
            .push(HoistedDeclarator::new(name.clone(), expr));
    } else {
        result
            .declarations
            .push(HoistedDeclarator::new(name.clone(), expr));
    }

    ast.expression_identifier(SPAN, ast.allocator.alloc_str(&name))
}

fn build_ssr_call<'a>(
    context: &SSRContext<'a>,
    ast: oxc_ast::AstBuilder<'a>,
    template_index: usize,
    result: &SSRResult<'a>,
) -> Expression<'a> {
    context.register_helper("ssr");
    let mut args = ast.vec();

    let tmpl = template_var_name(template_index);
    args.push(Argument::from(
        ast.expression_identifier(SPAN, ast.allocator.alloc_str(&tmpl)),
    ));

    if !result.template_values.is_empty() {
        for value in &result.template_values {
            args.push(Argument::from(context.clone_expr(value)));
        }
    }

    ast.expression_call(
        SPAN,
        helper_ident_expr(ast, SPAN, "ssr"),
        None::<oxc_ast::ast::TSTypeParameterInstantiation<'a>>,
        args,
        false,
    )
}

fn push_declarators_from<'a>(
    ast: oxc_ast::AstBuilder<'a>,
    decls: &[HoistedDeclarator<'a>],
    out: &mut oxc_allocator::Vec<'a, oxc_ast::ast::VariableDeclarator<'a>>,
) {
    for decl in decls {
        out.push(ast.variable_declarator(
            SPAN,
            VariableDeclarationKind::Var,
            ast.binding_pattern_binding_identifier(SPAN, ast.allocator.alloc_str(&decl.name)),
            NONE,
            Some(decl.expr.clone_in(ast.allocator)),
            false,
        ));
    }
}

fn push_group_declarator<'a>(
    context: &SSRContext<'a>,
    ast: oxc_ast::AstBuilder<'a>,
    group_state: &GroupState<'a>,
    out: &mut oxc_allocator::Vec<'a, oxc_ast::ast::VariableDeclarator<'a>>,
) {
    let Some(group_id) = &group_state.id else {
        return;
    };
    if group_state.dynamics.is_empty() {
        return;
    }

    context.register_helper("ssrRunInScope");

    let mut elements = ast.vec_with_capacity(group_state.dynamics.len());
    for expr in &group_state.dynamics {
        elements.push(oxc_ast::ast::ArrayExpressionElement::from(
            expr.clone_in(ast.allocator),
        ));
    }
    let arr = ast.expression_array(SPAN, elements);

    let call = ast.expression_call(
        SPAN,
        helper_ident_expr(ast, SPAN, "ssrRunInScope"),
        None::<oxc_ast::ast::TSTypeParameterInstantiation<'a>>,
        ast.vec1(Argument::from(arr)),
        false,
    );

    out.push(ast.variable_declarator(
        SPAN,
        VariableDeclarationKind::Var,
        ast.binding_pattern_binding_identifier(SPAN, ast.allocator.alloc_str(group_id)),
        NONE,
        Some(call),
        false,
    ));
}

pub fn create_template_expression<'a>(
    context: &SSRContext<'a>,
    ast: oxc_ast::AstBuilder<'a>,
    result: &SSRResult<'a>,
    group_state: Option<GroupState<'a>>,
) -> Expression<'a> {
    if !result.has_template() {
        if let Some(expr) = result.first_expr() {
            return context.clone_expr(expr);
        }
        return ast.expression_identifier(SPAN, "undefined");
    }

    let template_index = context.push_template(result.template_parts.clone());

    if result.wont_escape && result.template_parts.len() <= 1 {
        let tmpl = template_var_name(template_index);
        return ast.expression_identifier(SPAN, ast.allocator.alloc_str(&tmpl));
    }

    let ssr_call = build_ssr_call(context, ast, template_index, result);

    let has_group = group_state
        .as_ref()
        .is_some_and(|g| g.id.is_some() && !g.dynamics.is_empty());

    if result.declarations.is_empty() && result.post_declarations.is_empty() && !has_group {
        return ssr_call;
    }

    let mut declarators = ast.vec();
    push_declarators_from(ast, &result.declarations, &mut declarators);

    if let Some(group_state) = group_state.as_ref() {
        push_group_declarator(context, ast, group_state, &mut declarators);
    }

    push_declarators_from(ast, &result.post_declarations, &mut declarators);

    let var_decl_stmt = Statement::VariableDeclaration(ast.alloc_variable_declaration(
        SPAN,
        VariableDeclarationKind::Var,
        declarators,
        false,
    ));

    let return_stmt = Statement::ReturnStatement(ast.alloc_return_statement(SPAN, Some(ssr_call)));

    let params = ast.alloc_formal_parameters(
        SPAN,
        FormalParameterKind::ArrowFormalParameters,
        ast.vec(),
        NONE,
    );
    let body = ast.alloc_function_body(
        SPAN,
        ast.vec(),
        ast.vec_from_array([var_decl_stmt, return_stmt]),
    );
    let arrow = ast.expression_arrow_function(SPAN, false, false, NONE, params, NONE, body);

    ast.expression_call(
        SPAN,
        arrow,
        None::<oxc_ast::ast::TSTypeParameterInstantiation<'a>>,
        ast.vec(),
        false,
    )
}
