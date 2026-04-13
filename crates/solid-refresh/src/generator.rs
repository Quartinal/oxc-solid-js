//! AST code generation for signature hashing.

use common::expr_to_string;
use oxc_ast::ast::Expression;
use xxhash_rust::xxh32::xxh32;

/// Serializes an expression to a code string using `oxc_codegen`.
pub fn generate_code(expr: &Expression<'_>) -> String {
    expr_to_string(expr)
}

/// Generates code from an expression and returns its xxHash32 signature as a hex string.
///
/// Equivalent to JS: `xxHash32(generateCode(node)).toString(16)`
pub fn create_signature_value(expr: &Expression<'_>) -> String {
    let code = generate_code(expr);
    format!("{:x}", xxh32(code.as_bytes(), 0))
}
