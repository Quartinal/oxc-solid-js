use oxc_allocator::CloneIn;
use oxc_ast::ast::{
    Argument, ArrayExpressionElement, Expression, FormalParameterKind, PropertyKind, Statement,
    VariableDeclarationKind,
};
use oxc_ast::{AstBuilder, NONE};
use oxc_span::{Span, SPAN};
use oxc_syntax::operator::BinaryOperator;

use crate::ir::{helper_ident_expr, BlockContext, DynamicBinding, OutputKind, TransformResult};
use crate::output::build_dom_output_expr;
use crate::output_helpers::{
    arrow_zero_params_body, call_expr, get_numbered_id, ident_expr, inline_effect_source_expr,
    static_member,
};

fn set_prop_expr_with_value<'a>(
    ast: AstBuilder<'a>,
    span: Span,
    binding: &DynamicBinding<'a>,
    value: Expression<'a>,
    prev_value: Option<Expression<'a>>,
) -> Expression<'a> {
    let elem = ident_expr(ast, span, &binding.elem);
    let name = ast.expression_string_literal(span, ast.allocator.alloc_str(&binding.key), None);

    let mut args = ast.vec_with_capacity(if prev_value.is_some() { 4 } else { 3 });
    args.push(Argument::from(elem));
    args.push(Argument::from(name));
    args.push(Argument::from(value));
    if let Some(prev) = prev_value {
        args.push(Argument::from(prev));
    }

    ast.expression_call(
        span,
        helper_ident_expr(ast, span, "setProp"),
        None::<oxc_ast::ast::TSTypeParameterInstantiation<'a>>,
        args,
        false,
    )
}

fn build_single_dynamic_effect_call<'a>(
    ast: AstBuilder<'a>,
    span: Span,
    binding: &DynamicBinding<'a>,
) -> Expression<'a> {
    let value_name = "_v$";
    let prev_name = "_$p";

    let mut callback_params = ast.vec_with_capacity(2);
    callback_params.push(ast.plain_formal_parameter(
        span,
        ast.binding_pattern_binding_identifier(span, ast.allocator.alloc_str(value_name)),
    ));
    callback_params.push(ast.plain_formal_parameter(
        span,
        ast.binding_pattern_binding_identifier(span, ast.allocator.alloc_str(prev_name)),
    ));
    let callback_params = ast.alloc_formal_parameters(
        span,
        FormalParameterKind::ArrowFormalParameters,
        callback_params,
        NONE,
    );

    let setter = set_prop_expr_with_value(
        ast,
        span,
        binding,
        ident_expr(ast, span, value_name),
        Some(ident_expr(ast, span, prev_name)),
    );

    let mut callback_statements = ast.vec_with_capacity(1);
    callback_statements.push(Statement::ExpressionStatement(
        ast.alloc_expression_statement(span, setter),
    ));
    let callback_body = ast.alloc_function_body(span, ast.vec(), callback_statements);
    let callback = ast.expression_arrow_function(
        span,
        false,
        false,
        NONE,
        callback_params,
        NONE,
        callback_body,
    );

    let source = inline_effect_source_expr(ast, span, &binding.value);
    call_expr(
        ast,
        span,
        helper_ident_expr(ast, span, "effect"),
        [source, callback],
    )
}

fn build_multi_dynamic_effect_call<'a>(
    ast: AstBuilder<'a>,
    span: Span,
    dynamics: &[DynamicBinding<'a>],
) -> Expression<'a> {
    let prev_name = "_p$";
    let ids: Vec<String> = (0..dynamics.len()).map(get_numbered_id).collect();

    let mut value_props = ast.vec_with_capacity(dynamics.len());
    let mut current_props = ast.vec_with_capacity(dynamics.len());
    let mut prev_props = ast.vec_with_capacity(dynamics.len());
    let mut update_statements = ast.vec_with_capacity(dynamics.len());

    for (binding, id) in dynamics.iter().zip(ids.iter()) {
        let value_expr = binding.value.clone_in(ast.allocator);

        let key = ast.property_key_static_identifier(span, ast.allocator.alloc_str(id));
        value_props.push(ast.object_property_kind_object_property(
            span,
            PropertyKind::Init,
            key,
            value_expr,
            false,
            false,
            false,
        ));

        let current_key = ast.property_key_static_identifier(span, ast.allocator.alloc_str(id));
        let current_value =
            ast.binding_pattern_binding_identifier(span, ast.allocator.alloc_str(id));
        current_props.push(ast.binding_property(span, current_key, current_value, true, false));

        let prev_key = ast.property_key_static_identifier(span, ast.allocator.alloc_str(id));
        prev_props.push(ast.object_property_kind_object_property(
            span,
            PropertyKind::Init,
            prev_key,
            ident_expr(ast, span, "undefined"),
            false,
            false,
            false,
        ));

        let current = ident_expr(ast, span, id);
        let prev = static_member(ast, span, ident_expr(ast, span, prev_name), id);

        let setter = set_prop_expr_with_value(
            ast,
            span,
            binding,
            current.clone_in(ast.allocator),
            Some(prev.clone_in(ast.allocator)),
        );

        let changed = ast.expression_binary(span, current, BinaryOperator::StrictInequality, prev);
        let update = ast.expression_logical(
            span,
            changed,
            oxc_syntax::operator::LogicalOperator::And,
            setter,
        );

        update_statements.push(Statement::ExpressionStatement(
            ast.alloc_expression_statement(span, update),
        ));
    }

    let values = ast.expression_object(span, value_props);
    let getter = arrow_zero_params_body(ast, span, values);

    let mut callback_params = ast.vec_with_capacity(2);
    let current_pattern = ast.binding_pattern_object_pattern(span, current_props, NONE);
    callback_params.push(ast.plain_formal_parameter(span, current_pattern));
    callback_params.push(ast.plain_formal_parameter(
        span,
        ast.binding_pattern_binding_identifier(span, ast.allocator.alloc_str(prev_name)),
    ));
    let callback_params = ast.alloc_formal_parameters(
        span,
        FormalParameterKind::ArrowFormalParameters,
        callback_params,
        NONE,
    );

    let callback_body = ast.alloc_function_body(span, ast.vec(), update_statements);
    let callback = ast.expression_arrow_function(
        span,
        false,
        false,
        NONE,
        callback_params,
        NONE,
        callback_body,
    );

    let init = ast.expression_object(span, prev_props);
    call_expr(
        ast,
        span,
        helper_ident_expr(ast, span, "effect"),
        [getter, callback, init],
    )
}

pub fn build_universal_output_expr<'a>(
    result: &TransformResult<'a>,
    context: &BlockContext<'a>,
) -> Expression<'a> {
    let ast = context.ast();
    let gen_span = SPAN;

    if !result.child_results.is_empty() {
        let mut elements = ast.vec_with_capacity(result.child_results.len());
        for child in &result.child_results {
            let expr = match child.output_kind {
                OutputKind::Universal => build_universal_output_expr(child, context),
                OutputKind::Dom => build_dom_output_expr(child, context),
            };
            elements.push(ArrayExpressionElement::from(expr));
        }
        return ast.expression_array(gen_span, elements);
    }

    if result.text && !result.template.is_empty() {
        return ast.expression_string_literal(
            gen_span,
            ast.allocator.alloc_str(&result.template),
            None,
        );
    }

    if let Some(elem_id) = result.id.as_ref() {
        let has_no_runtime_work = result.statements.is_empty()
            && result.exprs.is_empty()
            && result.dynamics.is_empty()
            && result.post_exprs.is_empty();
        if has_no_runtime_work && result.declarations.len() == 1 {
            return result.declarations[0].init.clone_in(ast.allocator);
        }

        let mut statements = ast.vec();

        if !result.declarations.is_empty() {
            let mut declarators = ast.vec_with_capacity(result.declarations.len());
            for decl in &result.declarations {
                declarators.push(ast.variable_declarator(
                    gen_span,
                    VariableDeclarationKind::Var,
                    decl.pattern.clone_in(ast.allocator),
                    NONE,
                    Some(decl.init.clone_in(ast.allocator)),
                    false,
                ));
            }

            statements.push(Statement::VariableDeclaration(
                ast.alloc_variable_declaration(
                    gen_span,
                    VariableDeclarationKind::Var,
                    declarators,
                    false,
                ),
            ));
        }

        for stmt in &result.statements {
            statements.push(stmt.clone_in(ast.allocator));
        }

        for expr in &result.exprs {
            statements.push(Statement::ExpressionStatement(
                ast.alloc_expression_statement(gen_span, expr.clone_in(ast.allocator)),
            ));
        }

        if !result.dynamics.is_empty() {
            if context.effect_wrapper_enabled {
                context.register_helper("effect");
                context.register_helper("setProp");

                let effect_call = if result.dynamics.len() == 1 {
                    build_single_dynamic_effect_call(ast, gen_span, &result.dynamics[0])
                } else {
                    build_multi_dynamic_effect_call(ast, gen_span, &result.dynamics)
                };

                statements.push(Statement::ExpressionStatement(
                    ast.alloc_expression_statement(gen_span, effect_call),
                ));
            } else {
                context.register_helper("setProp");
                for binding in &result.dynamics {
                    let setter = set_prop_expr_with_value(
                        ast,
                        gen_span,
                        binding,
                        binding.value.clone_in(ast.allocator),
                        None,
                    );
                    statements.push(Statement::ExpressionStatement(
                        ast.alloc_expression_statement(gen_span, setter),
                    ));
                }
            }
        }

        for expr in &result.post_exprs {
            statements.push(Statement::ExpressionStatement(
                ast.alloc_expression_statement(gen_span, expr.clone_in(ast.allocator)),
            ));
        }

        statements.push(Statement::ReturnStatement(ast.alloc_return_statement(
            gen_span,
            Some(ident_expr(ast, gen_span, elem_id)),
        )));

        let params = ast.alloc_formal_parameters(
            gen_span,
            FormalParameterKind::ArrowFormalParameters,
            ast.vec(),
            NONE,
        );
        let body = ast.alloc_function_body(gen_span, ast.vec(), statements);
        let arrow_fn =
            ast.expression_arrow_function(gen_span, false, false, NONE, params, NONE, body);
        return call_expr(ast, gen_span, arrow_fn, []);
    }

    if !result.exprs.is_empty() {
        if result.needs_memo {
            context.register_helper("memo");
            let callee = helper_ident_expr(ast, gen_span, "memo");
            let mut args = ast.vec_with_capacity(result.exprs.len());
            for expr in &result.exprs {
                args.push(Argument::from(expr.clone_in(ast.allocator)));
            }
            return ast.expression_call(
                gen_span,
                callee,
                None::<oxc_ast::ast::TSTypeParameterInstantiation<'a>>,
                args,
                false,
            );
        }

        if result.exprs.len() == 1 {
            return result.exprs[0].clone_in(ast.allocator);
        }

        let mut exprs = ast.vec_with_capacity(result.exprs.len());
        for expr in &result.exprs {
            exprs.push(expr.clone_in(ast.allocator));
        }
        return ast.expression_sequence(gen_span, exprs);
    }

    ast.expression_string_literal(gen_span, ast.allocator.alloc_str(""), None)
}
