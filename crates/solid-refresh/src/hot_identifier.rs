//! Bundler-specific HMR expression builder.

use oxc_ast::{ast::Expression, AstBuilder};
use oxc_span::SPAN;

use crate::types::RuntimeType;

/// Builds the hot module expression for the given bundler type.
///
/// - Esm/Vite → `import.meta.hot`
/// - Webpack5/RspackEsm → `import.meta.webpackHot`
/// - Standard → `module.hot`
pub fn build_hot_identifier<'a>(ast: AstBuilder<'a>, bundler: RuntimeType) -> Expression<'a> {
    match bundler {
        RuntimeType::Esm | RuntimeType::Vite => {
            let import_meta = ast.expression_meta_property(
                SPAN,
                ast.identifier_name(SPAN, "import"),
                ast.identifier_name(SPAN, "meta"),
            );
            Expression::StaticMemberExpression(ast.alloc_static_member_expression(
                SPAN,
                import_meta,
                ast.identifier_name(SPAN, "hot"),
                false,
            ))
        }
        RuntimeType::Webpack5 | RuntimeType::RspackEsm => {
            let import_meta = ast.expression_meta_property(
                SPAN,
                ast.identifier_name(SPAN, "import"),
                ast.identifier_name(SPAN, "meta"),
            );
            Expression::StaticMemberExpression(ast.alloc_static_member_expression(
                SPAN,
                import_meta,
                ast.identifier_name(SPAN, "webpackHot"),
                false,
            ))
        }
        RuntimeType::Standard => {
            Expression::StaticMemberExpression(ast.alloc_static_member_expression(
                SPAN,
                ast.expression_identifier(SPAN, "module"),
                ast.identifier_name(SPAN, "hot"),
                false,
            ))
        }
    }
}
