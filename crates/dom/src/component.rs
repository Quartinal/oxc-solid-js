//! Component transform
//! Handles <MyComponent /> -> createComponent(MyComponent, {...})

use oxc_allocator::CloneIn;
use oxc_ast::ast::{
    Argument, ArrayExpressionElement, Expression, FormalParameterKind, FunctionType,
    JSXAttributeItem, JSXAttributeName, JSXAttributeValue, JSXChild, JSXElement, JSXElementName,
    JSXMemberExpression, JSXMemberExpressionObject, ObjectPropertyKind, PropertyKey, PropertyKind,
    Statement, VariableDeclarationKind,
};
use oxc_ast::AstBuilder;
use oxc_ast::NONE;
use oxc_span::SPAN;
use oxc_syntax::identifier::is_identifier_name;
use oxc_syntax::keyword::is_reserved_keyword;
use oxc_syntax::operator::{AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator};
use oxc_traverse::TraverseCtx;

use common::{is_dynamic, GenerateMode, TransformOptions, JSX_MEMBER_DASH_SENTINEL};

use crate::conditional::{is_condition_expression, transform_condition_inline_expr};
use crate::element::is_writable_ref_target;
use crate::expression_utils::{expression_to_assignment_target, peel_wrapped_expression};
use crate::ir::{helper_ident_expr, BlockContext, ChildTransformer, TransformResult};
use crate::output::build_dom_output_expr;
use crate::universal_output::build_universal_output_expr;

fn decode_jsx_member_segment(name: &str) -> std::borrow::Cow<'_, str> {
    if name.contains(JSX_MEMBER_DASH_SENTINEL) {
        std::borrow::Cow::Owned(name.replace(JSX_MEMBER_DASH_SENTINEL, "-"))
    } else {
        std::borrow::Cow::Borrowed(name)
    }
}

fn is_valid_member_property_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c == '$' || c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '$' || c == '_' || c.is_ascii_alphanumeric())
}

fn jsx_member_expression_to_expression<'a>(
    ast: AstBuilder<'a>,
    member: &JSXMemberExpression<'a>,
) -> Expression<'a> {
    let object = match &member.object {
        JSXMemberExpressionObject::IdentifierReference(id) => {
            ast.expression_identifier(id.span, id.name)
        }
        JSXMemberExpressionObject::MemberExpression(inner) => {
            jsx_member_expression_to_expression(ast, inner)
        }
        JSXMemberExpressionObject::ThisExpression(expr) => ast.expression_this(expr.span),
    };

    let property_name = decode_jsx_member_segment(member.property.name.as_str());

    if is_valid_member_property_identifier(property_name.as_ref()) {
        let property = ast.identifier_name(
            member.property.span,
            ast.allocator.alloc_str(property_name.as_ref()),
        );
        Expression::StaticMemberExpression(ast.alloc_static_member_expression(
            member.span,
            object,
            property,
            false,
        ))
    } else {
        let property = ast.expression_string_literal(
            member.property.span,
            ast.allocator.alloc_str(property_name.as_ref()),
            None,
        );
        Expression::ComputedMemberExpression(ast.alloc_computed_member_expression(
            member.span,
            object,
            property,
            false,
        ))
    }
}

fn jsx_element_name_to_expression<'a>(
    ast: AstBuilder<'a>,
    name: &JSXElementName<'a>,
) -> Expression<'a> {
    match name {
        JSXElementName::Identifier(id) => ast.expression_identifier(id.span, id.name),
        JSXElementName::IdentifierReference(id) => ast.expression_identifier(id.span, id.name),
        JSXElementName::MemberExpression(member) => {
            jsx_member_expression_to_expression(ast, member)
        }
        JSXElementName::ThisExpression(expr) => ast.expression_this(expr.span),
        JSXElementName::NamespacedName(ns) => {
            // Namespaced tag names are not valid component references in JS.
            let _ = ns;
            ast.expression_identifier(SPAN, "undefined")
        }
    }
}

fn component_callee_expression<'a>(
    name: &JSXElementName<'a>,
    context: &BlockContext<'a>,
    options: &TransformOptions<'a>,
    ctx: &TraverseCtx<'a, ()>,
) -> Expression<'a> {
    let ast = context.ast();

    let builtin_name = match name {
        JSXElementName::Identifier(id) => Some(id.name.as_str()),
        JSXElementName::IdentifierReference(id) => Some(id.name.as_str()),
        _ => None,
    };

    if let Some(tag_name) = builtin_name {
        if options.built_ins.iter().any(|builtin| *builtin == tag_name)
            && ctx
                .scoping()
                .find_binding(ctx.current_scope_id(), tag_name.into())
                .is_none()
        {
            context.register_helper(tag_name);
            return helper_ident_expr(ast, SPAN, tag_name);
        }
    }

    jsx_element_name_to_expression(ast, name)
}

fn getter_return_expr<'a>(
    ast: AstBuilder<'a>,
    span: oxc_span::Span,
    expr: Expression<'a>,
) -> Expression<'a> {
    let _ = span;
    let params =
        ast.alloc_formal_parameters(SPAN, FormalParameterKind::FormalParameter, ast.vec(), NONE);
    let mut statements = ast.vec_with_capacity(1);
    statements.push(Statement::ReturnStatement(
        ast.alloc_return_statement(SPAN, Some(expr)),
    ));
    let body = ast.alloc_function_body(SPAN, ast.vec(), statements);
    ast.expression_function(
        SPAN,
        FunctionType::FunctionExpression,
        None,
        false,
        false,
        false,
        NONE,
        NONE,
        params,
        NONE,
        Some(body),
    )
}

fn arrow_return_expr<'a>(
    ast: AstBuilder<'a>,
    span: oxc_span::Span,
    expr: Expression<'a>,
) -> Expression<'a> {
    let _ = span;
    let params = ast.alloc_formal_parameters(
        SPAN,
        FormalParameterKind::ArrowFormalParameters,
        ast.vec(),
        NONE,
    );
    let mut statements = ast.vec_with_capacity(1);
    statements.push(Statement::ExpressionStatement(
        ast.alloc_expression_statement(SPAN, expr),
    ));
    let body = ast.alloc_function_body(SPAN, ast.vec(), statements);
    ast.expression_arrow_function(SPAN, true, false, NONE, params, NONE, body)
}

fn memo_wrapper_enabled(options: &TransformOptions<'_>) -> bool {
    !options.memo_wrapper.is_empty()
}

fn memo_wrap_expr<'a>(
    ast: AstBuilder<'a>,
    span: oxc_span::Span,
    expr: Expression<'a>,
    context: &BlockContext<'a>,
) -> Expression<'a> {
    context.register_helper("memo");
    let callee = helper_ident_expr(ast, span, "memo");
    let mut args = ast.vec_with_capacity(1);
    args.push(Argument::from(expr));
    ast.expression_call(
        SPAN,
        callee,
        None::<oxc_ast::ast::TSTypeParameterInstantiation<'a>>,
        args,
        false,
    )
}

fn is_valid_prop_identifier(key: &str) -> bool {
    is_identifier_name(key) && !is_reserved_keyword(key)
}

fn make_prop_key<'a>(
    ast: AstBuilder<'a>,
    span: oxc_span::Span,
    raw_key: &str,
) -> (PropertyKey<'a>, bool) {
    let _ = span;
    let key = ast.allocator.alloc_str(raw_key);
    let is_valid_identifier = is_valid_prop_identifier(raw_key);
    let property_key = if is_valid_identifier {
        PropertyKey::StaticIdentifier(ast.alloc_identifier_name(SPAN, key))
    } else {
        PropertyKey::StringLiteral(ast.alloc_string_literal(SPAN, key, None))
    };

    (property_key, is_valid_identifier)
}

/// Get children as an expression with recursive transformation.
///
/// Returns `(children_expr, dynamic_children)` where `dynamic_children` mirrors
/// Babel's component-children behavior:
/// - multiple children always use a getter
/// - single element/fragment child uses a getter
/// - single text child is static
/// - single expression/spread child follows expression dynamism
fn get_children_expr_transformed<'a, 'b>(
    element: &JSXElement<'a>,
    context: &BlockContext<'a>,
    options: &TransformOptions<'a>,
    transform_child: ChildTransformer<'a, 'b>,
) -> Option<(Expression<'a>, bool)> {
    #[derive(Clone, Copy)]
    enum ChildKind {
        Text,
        ExprLike { dynamic: bool },
        ElementLike,
    }

    let ast = context.ast();
    let mut children: Vec<Expression<'a>> = Vec::new();
    let mut child_kinds: Vec<ChildKind> = Vec::new();

    for child in &element.children {
        match child {
            JSXChild::Text(text) => {
                let content = common::expression::normalize_jsx_text(text);
                if !content.is_empty() {
                    let decoded = common::expression::decode_html_entities(&content);
                    children.push(ast.expression_string_literal(
                        SPAN,
                        ast.allocator.alloc_str(&decoded),
                        None,
                    ));
                    child_kinds.push(ChildKind::Text);
                }
            }
            JSXChild::ExpressionContainer(container) => {
                if let Some(expr) = container.expression.as_expression() {
                    let has_static_marker =
                        context.has_static_marker_comment(container.span, options.static_marker);
                    let dynamic = !has_static_marker && is_dynamic(expr);

                    let child_expr = if has_static_marker {
                        context.clone_expr_without_trivia(expr)
                    } else {
                        context.clone_expr(expr)
                    };

                    children.push(child_expr);
                    child_kinds.push(ChildKind::ExprLike { dynamic });
                }
            }
            JSXChild::Element(_) | JSXChild::Fragment(_) => {
                if let Some(result) = transform_child(child) {
                    let child_expr = match options.generate {
                        GenerateMode::Universal => build_universal_output_expr(&result, context),
                        GenerateMode::Dynamic if result.uses_universal_output() => {
                            build_universal_output_expr(&result, context)
                        }
                        _ => build_dom_output_expr(&result, context),
                    };
                    children.push(child_expr);
                    child_kinds.push(ChildKind::ElementLike);
                }
            }
            JSXChild::Spread(spread) => {
                let dynamic = is_dynamic(&spread.expression);
                let child_expr = context.clone_expr(&spread.expression);
                children.push(child_expr);
                child_kinds.push(ChildKind::ExprLike { dynamic });
            }
        }
    }

    if children.is_empty() {
        return None;
    }

    if children.len() == 1 {
        let expr = children
            .pop()
            .unwrap_or_else(|| ast.expression_identifier(SPAN, "undefined"));
        let kind = child_kinds
            .pop()
            .unwrap_or(ChildKind::ExprLike { dynamic: true });
        let dynamic = match kind {
            ChildKind::Text => false,
            ChildKind::ElementLike => true,
            ChildKind::ExprLike { dynamic } => dynamic,
        };
        Some((expr, dynamic))
    } else {
        for (index, kind) in child_kinds.iter().enumerate() {
            let ChildKind::ExprLike { dynamic: true } = kind else {
                continue;
            };

            let mut child_expr = std::mem::replace(
                &mut children[index],
                ast.expression_identifier(SPAN, "undefined"),
            );
            if options.wrap_conditionals
                && memo_wrapper_enabled(options)
                && is_condition_expression(&child_expr)
            {
                child_expr = transform_condition_inline_expr(child_expr, context);
            }

            if memo_wrapper_enabled(options) {
                let memo_arg = arrow_return_expr(ast, SPAN, child_expr);
                children[index] = memo_wrap_expr(ast, SPAN, memo_arg, context);
            } else {
                // Wrapperless parity: keep dynamic entries reactive via plain accessors,
                // but do not memo-wrap them.
                children[index] = arrow_return_expr(ast, SPAN, child_expr);
            }
        }

        let mut elements = ast.vec_with_capacity(children.len());
        for expr in children {
            elements.push(ArrayExpressionElement::from(expr));
        }
        Some((ast.expression_array(SPAN, elements), true))
    }
}

/// Transform a component element
pub fn transform_component<'a, 'b>(
    element: &JSXElement<'a>,
    _tag_name: &str,
    context: &BlockContext<'a>,
    options: &TransformOptions<'a>,
    transform_child: ChildTransformer<'a, 'b>,
    ctx: &TraverseCtx<'a, ()>,
) -> TransformResult<'a> {
    let ast = context.ast();
    let mut result = TransformResult {
        span: element.span,
        ..Default::default()
    };

    // Build props object
    let props = build_props(element, context, options, transform_child, ctx);

    context.register_helper("createComponent");

    // Generate createComponent call
    let callee = helper_ident_expr(ast, SPAN, "createComponent");
    let mut args = ast.vec_with_capacity(2);
    args.push(Argument::from(component_callee_expression(
        &element.opening_element.name,
        context,
        options,
        ctx,
    )));
    args.push(Argument::from(props));
    result.exprs.push(ast.expression_call(
        SPAN,
        callee,
        None::<oxc_ast::ast::TSTypeParameterInstantiation<'a>>,
        args,
        false,
    ));

    result
}

fn getter_from_zero_arg_arrow_iife<'a>(
    expr: &Expression<'a>,
    context: &BlockContext<'a>,
    options: &TransformOptions<'a>,
) -> Option<Expression<'a>> {
    let Expression::CallExpression(call) = peel_wrapped_expression(expr) else {
        return None;
    };
    if !call.arguments.is_empty() {
        return None;
    }

    let Expression::ArrowFunctionExpression(arrow) = peel_wrapped_expression(&call.callee) else {
        return None;
    };

    if !arrow.params.items.is_empty() || arrow.params.rest.is_some() {
        return None;
    }

    let ast = context.ast();

    if arrow.expression {
        let Some(Statement::ExpressionStatement(expr_stmt)) = arrow.body.statements.first() else {
            return None;
        };

        let mut body_expr = context.clone_expr(&expr_stmt.expression);
        if options.wrap_conditionals
            && memo_wrapper_enabled(options)
            && is_condition_expression(&body_expr)
        {
            body_expr = transform_condition_inline_expr(body_expr, context);
        }

        return Some(getter_return_expr(ast, SPAN, body_expr));
    }

    let params =
        ast.alloc_formal_parameters(SPAN, FormalParameterKind::FormalParameter, ast.vec(), NONE);
    let body = Some(arrow.body.clone_in(ast.allocator));

    Some(ast.expression_function(
        SPAN,
        FunctionType::FunctionExpression,
        None,
        false,
        false,
        false,
        NONE,
        NONE,
        params,
        NONE,
        body,
    ))
}

/// Build props object for a component.
fn build_props<'a, 'b>(
    element: &JSXElement<'a>,
    context: &BlockContext<'a>,
    options: &TransformOptions<'a>,
    transform_child: ChildTransformer<'a, 'b>,
    ctx: &TraverseCtx<'a, ()>,
) -> Expression<'a> {
    let ast = context.ast();
    let span = SPAN;

    let mut props_parts: Vec<Expression<'a>> = Vec::new();
    let mut running_props: Vec<ObjectPropertyKind<'a>> = Vec::new();
    let mut dynamic_spread = false;

    let flush_running_props =
        |props_parts: &mut Vec<Expression<'a>>, running_props: &mut Vec<ObjectPropertyKind<'a>>| {
            if running_props.is_empty() {
                return;
            }

            let mut object_props = ast.vec_with_capacity(running_props.len());
            for prop in running_props.drain(..) {
                object_props.push(prop);
            }
            props_parts.push(ast.expression_object(span, object_props));
        };

    for attr in &element.opening_element.attributes {
        match attr {
            JSXAttributeItem::SpreadAttribute(spread) => {
                flush_running_props(&mut props_parts, &mut running_props);

                let spread_expr = if is_dynamic(&spread.argument) {
                    dynamic_spread = true;
                    if let Expression::CallExpression(call) = &spread.argument {
                        if call.arguments.is_empty()
                            && !matches!(
                                call.callee,
                                Expression::CallExpression(_)
                                    | Expression::StaticMemberExpression(_)
                                    | Expression::ComputedMemberExpression(_)
                            )
                        {
                            context.clone_expr(&call.callee)
                        } else {
                            arrow_return_expr(ast, span, context.clone_expr(&spread.argument))
                        }
                    } else {
                        arrow_return_expr(ast, span, context.clone_expr(&spread.argument))
                    }
                } else {
                    context.clone_expr(&spread.argument)
                };

                props_parts.push(spread_expr);
            }
            JSXAttributeItem::Attribute(attr) => {
                let raw_key: String = match &attr.name {
                    JSXAttributeName::Identifier(id) => id.name.as_str().to_string(),
                    JSXAttributeName::NamespacedName(ns) => {
                        format!("{}:{}", ns.namespace.name, ns.name.name)
                    }
                };

                if raw_key == "children" && !element.children.is_empty() {
                    continue;
                }

                if raw_key == "ref" {
                    if let Some(JSXAttributeValue::ExpressionContainer(container)) = &attr.value {
                        if let Some(expr) = container.expression.as_expression() {
                            let ref_expr = peel_wrapped_expression(expr);
                            let ref_key = PropertyKey::StaticIdentifier(
                                ast.alloc_identifier_name(span, "ref"),
                            );

                            if matches!(
                                ref_expr,
                                Expression::ArrowFunctionExpression(_)
                                    | Expression::FunctionExpression(_)
                            ) || !is_writable_ref_target(ref_expr, ctx)
                            {
                                running_props.push(ast.object_property_kind_object_property(
                                    span,
                                    PropertyKind::Init,
                                    ref_key,
                                    context.clone_expr(ref_expr),
                                    false,
                                    false,
                                    false,
                                ));
                            } else {
                                let ref_param = ast.binding_pattern_binding_identifier(
                                    span,
                                    ast.allocator.alloc_str("r$"),
                                );
                                let params = ast.alloc_formal_parameters(
                                    span,
                                    FormalParameterKind::FormalParameter,
                                    ast.vec1(ast.plain_formal_parameter(span, ref_param)),
                                    NONE,
                                );

                                let ref_uid = context.generate_uid("ref$");
                                let mut body_stmts = ast.vec_with_capacity(2);
                                let var_decl = {
                                    let declarator = ast.variable_declarator(
                                        span,
                                        VariableDeclarationKind::Var,
                                        ast.binding_pattern_binding_identifier(
                                            span,
                                            ast.allocator.alloc_str(&ref_uid),
                                        ),
                                        NONE,
                                        Some(context.clone_expr(ref_expr)),
                                        false,
                                    );
                                    Statement::VariableDeclaration(ast.alloc_variable_declaration(
                                        span,
                                        VariableDeclarationKind::Var,
                                        ast.vec1(declarator),
                                        false,
                                    ))
                                };
                                body_stmts.push(var_decl);

                                let ref_ident = ast
                                    .expression_identifier(span, ast.allocator.alloc_str(&ref_uid));
                                let r_ident = ast.expression_identifier(span, "r$");
                                let typeof_ref = ast.expression_unary(
                                    span,
                                    UnaryOperator::Typeof,
                                    ref_ident.clone_in(ast.allocator),
                                );
                                let function_str = ast.expression_string_literal(
                                    span,
                                    ast.allocator.alloc_str("function"),
                                    None,
                                );
                                let test = ast.expression_binary(
                                    span,
                                    typeof_ref,
                                    BinaryOperator::StrictEquality,
                                    function_str,
                                );

                                let mut call_args = ast.vec_with_capacity(1);
                                call_args.push(Argument::from(r_ident.clone_in(ast.allocator)));
                                let call = ast.expression_call(
                                    span,
                                    ref_ident.clone_in(ast.allocator),
                                    None::<oxc_ast::ast::TSTypeParameterInstantiation<'a>>,
                                    call_args,
                                    false,
                                );

                                let ref_action = if let Some(target) =
                                    expression_to_assignment_target(context.clone_expr(ref_expr))
                                {
                                    ast.expression_conditional(
                                        span,
                                        test,
                                        call,
                                        ast.expression_assignment(
                                            span,
                                            AssignmentOperator::Assign,
                                            target,
                                            r_ident,
                                        ),
                                    )
                                } else {
                                    ast.expression_logical(span, test, LogicalOperator::And, call)
                                };

                                body_stmts.push(Statement::ExpressionStatement(
                                    ast.alloc_expression_statement(span, ref_action),
                                ));

                                let body = ast.alloc_function_body(span, ast.vec(), body_stmts);
                                let func = ast.expression_function(
                                    span,
                                    FunctionType::FunctionExpression,
                                    None,
                                    false,
                                    false,
                                    false,
                                    NONE,
                                    NONE,
                                    params,
                                    NONE,
                                    Some(body),
                                );

                                running_props.push(ast.object_property_kind_object_property(
                                    span,
                                    PropertyKind::Init,
                                    ref_key,
                                    func,
                                    true,
                                    false,
                                    false,
                                ));
                            }
                        }
                    }
                    continue;
                }

                let (key, key_is_identifier) = make_prop_key(ast, attr.span, &raw_key);

                match &attr.value {
                    Some(JSXAttributeValue::StringLiteral(lit)) => {
                        let decoded = common::expression::decode_html_entities(&lit.value);
                        running_props.push(ast.object_property_kind_object_property(
                            span,
                            PropertyKind::Init,
                            key,
                            ast.expression_string_literal(
                                span,
                                ast.allocator.alloc_str(&decoded),
                                None,
                            ),
                            false,
                            false,
                            false,
                        ));
                    }
                    Some(JSXAttributeValue::ExpressionContainer(container)) => {
                        if let Some(expr) = container.expression.as_expression() {
                            let has_static_marker = context
                                .has_static_marker_comment(container.span, options.static_marker);

                            if !has_static_marker && is_dynamic(expr) {
                                let getter_fn = if let Some(iife_getter) =
                                    getter_from_zero_arg_arrow_iife(expr, context, options)
                                {
                                    iife_getter
                                } else {
                                    let mut getter_value = context.clone_expr(expr);
                                    if options.wrap_conditionals
                                        && memo_wrapper_enabled(options)
                                        && is_condition_expression(&getter_value)
                                    {
                                        getter_value =
                                            transform_condition_inline_expr(getter_value, context);
                                    }
                                    getter_return_expr(ast, span, getter_value)
                                };

                                running_props.push(ast.object_property_kind_object_property(
                                    span,
                                    PropertyKind::Get,
                                    key,
                                    getter_fn,
                                    false,
                                    false,
                                    !key_is_identifier,
                                ));
                            } else {
                                let static_value = if has_static_marker {
                                    context.clone_expr_without_trivia(expr)
                                } else {
                                    context.clone_expr(expr)
                                };
                                running_props.push(ast.object_property_kind_object_property(
                                    span,
                                    PropertyKind::Init,
                                    key,
                                    static_value,
                                    false,
                                    false,
                                    false,
                                ));
                            }
                        }
                    }
                    None => {
                        running_props.push(ast.object_property_kind_object_property(
                            span,
                            PropertyKind::Init,
                            key,
                            ast.expression_boolean_literal(span, true),
                            false,
                            false,
                            false,
                        ));
                    }
                    _ => {}
                }
            }
        }
    }

    if !element.children.is_empty() {
        if let Some((children, dynamic_children)) =
            get_children_expr_transformed(element, context, options, transform_child)
        {
            let (key, key_is_identifier) = make_prop_key(ast, span, "children");
            if dynamic_children {
                let getter_fn = if let Some(iife_getter) =
                    getter_from_zero_arg_arrow_iife(&children, context, options)
                {
                    iife_getter
                } else {
                    let mut getter_children = children;
                    if options.wrap_conditionals
                        && memo_wrapper_enabled(options)
                        && is_condition_expression(&getter_children)
                    {
                        getter_children = transform_condition_inline_expr(getter_children, context);
                    }
                    getter_return_expr(ast, span, getter_children)
                };

                running_props.push(ast.object_property_kind_object_property(
                    span,
                    PropertyKind::Get,
                    key,
                    getter_fn,
                    false,
                    false,
                    !key_is_identifier,
                ));
            } else {
                running_props.push(ast.object_property_kind_object_property(
                    span,
                    PropertyKind::Init,
                    key,
                    children,
                    false,
                    false,
                    false,
                ));
            }
        }
    }

    flush_running_props(&mut props_parts, &mut running_props);

    if props_parts.is_empty() {
        return ast.expression_object(span, ast.vec());
    }

    if props_parts.len() > 1 || dynamic_spread {
        context.register_helper("mergeProps");
        let callee = helper_ident_expr(ast, span, "mergeProps");
        let mut args = ast.vec_with_capacity(props_parts.len());
        for part in props_parts {
            args.push(Argument::from(part));
        }
        ast.expression_call(
            span,
            callee,
            None::<oxc_ast::ast::TSTypeParameterInstantiation<'a>>,
            args,
            false,
        )
    } else {
        props_parts
            .pop()
            .unwrap_or_else(|| ast.expression_object(span, ast.vec()))
    }
}
