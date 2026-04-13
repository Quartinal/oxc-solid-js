//! Walk up to nearest enclosing statement.
//!
//! In Babel's solid-refresh, `getStatementPath` walks up `parentPath` until it
//! finds a Statement node. In OXC we don't have mutable paths, so during
//! traversal we inspect the ancestor chain instead.

use oxc_traverse::{ancestor::Ancestor, TraverseCtx};

/// Returns `true` when the current traversal position is directly inside a
/// statement‑level context (i.e. the nearest scope‑relevant ancestor is a
/// statement list such as `Program.body`, `FunctionBody.statements`, or
/// `BlockStatement.body`).
///
/// This is the OXC equivalent of Babel's `getStatementPath` — callers use it
/// to decide whether the current node can be treated as (or hoisted to) a
/// top‑level statement position.
pub fn is_in_statement_position(ctx: &TraverseCtx<'_, ()>) -> bool {
    for ancestor in ctx.ancestors() {
        match ancestor {
            // Direct children of these containers are statements.
            Ancestor::ProgramBody(_)
            | Ancestor::FunctionBodyStatements(_)
            | Ancestor::BlockStatementBody(_) => return true,

            // Export wrappers don't change the statement‑level nature.
            Ancestor::ExportDefaultDeclarationDeclaration(_)
            | Ancestor::ExportNamedDeclarationDeclaration(_) => continue,

            // Everything else means we're inside an expression or other
            // non‑statement context.
            _ => return false,
        }
    }
    false
}
