//! Check if a statement is at program scope.
//!
//! Upstream: `solid-refresh/src/babel/core/is-statement-top-level.ts`
//! Checks whether the nearest enclosing scope‑creating ancestor is the
//! `Program` node — i.e. the statement is at module / script top‑level.

use oxc_traverse::{ancestor::Ancestor, TraverseCtx};

/// Returns `true` if the current traversal position is a direct child of the
/// `Program` body, possibly wrapped in an export declaration.
///
/// Any intervening scope boundary (function body, block statement, etc.) causes
/// an early `false` return.
pub fn is_statement_top_level(ctx: &TraverseCtx<'_, ()>) -> bool {
    for ancestor in ctx.ancestors() {
        match ancestor {
            // Reached Program.body — this is top-level.
            Ancestor::ProgramBody(_) => return true,

            // Export wrappers are transparent for top-level detection.
            Ancestor::ExportDefaultDeclarationDeclaration(_)
            | Ancestor::ExportNamedDeclarationDeclaration(_) => continue,

            // Any other scope boundary means we're not top-level.
            _ => return false,
        }
    }
    false
}
