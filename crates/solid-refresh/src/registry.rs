//! HMR registry creation ($$registry + $$refresh calls).
//!
//! On first call, generates a unique identifier for the registry variable,
//! records the `$$registry` and `$$refresh` imports, and stores the registry
//! identifier name in state. On subsequent calls, returns the cached identifier.
//!
//! The actual AST nodes (`const _REGISTRY = $$registry()` and the
//! `if (import.meta.hot) { $$refresh(...) }` block) are emitted in `exit_program`.

use crate::constants::{IMPORT_REFRESH, IMPORT_REGISTRY};
use crate::import_identifier::get_import_identifier;
use crate::types::StateContext;

const REGISTRY_KEY: &str = "REGISTRY";

/// Ensures the registry is created and returns the local registry identifier name.
///
/// On first call: generates a uid, records the `$$registry` import, and stores
/// the registry identifier. On subsequent calls: returns the cached identifier.
///
/// The actual AST nodes (const declaration + if block) are emitted in `exit_program`.
pub fn create_registry(state: &mut StateContext<'_>) -> String {
    if let Some(existing) = state.imports.get(REGISTRY_KEY) {
        return existing.clone();
    }

    // Ensure $$registry and $$refresh imports are requested and store names for exit_program
    state.registry_import_name = Some(get_import_identifier(state, &IMPORT_REGISTRY));
    state.refresh_import_name = Some(get_import_identifier(state, &IMPORT_REFRESH));

    // Generate the registry variable name
    let registry_uid = state.generate_uid(REGISTRY_KEY);
    state
        .imports
        .insert(REGISTRY_KEY.to_string(), registry_uid.clone());

    registry_uid
}
