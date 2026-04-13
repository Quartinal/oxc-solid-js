//! Unique identifier generation.

use std::collections::HashSet;

/// Generates a unique identifier name that doesn't collide with any existing binding.
///
/// Produces names like `name_1`, `name_2`, etc.
/// Registers the result in `used_names` to prevent future collisions.
pub fn generate_unique_name(name: &str, used_names: &mut HashSet<String>) -> String {
    let mut i = 1u32;
    loop {
        let candidate = format!("{name}_{i}");
        if !used_names.contains(&candidate) {
            used_names.insert(candidate.clone());
            return candidate;
        }
        i += 1;
    }
}
