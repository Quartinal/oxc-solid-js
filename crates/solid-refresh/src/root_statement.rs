//! Walk up to program-level statement.

use oxc_ast::ast::Program;
use oxc_span::{GetSpan, Span};

/// Finds the index in `program.body` of the statement that contains the given span.
///
/// This is the OXC equivalent of Babel's `getRootStatementPath(path)`.
/// Returns the index of the enclosing top-level statement, or `program.body.len()`
/// if no enclosing statement is found.
pub fn find_root_statement_index(program: &Program<'_>, target_span: Span) -> usize {
    for (i, stmt) in program.body.iter().enumerate() {
        let span = stmt.span();
        if span.start <= target_span.start && target_span.start < span.end {
            return i;
        }
    }
    program.body.len()
}
