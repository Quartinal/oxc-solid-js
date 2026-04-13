//! Call expression callee validation.

use oxc_ast::ast::{Expression, MemberExpression};

use crate::types::{
    ImportDefinition, ImportIdentifierSpecifier, ImportIdentifierType, StateContext,
};
use crate::unwrap::unwrap_expression;

/// Checks if a call expression's callee matches a registered import of the given type.
///
/// Handles:
/// - Direct identifier calls: `render(...)` → lookup in identifier_registrations
/// - Namespace member calls: `S.render(...)` → lookup in namespace_registrations
/// - Unwraps parenthesized/TS type expressions before checking.
pub fn is_valid_callee(
    state: &StateContext<'_>,
    callee: &Expression<'_>,
    target: ImportIdentifierType,
) -> bool {
    let unwrapped = unwrap_expression(callee);

    // Check direct identifier
    if let Expression::Identifier(ident) = unwrapped {
        return is_identifier_valid_callee(state, &ident.name, target);
    }

    // Check member expression (namespace access)
    if let Some(member) = unwrapped.as_member_expression() {
        return is_member_expression_valid_callee(state, member, target);
    }

    false
}

fn is_identifier_valid_callee(
    state: &StateContext<'_>,
    name: &str,
    target: ImportIdentifierType,
) -> bool {
    state
        .identifier_registrations
        .get(name)
        .is_some_and(|reg| reg.import_type == target)
}

fn is_member_expression_valid_callee(
    state: &StateContext<'_>,
    member: &MemberExpression<'_>,
    target: ImportIdentifierType,
) -> bool {
    // Must be a static member expression (a.b, not a[b])
    let MemberExpression::StaticMemberExpression(static_member) = member else {
        return false;
    };

    let prop_name = static_member.property.name.as_str();

    // Unwrap the object to find the namespace identifier
    let object = unwrap_expression(&static_member.object);
    let Expression::Identifier(obj_ident) = object else {
        return false;
    };

    let Some(registrations) = state.namespace_registrations.get(obj_ident.name.as_str()) else {
        return false;
    };

    is_property_valid_callee(registrations, target, prop_name)
}

fn is_property_valid_callee(
    registrations: &[ImportIdentifierSpecifier],
    target: ImportIdentifierType,
    prop_name: &str,
) -> bool {
    registrations.iter().any(|reg| {
        if reg.import_type != target {
            return false;
        }
        match &reg.definition {
            ImportDefinition::Named { name, .. } => *name == prop_name,
            ImportDefinition::Default { .. } => prop_name == "default",
        }
    })
}
