use oxc_ast::ast::ImportSpecifier;

/// Returns true if the name starts with an uppercase ASCII letter (PascalCase heuristic).
/// This is the check used to determine if a function is component-ish.
#[inline]
pub fn is_component_ish_name(name: &str) -> bool {
    name.as_bytes()
        .first()
        .is_some_and(|b| b.is_ascii_uppercase())
}

/// Extracts the imported name from an ImportSpecifier.
/// Handles both `Identifier` and `StringLiteral` imported names.
pub fn get_import_specifier_name<'a>(specifier: &'a ImportSpecifier<'a>) -> &'a str {
    specifier.imported.name().as_str()
}
