use std::collections::HashSet;

use oxc_ast::ast::{
    ImportDeclarationSpecifier, ImportOrExportKind, ModuleExportName, Program, Statement,
};
use oxc_ast::AstBuilder;
use oxc_span::Span;

pub fn collect_value_import_local_names(program: &Program<'_>) -> HashSet<String> {
    let mut locals = HashSet::new();

    for stmt in &program.body {
        let Statement::ImportDeclaration(import_decl) = stmt else {
            continue;
        };

        if import_decl.import_kind != ImportOrExportKind::Value {
            continue;
        }

        let Some(specifiers) = &import_decl.specifiers else {
            continue;
        };

        for spec in specifiers.iter() {
            match spec {
                ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    locals.insert(s.local.name.as_str().to_string());
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    locals.insert(s.local.name.as_str().to_string());
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    locals.insert(s.local.name.as_str().to_string());
                }
            }
        }
    }

    locals
}

pub fn build_named_value_import_statement<'a>(
    ast: AstBuilder<'a>,
    span: Span,
    module_name: &str,
    imported_name: &str,
    local_name: &str,
) -> Statement<'a> {
    let imported = ModuleExportName::IdentifierName(
        ast.identifier_name(span, ast.allocator.alloc_str(imported_name)),
    );
    let local = ast.binding_identifier(span, ast.allocator.alloc_str(local_name));
    let specifier = ast.import_specifier(span, imported, local, ImportOrExportKind::Value);
    let source = ast.string_literal(span, ast.allocator.alloc_str(module_name), None);

    let import_decl = ast.import_declaration(
        span,
        Some(ast.vec1(ImportDeclarationSpecifier::ImportSpecifier(
            ast.alloc(specifier),
        ))),
        source,
        None,
        None::<oxc_ast::ast::WithClause<'a>>,
        ImportOrExportKind::Value,
    );

    Statement::ImportDeclaration(ast.alloc(import_decl))
}

pub fn prepend_program_statements<'a>(program: &mut Program<'a>, statements: Vec<Statement<'a>>) {
    for statement in statements.into_iter().rev() {
        program.body.insert(0, statement);
    }
}
