//! Check functions for JSX nodes
//! Ported from dom-expressions/src/shared/utils.js

use std::borrow::Cow;

use oxc_ast::ast::{
    Expression, JSXAttribute, JSXAttributeItem, JSXAttributeName, JSXAttributeValue, JSXChild,
    JSXElement, JSXElementName, JSXFragment, JSXMemberExpression, JSXMemberExpressionObject,
    Statement,
};
use oxc_syntax::operator::BinaryOperator;

use crate::constants::{BUILT_INS, SVG_ELEMENTS};
use crate::expression::expr_to_string;

/// Check if a tag name represents a component.
///
/// Mirrors babel-plugin-jsx-dom-expressions `isComponent`:
/// - first char is uppercase, or
/// - tag contains a member-access dot, or
/// - first char is not an ASCII letter (e.g. `_garbage`).
pub fn is_component(tag: &str) -> bool {
    if tag.is_empty() {
        return false;
    }

    let first_char = tag.chars().next().unwrap();
    first_char.is_ascii_uppercase() || tag.contains('.') || !first_char.is_ascii_alphabetic()
}

/// Check if this is a built-in Solid component (For, Show, etc.)
pub fn is_built_in(tag: &str) -> bool {
    BUILT_INS.contains(tag)
}

/// Check if this is an SVG element
pub fn is_svg_element(tag: &str) -> bool {
    SVG_ELEMENTS.contains(tag)
}

/// Get the tag name from a JSX element
pub fn get_tag_name<'e>(element: &'e JSXElement<'_>) -> Cow<'e, str> {
    get_jsx_element_name(&element.opening_element.name)
}

/// Get the name from a JSXElementName
fn get_jsx_element_name<'e>(name: &'e JSXElementName<'_>) -> Cow<'e, str> {
    match name {
        JSXElementName::Identifier(id) => Cow::Borrowed(id.name.as_str()),
        JSXElementName::IdentifierReference(id) => Cow::Borrowed(id.name.as_str()),
        JSXElementName::NamespacedName(ns) => {
            Cow::Owned(format!("{}:{}", ns.namespace.name, ns.name.name))
        }
        JSXElementName::MemberExpression(member) => Cow::Owned(get_member_expression_name(member)),
        JSXElementName::ThisExpression(_) => Cow::Borrowed("this"),
    }
}

/// Get the name from a JSX member expression (e.g., Foo.Bar.Baz)
fn get_member_expression_name(member: &JSXMemberExpression) -> String {
    let object = match &member.object {
        JSXMemberExpressionObject::IdentifierReference(id) => id.name.to_string(),
        JSXMemberExpressionObject::MemberExpression(m) => get_member_expression_name(m),
        JSXMemberExpressionObject::ThisExpression(_) => "this".to_string(),
    };
    format!("{}.{}", object, member.property.name)
}

fn is_effect_helper_name(name: &str) -> bool {
    name == "effect" || name.starts_with("_$effect")
}

fn expression_contains_effect_call(expr: &Expression) -> bool {
    match expr {
        Expression::CallExpression(call) => {
            let callee_has_effect = matches!(
                &call.callee,
                Expression::Identifier(ident) if is_effect_helper_name(ident.name.as_str())
            );

            callee_has_effect
                || expression_contains_effect_call(&call.callee)
                || call.arguments.iter().any(|arg| {
                    arg.as_expression()
                        .is_some_and(expression_contains_effect_call)
                })
        }
        Expression::ArrowFunctionExpression(arrow) => arrow
            .body
            .statements
            .iter()
            .any(statement_contains_effect_call),
        Expression::FunctionExpression(function) => function
            .body
            .as_ref()
            .is_some_and(|body| body.statements.iter().any(statement_contains_effect_call)),
        Expression::ConditionalExpression(cond) => {
            expression_contains_effect_call(&cond.test)
                || expression_contains_effect_call(&cond.consequent)
                || expression_contains_effect_call(&cond.alternate)
        }
        Expression::LogicalExpression(logical) => {
            expression_contains_effect_call(&logical.left)
                || expression_contains_effect_call(&logical.right)
        }
        Expression::BinaryExpression(binary) => {
            expression_contains_effect_call(&binary.left)
                || expression_contains_effect_call(&binary.right)
        }
        Expression::UnaryExpression(unary) => expression_contains_effect_call(&unary.argument),
        Expression::ParenthesizedExpression(paren) => {
            expression_contains_effect_call(&paren.expression)
        }
        Expression::TSAsExpression(ts) => expression_contains_effect_call(&ts.expression),
        Expression::TSSatisfiesExpression(ts) => expression_contains_effect_call(&ts.expression),
        Expression::TSNonNullExpression(ts) => expression_contains_effect_call(&ts.expression),
        Expression::TSTypeAssertion(ts) => expression_contains_effect_call(&ts.expression),
        Expression::ObjectExpression(object) => object.properties.iter().any(|prop| match prop {
            oxc_ast::ast::ObjectPropertyKind::ObjectProperty(prop) => {
                let key_has_effect = prop.computed
                    && prop
                        .key
                        .as_expression()
                        .is_some_and(expression_contains_effect_call);
                key_has_effect || expression_contains_effect_call(&prop.value)
            }
            oxc_ast::ast::ObjectPropertyKind::SpreadProperty(spread) => {
                expression_contains_effect_call(&spread.argument)
            }
        }),
        Expression::ArrayExpression(array) => array.elements.iter().any(|element| match element {
            oxc_ast::ast::ArrayExpressionElement::SpreadElement(spread) => {
                expression_contains_effect_call(&spread.argument)
            }
            oxc_ast::ast::ArrayExpressionElement::Elision(_) => false,
            _ => element
                .as_expression()
                .is_some_and(expression_contains_effect_call),
        }),
        _ => false,
    }
}

fn statement_contains_effect_call(statement: &Statement) -> bool {
    match statement {
        Statement::ExpressionStatement(expr_stmt) => {
            expression_contains_effect_call(&expr_stmt.expression)
        }
        Statement::ReturnStatement(return_stmt) => return_stmt
            .argument
            .as_ref()
            .is_some_and(expression_contains_effect_call),
        Statement::VariableDeclaration(var_decl) => var_decl.declarations.iter().any(|decl| {
            decl.init
                .as_ref()
                .is_some_and(expression_contains_effect_call)
        }),
        Statement::BlockStatement(block) => block.body.iter().any(statement_contains_effect_call),
        Statement::IfStatement(if_stmt) => {
            expression_contains_effect_call(&if_stmt.test)
                || statement_contains_effect_call(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|alt| statement_contains_effect_call(alt))
        }
        _ => false,
    }
}

fn iife_contains_effect_call(expr: &Expression) -> bool {
    let Expression::CallExpression(call) = expr else {
        return false;
    };

    if !call.arguments.is_empty() {
        return false;
    }

    match &call.callee {
        Expression::ArrowFunctionExpression(arrow) => arrow
            .body
            .statements
            .iter()
            .any(statement_contains_effect_call),
        Expression::FunctionExpression(function) => function
            .body
            .as_ref()
            .is_some_and(|body| body.statements.iter().any(statement_contains_effect_call)),
        _ => false,
    }
}

fn jsx_child_is_dynamic(child: &JSXChild) -> bool {
    match child {
        JSXChild::Text(_) => false,
        JSXChild::Element(element) => is_jsx_element_dynamic(element),
        JSXChild::Fragment(fragment) => is_jsx_fragment_dynamic(fragment),
        JSXChild::ExpressionContainer(container) => {
            container.expression.as_expression().is_some_and(is_dynamic)
        }
        JSXChild::Spread(spread) => is_dynamic(&spread.expression),
    }
}

fn is_jsx_fragment_dynamic(fragment: &JSXFragment) -> bool {
    fragment.children.iter().any(jsx_child_is_dynamic)
}

fn is_jsx_element_dynamic(element: &JSXElement) -> bool {
    // Component JSX always lowers to runtime calls (`createComponent`) and should
    // participate in dynamic expression heuristics even when props/children are static.
    if is_component(&get_tag_name(element)) {
        return true;
    }

    let has_dynamic_attribute = element
        .opening_element
        .attributes
        .iter()
        .any(|attr| match attr {
            JSXAttributeItem::SpreadAttribute(_) => true,
            JSXAttributeItem::Attribute(attr) => attr
                .value
                .as_ref()
                .and_then(|value| match value {
                    JSXAttributeValue::ExpressionContainer(container) => {
                        container.expression.as_expression()
                    }
                    _ => None,
                })
                .is_some_and(is_dynamic),
        });

    has_dynamic_attribute || element.children.iter().any(jsx_child_is_dynamic)
}

/// Check if an expression is dynamic (needs effect wrapping)
/// This is a simplified version - full implementation would need scope analysis
pub fn is_dynamic(expr: &Expression) -> bool {
    match expr {
        // Literals are static
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_) => false,

        // Template literals with no expressions are static
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => false,

        // Function calls are dynamic, except transformed JSX IIFEs that don't
        // contain reactive effects in their body.
        Expression::CallExpression(call) => {
            if call.arguments.is_empty()
                && matches!(
                    &call.callee,
                    Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
                )
            {
                iife_contains_effect_call(expr)
            } else {
                true
            }
        }

        // Member expressions accessing reactive values are dynamic
        Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) => true,

        // Match babel-plugin-jsx-dom-expressions heuristic: bare identifiers are static.
        // Dynamic wrappers are generally reserved for call/member expressions.
        Expression::Identifier(_) => false,

        // JSX expressions are only dynamic when their attributes/children are dynamic.
        Expression::JSXElement(element) => is_jsx_element_dynamic(element),
        Expression::JSXFragment(fragment) => is_jsx_fragment_dynamic(fragment),

        // Conditional/logical expressions depend on their operands.
        Expression::ConditionalExpression(cond) => {
            is_dynamic(&cond.test) || is_dynamic(&cond.consequent) || is_dynamic(&cond.alternate)
        }
        Expression::LogicalExpression(logical) => {
            is_dynamic(&logical.left) || is_dynamic(&logical.right)
        }

        // Parentheses/TS wrappers should not affect dynamic classification.
        Expression::ParenthesizedExpression(paren) => is_dynamic(&paren.expression),
        Expression::TSAsExpression(ts) => is_dynamic(&ts.expression),
        Expression::TSSatisfiesExpression(ts) => is_dynamic(&ts.expression),
        Expression::TSNonNullExpression(ts) => is_dynamic(&ts.expression),
        Expression::TSTypeAssertion(ts) => is_dynamic(&ts.expression),

        // Binary/unary with dynamic operands.
        // Match Babel's checkMember behavior for `in` operator, which is always dynamic.
        Expression::BinaryExpression(b) => {
            if b.operator == BinaryOperator::In {
                true
            } else {
                is_dynamic(&b.left) || is_dynamic(&b.right)
            }
        }
        Expression::UnaryExpression(u) => is_dynamic(&u.argument),

        // Arrow functions themselves are static (the reference)
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => false,

        // Object/array literals depend on their contents
        Expression::ObjectExpression(o) => o.properties.iter().any(|p| match p {
            oxc_ast::ast::ObjectPropertyKind::ObjectProperty(prop) => {
                let key_dynamic = prop.computed
                    && match &prop.key {
                        oxc_ast::ast::PropertyKey::StaticIdentifier(_)
                        | oxc_ast::ast::PropertyKey::PrivateIdentifier(_) => false,
                        _ => prop.key.as_expression().is_some_and(is_dynamic),
                    };
                key_dynamic || is_dynamic(&prop.value)
            }
            // Babel parity: object/array spread is treated as dynamic in member checks.
            oxc_ast::ast::ObjectPropertyKind::SpreadProperty(_) => true,
        }),
        Expression::ArrayExpression(a) => a.elements.iter().any(|el| match el {
            oxc_ast::ast::ArrayExpressionElement::SpreadElement(s) => is_dynamic(&s.argument),
            oxc_ast::ast::ArrayExpressionElement::Elision(_) => false,
            _ => {
                if let Some(expr) = el.as_expression() {
                    is_dynamic(expr)
                } else {
                    false
                }
            }
        }),

        // Default to dynamic for safety
        _ => true,
    }
}

/// Find a JSX attribute by name on an element.
///
/// Returns the attribute if found, allowing access to both the name and value.
pub fn find_prop<'a>(element: &'a JSXElement<'a>, name: &str) -> Option<&'a JSXAttribute<'a>> {
    for attr in &element.opening_element.attributes {
        if let JSXAttributeItem::Attribute(attr) = attr {
            if let JSXAttributeName::Identifier(id) = &attr.name {
                if id.name == name {
                    return Some(attr);
                }
            }
        }
    }
    None
}

/// Find a JSX attribute by name and return its value as a string.
///
/// Handles expression containers, string literals, and boolean attributes (no value = true).
pub fn find_prop_value(element: &JSXElement<'_>, name: &str) -> Option<String> {
    find_prop(element, name).and_then(|attr| get_attr_value(attr))
}

/// Get the value of a JSX attribute as a string.
///
/// - Expression containers: returns the expression as a string
/// - String literals: returns the quoted string
/// - No value (boolean): returns "true"
pub fn get_attr_value(attr: &JSXAttribute<'_>) -> Option<String> {
    match &attr.value {
        Some(JSXAttributeValue::ExpressionContainer(container)) => container
            .expression
            .as_expression()
            .map(|e| expr_to_string(e)),
        Some(JSXAttributeValue::StringLiteral(lit)) => Some(format!("\"{}\"", lit.value)),
        None => Some("true".to_string()),
        _ => None,
    }
}

/// Get the full name of a JSX attribute (including namespace if present).
///
/// - `id` -> "id"
/// - `on:click` -> "on:click"
pub fn get_attr_name<'e>(name: &'e JSXAttributeName<'_>) -> Cow<'e, str> {
    match name {
        JSXAttributeName::Identifier(id) => Cow::Borrowed(id.name.as_str()),
        JSXAttributeName::NamespacedName(ns) => {
            Cow::Owned(format!("{}:{}", ns.namespace.name, ns.name.name))
        }
    }
}

/// Check if a JSX attribute name is namespaced (e.g., `on:click`, `use:directive`).
pub fn is_namespaced_attr(name: &JSXAttributeName) -> bool {
    matches!(name, JSXAttributeName::NamespacedName(_))
}
