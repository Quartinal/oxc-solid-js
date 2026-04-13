//! Collect identifiers referenced from outside component scope.
//!
//! Used for granular HMR — collects "foreign bindings" (identifiers referenced
//! inside a component but bound outside it) as dependency parameters.
//!
//! This is a simplified initial implementation that collects all referenced
//! identifiers from an expression tree. The full foreign binding analysis
//! (filtering against component-local scope) will be performed by the main
//! transform, which has access to `TraverseCtx::scoping()`.

use std::collections::HashSet;

use oxc_ast::ast::{
    Argument, ArrayExpressionElement, Expression, ObjectPropertyKind, PropertyKey,
    SimpleAssignmentTarget, Statement,
};

/// Collects all identifier references within an expression.
///
/// Walks the expression tree and gathers every `IdentifierReference` name.
/// The caller is responsible for filtering out locally-bound identifiers
/// (those defined within the component scope) to derive the true foreign
/// binding set.
pub fn collect_referenced_identifiers(expr: &Expression<'_>) -> HashSet<String> {
    let mut identifiers = HashSet::new();
    collect_from_expression(expr, &mut identifiers);
    identifiers
}

fn collect_from_expression<'a>(expr: &Expression<'a>, ids: &mut HashSet<String>) {
    match expr {
        Expression::Identifier(ident) => {
            ids.insert(ident.name.to_string());
        }
        Expression::CallExpression(call) => {
            collect_from_expression(&call.callee, ids);
            for arg in &call.arguments {
                collect_from_argument(arg, ids);
            }
        }
        Expression::StaticMemberExpression(member) => {
            collect_from_expression(&member.object, ids);
            // property is an IdentifierName, not a reference — skip it
        }
        Expression::ComputedMemberExpression(member) => {
            collect_from_expression(&member.object, ids);
            collect_from_expression(&member.expression, ids);
        }
        Expression::BinaryExpression(bin) => {
            collect_from_expression(&bin.left, ids);
            collect_from_expression(&bin.right, ids);
        }
        Expression::LogicalExpression(log) => {
            collect_from_expression(&log.left, ids);
            collect_from_expression(&log.right, ids);
        }
        Expression::UnaryExpression(un) => {
            collect_from_expression(&un.argument, ids);
        }
        Expression::ConditionalExpression(cond) => {
            collect_from_expression(&cond.test, ids);
            collect_from_expression(&cond.consequent, ids);
            collect_from_expression(&cond.alternate, ids);
        }
        Expression::AssignmentExpression(assign) => {
            collect_from_expression(&assign.right, ids);
        }
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                collect_from_expression(expr, ids);
            }
        }
        Expression::TemplateLiteral(tpl) => {
            for expr in &tpl.expressions {
                collect_from_expression(expr, ids);
            }
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                collect_from_array_element(elem, ids);
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                collect_from_object_property(prop, ids);
            }
        }
        Expression::ArrowFunctionExpression(arrow) => {
            for stmt in &arrow.body.statements {
                collect_from_statement(stmt, ids);
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_from_expression(&paren.expression, ids);
        }
        Expression::TaggedTemplateExpression(tagged) => {
            collect_from_expression(&tagged.tag, ids);
            for expr in &tagged.quasi.expressions {
                collect_from_expression(expr, ids);
            }
        }
        Expression::AwaitExpression(aw) => {
            collect_from_expression(&aw.argument, ids);
        }
        Expression::YieldExpression(y) => {
            if let Some(arg) = &y.argument {
                collect_from_expression(arg, ids);
            }
        }
        Expression::UpdateExpression(up) => {
            collect_from_simple_assignment_target(&up.argument, ids);
        }
        Expression::NewExpression(new) => {
            collect_from_expression(&new.callee, ids);
            for arg in &new.arguments {
                collect_from_argument(arg, ids);
            }
        }
        Expression::JSXElement(el) => {
            // Collect from JSX member expressions (e.g., <Foo.Bar />)
            if let oxc_ast::ast::JSXElementName::MemberExpression(member) = &el.opening_element.name
            {
                collect_from_jsx_member(member, ids);
            }
            if let oxc_ast::ast::JSXElementName::Identifier(ident) = &el.opening_element.name {
                // Only uppercase names are component references
                if ident.name.chars().next().is_some_and(|c| c.is_uppercase()) {
                    ids.insert(ident.name.to_string());
                }
            }
        }
        // Literals and other non-referencing expressions
        _ => {}
    }
}

fn collect_from_argument<'a>(arg: &Argument<'a>, ids: &mut HashSet<String>) {
    match arg {
        Argument::SpreadElement(spread) => {
            collect_from_expression(&spread.argument, ids);
        }
        _ => {
            if let Some(expr) = arg.as_expression() {
                collect_from_expression(expr, ids);
            }
        }
    }
}

fn collect_from_array_element<'a>(elem: &ArrayExpressionElement<'a>, ids: &mut HashSet<String>) {
    match elem {
        ArrayExpressionElement::SpreadElement(spread) => {
            collect_from_expression(&spread.argument, ids);
        }
        ArrayExpressionElement::Elision(_) => {}
        _ => {
            if let Some(expr) = elem.as_expression() {
                collect_from_expression(expr, ids);
            }
        }
    }
}

fn collect_from_object_property<'a>(prop: &ObjectPropertyKind<'a>, ids: &mut HashSet<String>) {
    match prop {
        ObjectPropertyKind::ObjectProperty(p) => {
            if p.computed {
                collect_from_property_key(&p.key, ids);
            }
            collect_from_expression(&p.value, ids);
        }
        ObjectPropertyKind::SpreadProperty(spread) => {
            collect_from_expression(&spread.argument, ids);
        }
    }
}

fn collect_from_property_key<'a>(key: &PropertyKey<'a>, ids: &mut HashSet<String>) {
    if let Some(expr) = key.as_expression() {
        collect_from_expression(expr, ids);
    }
}

fn collect_from_simple_assignment_target<'a>(
    target: &SimpleAssignmentTarget<'a>,
    ids: &mut HashSet<String>,
) {
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) => {
            ids.insert(ident.name.to_string());
        }
        SimpleAssignmentTarget::StaticMemberExpression(member) => {
            collect_from_expression(&member.object, ids);
        }
        SimpleAssignmentTarget::ComputedMemberExpression(member) => {
            collect_from_expression(&member.object, ids);
            collect_from_expression(&member.expression, ids);
        }
        _ => {}
    }
}

fn collect_from_statement<'a>(stmt: &Statement<'a>, ids: &mut HashSet<String>) {
    match stmt {
        Statement::ExpressionStatement(expr_stmt) => {
            collect_from_expression(&expr_stmt.expression, ids);
        }
        Statement::ReturnStatement(ret) => {
            if let Some(arg) = &ret.argument {
                collect_from_expression(arg, ids);
            }
        }
        Statement::BlockStatement(block) => {
            for s in &block.body {
                collect_from_statement(s, ids);
            }
        }
        _ => {}
    }
}

fn collect_from_jsx_member<'a>(
    member: &oxc_ast::ast::JSXMemberExpression<'a>,
    ids: &mut HashSet<String>,
) {
    match &member.object {
        oxc_ast::ast::JSXMemberExpressionObject::MemberExpression(nested) => {
            collect_from_jsx_member(nested, ids);
        }
        oxc_ast::ast::JSXMemberExpressionObject::IdentifierReference(ident) => {
            ids.insert(ident.name.to_string());
        }
        oxc_ast::ast::JSXMemberExpressionObject::ThisExpression(_) => {}
    }
}
