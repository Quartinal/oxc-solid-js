//! Lazy deduped import insertion.
//!
//! Collects import requests during traversal. Actual ImportDeclaration nodes
//! are created and prepended to Program.body in exit_program.

use crate::types::{ImportDefinition, StateContext};

/// Gets or creates a unique local identifier for an import.
///
/// On first request for a given import, generates a uid and records it.
/// Returns the local identifier name (e.g., "_$$registry").
///
/// The actual ImportDeclaration insertion happens later in exit_program.
pub fn get_import_identifier(
    state: &mut StateContext<'_>,
    registration: &ImportDefinition,
) -> String {
    let name = registration.name();
    let target = format!("{}[{}]", registration.source(), name);

    if let Some(existing) = state.imports.get(&target) {
        return existing.clone();
    }

    let uid = state.generate_uid(name);
    state.imports.insert(target, uid.clone());
    uid
}
