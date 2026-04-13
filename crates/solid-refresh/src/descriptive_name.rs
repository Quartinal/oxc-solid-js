//! Extract descriptive name from enclosing AST context.
//!
//! Upstream: `solid-refresh/src/babel/core/get-descriptive-name.ts`
//! Walks up the ancestor chain to find a human-readable name from the
//! enclosing function declaration, variable declarator, class method, or
//! object property.

use oxc_ast::ast::{BindingPattern, PropertyKey};
use oxc_traverse::{ancestor::Ancestor, TraverseCtx};

/// Extracts a descriptive name from a [`PropertyKey`].
///
/// Returns the identifier name for `StaticIdentifier` or `PrivateIdentifier`
/// keys, mirroring the upstream `Identifier` / `PrivateName` checks.
fn property_key_name<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(ident) => Some(ident.name.as_str()),
        PropertyKey::PrivateIdentifier(ident) => Some(ident.name.as_str()),
        _ => None,
    }
}

/// Walks up the ancestor chain to find a descriptive name for the current
/// traversal position.
///
/// Checks, in order of encounter:
/// - **`FunctionDeclaration` / `FunctionExpression`** — returns `function.id.name`
///   if the function is named.
/// - **`VariableDeclarator`** — returns the binding identifier name
///   (e.g. `const Foo = …` → `"Foo"`).
/// - **`MethodDefinition`** (class methods, including private) — returns the
///   method key name.
/// - **`ObjectProperty`** (object methods / shorthand) — returns the property
///   key name.
///
/// If no enclosing context provides a name, returns `default_name`.
pub fn get_descriptive_name(ctx: &TraverseCtx<'_, ()>, default_name: &str) -> String {
    for ancestor in ctx.ancestors() {
        match ancestor {
            // Function (declaration or expression): check for a named id.
            // FunctionBody means "the current node is the body field of a Function".
            Ancestor::FunctionBody(func) => {
                if let Some(id) = func.id() {
                    return id.name.to_string();
                }
            }
            // FunctionParams means "the current node is the params field of a Function".
            Ancestor::FunctionParams(func) => {
                if let Some(id) = func.id() {
                    return id.name.to_string();
                }
            }

            // VariableDeclarator: check if the binding id is a simple identifier.
            Ancestor::VariableDeclaratorInit(decl) => {
                if let BindingPattern::BindingIdentifier(ident) = decl.id() {
                    return ident.name.to_string();
                }
            }

            // Class method (including private methods): extract key name.
            Ancestor::MethodDefinitionValue(method) => {
                if let Some(name) = property_key_name(method.key()) {
                    return name.to_string();
                }
            }

            // Object property (covers Babel's ObjectMethod): extract key name.
            Ancestor::ObjectPropertyValue(prop) => {
                if let Some(name) = property_key_name(prop.key()) {
                    return name.to_string();
                }
            }

            _ => {}
        }
    }
    default_name.to_string()
}
