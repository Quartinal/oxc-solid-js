//! ImportDeclaration scanning and registration.

use oxc_ast::ast::{ImportDeclaration, ImportDeclarationSpecifier};

use crate::checks::get_import_specifier_name;
use crate::types::{ImportDefinition, ImportIdentifierSpecifier, StateContext};

/// Scans an ImportDeclaration and registers matching bindings in state.
///
/// For each specifier in `state.specifiers` whose source matches the import's source,
/// registers the local binding name in either `identifier_registrations` or
/// `namespace_registrations`.
pub fn register_import_specifiers(
    state: &mut StateContext<'_>,
    import_decl: &ImportDeclaration<'_>,
) {
    let source_value = import_decl.source.value.as_str();

    // Collect matching definitions first to avoid borrow issues
    let matching: Vec<ImportIdentifierSpecifier> = state
        .specifiers
        .iter()
        .filter(|spec| spec.definition.source() == source_value)
        .cloned()
        .collect();

    if matching.is_empty() {
        return;
    }

    let specifiers = match import_decl.specifiers.as_ref() {
        Some(s) => s.as_slice(),
        None => return,
    };

    for spec in specifiers {
        for id in &matching {
            register_single_specifier(state, id, spec);
        }
    }
}

fn register_single_specifier(
    state: &mut StateContext<'_>,
    id: &ImportIdentifierSpecifier,
    specifier: &ImportDeclarationSpecifier<'_>,
) {
    match specifier {
        ImportDeclarationSpecifier::ImportDefaultSpecifier(default_spec) => {
            // Only register if the definition is a default import
            if matches!(id.definition, ImportDefinition::Default { .. }) {
                state
                    .identifier_registrations
                    .insert(default_spec.local.name.to_string(), id.clone());
            }
        }
        ImportDeclarationSpecifier::ImportSpecifier(named_spec) => {
            // Skip type-only imports
            if named_spec.import_kind.is_type() {
                return;
            }
            let imported_name = get_import_specifier_name(named_spec);
            let should_register = match &id.definition {
                ImportDefinition::Named { name, .. } => imported_name == *name,
                ImportDefinition::Default { .. } => imported_name == "default",
            };
            if should_register {
                state
                    .identifier_registrations
                    .insert(named_spec.local.name.to_string(), id.clone());
            }
        }
        ImportDeclarationSpecifier::ImportNamespaceSpecifier(ns_spec) => {
            let local_name = ns_spec.local.name.to_string();
            state
                .namespace_registrations
                .entry(local_name)
                .or_default()
                .push(id.clone());
        }
    }
}
