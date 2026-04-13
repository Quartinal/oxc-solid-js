use std::collections::BTreeMap;

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast::{AstBuilder, NONE};
use oxc_span::SPAN;

use crate::checks::is_component_ish_name;
use crate::constants::{IMPORT_COMPONENT, IMPORT_CONTEXT};
use crate::decline_call::build_hmr_decline_call;
use crate::foreign_bindings::collect_referenced_identifiers;
use crate::generator::create_signature_value;
use crate::hot_identifier::build_hot_identifier;
use crate::import_identifier::get_import_identifier;
use crate::register_imports::register_import_specifiers;
use crate::registry::create_registry;
use crate::types::{ImportIdentifierType, Options, RuntimeType, StateContext};
use crate::unwrap::unwrap_expression;
use crate::valid_callee::is_valid_callee;

/// The solid-refresh HMR transform.
///
/// Wraps component and context declarations with HMR registration calls,
/// extracts JSX expressions, and injects bundler-specific hot-module glue.
pub struct SolidRefreshTransform<'a> {
    allocator: &'a Allocator,
    source_text: &'a str,
    state: StateContext<'a>,
}

impl<'a> SolidRefreshTransform<'a> {
    pub fn new(
        allocator: &'a Allocator,
        options: &Options,
        filename: Option<&'a str>,
        source_text: &'a str,
    ) -> Self {
        Self {
            allocator,
            source_text,
            state: StateContext::new(options, filename),
        }
    }

    /// Runs the solid-refresh transform on the program.
    ///
    /// The transform has 4 phases:
    /// - Phase 0: Setup (check @refresh skip/reload, register imports, fix render calls)
    /// - Phase 1: Bubble function declarations to program top
    /// - Phase 2: Transform JSX (extract dynamic expressions — stub)
    /// - Phase 3: Wrap components and contexts with HMR registrations
    /// - Phase 4: Finalize (prepend imports, registry decl, refresh call)
    pub fn transform(mut self, program: &mut Program<'a>) {
        // Phase 0: Setup (comments, import registration, render fixup)
        if self.setup_program(program) {
            return;
        }

        // Phase 1: Bubble component-ish FunctionDeclarations to program top
        self.bubble_function_declarations(program);

        // Phase 2: Transform JSX (extract dynamic expressions into templates)
        self.transform_jsx(program);

        // Phase 3: Wrap components and contexts with HMR registrations
        self.wrap_components(program);

        // Phase 4: Finalize (prepend imports, registry decl, refresh call)
        self.finalize_program(program);
    }

    // -----------------------------------------------------------------------
    // Phase 0: Setup
    // -----------------------------------------------------------------------

    /// Returns `true` if the remaining phases should be skipped
    /// (due to `@refresh skip` or `@refresh reload`).
    fn setup_program(&mut self, program: &mut Program<'a>) -> bool {
        let mut should_skip = false;
        let mut is_done = false;

        for comment in &program.comments {
            let content_span = comment.content_span();
            let start = content_span.start as usize;
            let end = content_span.end as usize;
            if let Some(text) = self.source_text.get(start..end) {
                let trimmed = text.trim();
                if trimmed == "@refresh skip" {
                    should_skip = true;
                    is_done = true;
                    break;
                }
                if trimmed == "@refresh reload" {
                    is_done = true;
                    let ast = AstBuilder::new(self.allocator);
                    let decline = build_hmr_decline_call(&mut self.state, ast);
                    program.body.push(decline);
                    break;
                }
            }
        }

        if !should_skip {
            self.capture_identifiers(program);
            if self.state.fix_render {
                self.fix_render_calls(program);
            }
        }

        is_done
    }

    /// Walk import declarations and register matching bindings in state.
    fn capture_identifiers(&mut self, program: &Program<'a>) {
        for stmt in program.body.iter() {
            if let Statement::ImportDeclaration(import_decl) = stmt {
                if !import_decl.import_kind.is_type() {
                    register_import_specifiers(&mut self.state, import_decl);
                }
            }
        }
    }

    /// Replace top-level `render(...)` calls with `const _cleanup = render(...)`
    /// and insert `if (hot) { hot.dispose(_cleanup); }` after each.
    fn fix_render_calls(&mut self, program: &mut Program<'a>) {
        let mut render_indices: Vec<usize> = Vec::new();

        for (i, stmt) in program.body.iter().enumerate() {
            if let Statement::ExpressionStatement(expr_stmt) = stmt {
                let unwrapped = unwrap_expression(&expr_stmt.expression);
                if let Expression::CallExpression(call) = unwrapped {
                    if is_valid_callee(&self.state, &call.callee, ImportIdentifierType::Render) {
                        render_indices.push(i);
                    }
                }
            }
        }

        // Process in reverse so indices remain valid
        for &i in render_indices.iter().rev() {
            let ast = AstBuilder::new(self.allocator);

            // Remove the ExpressionStatement and extract its expression
            let original_stmt = program.body.remove(i);
            let expr = match original_stmt {
                Statement::ExpressionStatement(expr_stmt) => expr_stmt.unbox().expression,
                // Safety: we only collected ExpressionStatement indices above
                _ => continue,
            };

            // Build: const _cleanup = <expr>;
            let cleanup_name = self.state.generate_uid("cleanup");
            let cleanup_str = ast.allocator.alloc_str(&cleanup_name);
            let binding = ast.binding_pattern_binding_identifier(SPAN, cleanup_str);
            let declarator = ast.variable_declarator(
                SPAN,
                VariableDeclarationKind::Const,
                binding,
                NONE,
                Some(expr),
                false,
            );
            let var_decl = Statement::VariableDeclaration(ast.alloc_variable_declaration(
                SPAN,
                VariableDeclarationKind::Const,
                ast.vec1(declarator),
                false,
            ));
            program.body.insert(i, var_decl);

            // Build: if (hot) { hot.dispose(_cleanup); }
            let hot_test = build_hot_identifier(ast, self.state.bundler);
            let hot_obj = build_hot_identifier(ast, self.state.bundler);
            let dispose_callee =
                Expression::StaticMemberExpression(ast.alloc_static_member_expression(
                    SPAN,
                    hot_obj,
                    ast.identifier_name(SPAN, "dispose"),
                    false,
                ));
            let cleanup_ref = ast.expression_identifier(SPAN, cleanup_str);
            let dispose_call = ast.expression_call(
                SPAN,
                dispose_callee,
                NONE,
                ast.vec1(Argument::from(cleanup_ref)),
                false,
            );
            let dispose_stmt = ast.statement_expression(SPAN, dispose_call);
            let block =
                Statement::BlockStatement(ast.alloc_block_statement(SPAN, ast.vec1(dispose_stmt)));
            let if_stmt = ast.statement_if(SPAN, hot_test, block, None);
            program.body.insert(i + 1, if_stmt);
        }
    }

    // -----------------------------------------------------------------------
    // Phase 1: Bubble function declarations
    // -----------------------------------------------------------------------

    /// Hoists top-level FunctionDeclarations with PascalCase names to the
    /// start of the program body, matching upstream `bubbleFunctionDeclaration`.
    fn bubble_function_declarations(&mut self, program: &mut Program<'a>) {
        let ast = AstBuilder::new(self.allocator);

        #[derive(Clone, Copy)]
        enum BubbleKind {
            Bare,
            ExportNamed,
            ExportDefault,
        }

        let mut to_bubble: Vec<(usize, BubbleKind)> = Vec::new();

        for (i, stmt) in program.body.iter().enumerate() {
            match stmt {
                Statement::FunctionDeclaration(func) if is_bubbleable(func) => {
                    to_bubble.push((i, BubbleKind::Bare));
                }
                Statement::ExportNamedDeclaration(export) => {
                    if let Some(Declaration::FunctionDeclaration(func)) = &export.declaration {
                        if is_bubbleable(func) {
                            to_bubble.push((i, BubbleKind::ExportNamed));
                        }
                    }
                }
                Statement::ExportDefaultDeclaration(export) => {
                    if let ExportDefaultDeclarationKind::FunctionDeclaration(func) =
                        &export.declaration
                    {
                        if is_bubbleable(func) {
                            to_bubble.push((i, BubbleKind::ExportDefault));
                        }
                    }
                }
                _ => {}
            }
        }

        // Process in reverse, collecting function decls to bubble up
        let mut bubbled: Vec<Statement<'a>> = Vec::new();

        for (i, kind) in to_bubble.into_iter().rev() {
            match kind {
                BubbleKind::Bare => {
                    let stmt = program.body.remove(i);
                    bubbled.push(stmt);
                }
                BubbleKind::ExportNamed => {
                    // Remove old `export function Foo() {}`
                    let stmt = program.body.remove(i);
                    let Statement::ExportNamedDeclaration(mut export_decl) = stmt else {
                        continue;
                    };
                    let Some(Declaration::FunctionDeclaration(func_box)) =
                        export_decl.declaration.take()
                    else {
                        continue;
                    };

                    // Extract the name for the export specifier
                    let name = match &func_box.id {
                        Some(id) => id.name.as_str(),
                        None => continue,
                    };
                    let name_str: &'a str = ast.allocator.alloc_str(name);

                    // Build: export { Foo }
                    let local = ast.module_export_name_identifier_name(SPAN, name_str);
                    let exported = ast.module_export_name_identifier_name(SPAN, name_str);
                    let specifier =
                        ast.export_specifier(SPAN, local, exported, ImportOrExportKind::Value);
                    let new_export = ast.export_named_declaration(
                        SPAN,
                        None,
                        ast.vec1(specifier),
                        None,
                        ImportOrExportKind::Value,
                        NONE,
                    );
                    program
                        .body
                        .insert(i, Statement::ExportNamedDeclaration(ast.alloc(new_export)));

                    // Push `function Foo() {}` to bubbled list
                    bubbled.push(Statement::FunctionDeclaration(func_box));
                }
                BubbleKind::ExportDefault => {
                    // Remove old `export default function Foo() {}`
                    let stmt = program.body.remove(i);
                    let Statement::ExportDefaultDeclaration(mut export_decl) = stmt else {
                        continue;
                    };

                    // Extract the function declaration
                    let func_box = match std::mem::replace(
                        &mut export_decl.declaration,
                        // Replace with a dummy — we'll discard the export_decl
                        ExportDefaultDeclarationKind::NullLiteral(ast.alloc_null_literal(SPAN)),
                    ) {
                        ExportDefaultDeclarationKind::FunctionDeclaration(f) => f,
                        _ => continue,
                    };

                    let name = match &func_box.id {
                        Some(id) => id.name.as_str(),
                        None => continue,
                    };
                    let name_str: &'a str = ast.allocator.alloc_str(name);

                    // Build: export default Foo
                    let ident_expr = ast.expression_identifier(SPAN, name_str);
                    let new_export = ast.export_default_declaration(
                        SPAN,
                        ExportDefaultDeclarationKind::from(ident_expr),
                    );
                    program.body.insert(
                        i,
                        Statement::ExportDefaultDeclaration(ast.alloc(new_export)),
                    );

                    // Push `function Foo() {}` to bubbled list
                    bubbled.push(Statement::FunctionDeclaration(func_box));
                }
            }
        }

        // Prepend in original order (reverse back since we collected in reverse)
        bubbled.reverse();
        for (insert_idx, decl) in bubbled.into_iter().enumerate() {
            program.body.insert(insert_idx, decl);
        }
    }

    // -----------------------------------------------------------------------
    // Phase 2: Transform JSX
    // -----------------------------------------------------------------------

    /// Extracts dynamic expressions from JSX trees into template components
    /// for granular HMR. Skipped when `state.jsx` is `false`.
    fn transform_jsx(&mut self, program: &mut Program<'a>) {
        if !self.state.jsx {
            return;
        }
        let mut used_names = std::collections::HashSet::new();
        crate::transform_jsx::transform_all_jsx(
            self.allocator,
            self.source_text,
            &mut used_names,
            program,
        );
    }

    // -----------------------------------------------------------------------
    // Phase 3: Wrap components and contexts
    // -----------------------------------------------------------------------

    /// Wraps component and context declarations with HMR registration calls.
    fn wrap_components(&mut self, program: &mut Program<'a>) {
        let mut i = 0;
        while i < program.body.len() {
            let needs_transform = match &program.body[i] {
                Statement::VariableDeclaration(_) => Some(WrapTarget::VarDecl),
                Statement::FunctionDeclaration(f) => {
                    if is_bubbleable(f) {
                        Some(WrapTarget::FnDecl)
                    } else {
                        None
                    }
                }
                Statement::ExportNamedDeclaration(export) => match &export.declaration {
                    Some(Declaration::VariableDeclaration(_)) => Some(WrapTarget::ExportVarDecl),
                    Some(Declaration::FunctionDeclaration(f)) if is_bubbleable(f) => {
                        Some(WrapTarget::ExportFnDecl)
                    }
                    _ => None,
                },
                Statement::ExportDefaultDeclaration(export) => {
                    if let ExportDefaultDeclarationKind::FunctionDeclaration(f) =
                        &export.declaration
                    {
                        if is_bubbleable(f) {
                            Some(WrapTarget::ExportDefaultFnDecl)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            };

            if let Some(target) = needs_transform {
                match target {
                    WrapTarget::VarDecl => {
                        self.transform_variable_declarators_in_stmt(program, i);
                    }
                    WrapTarget::FnDecl => {
                        self.transform_function_declaration(program, i);
                    }
                    WrapTarget::ExportVarDecl => {
                        self.transform_variable_declarators_in_export(program, i);
                    }
                    WrapTarget::ExportFnDecl => {
                        self.transform_function_declaration_in_named_export(program, i);
                    }
                    WrapTarget::ExportDefaultFnDecl => {
                        self.transform_function_declaration_in_default_export(program, i);
                    }
                }
            }
            i += 1;
        }
    }

    /// Transform declarators within a top-level `VariableDeclaration`.
    fn transform_variable_declarators_in_stmt(
        &mut self,
        program: &mut Program<'a>,
        stmt_idx: usize,
    ) {
        let Statement::VariableDeclaration(var_decl) = &mut program.body[stmt_idx] else {
            return;
        };
        self.transform_variable_declarators(&mut var_decl.declarations);
    }

    /// Transform declarators within `export const Foo = ...`.
    fn transform_variable_declarators_in_export(
        &mut self,
        program: &mut Program<'a>,
        stmt_idx: usize,
    ) {
        let Statement::ExportNamedDeclaration(export) = &mut program.body[stmt_idx] else {
            return;
        };
        let Some(Declaration::VariableDeclaration(var_decl)) = &mut export.declaration else {
            return;
        };
        self.transform_variable_declarators(&mut var_decl.declarations);
    }

    /// Core variable declarator transformation logic: for each declarator with a
    /// PascalCase identifier, wrap component functions or context calls.
    fn transform_variable_declarators(
        &mut self,
        declarations: &mut oxc_allocator::Vec<'a, VariableDeclarator<'a>>,
    ) {
        for decl in declarations.iter_mut() {
            let BindingPattern::BindingIdentifier(ref ident) = decl.id else {
                continue;
            };
            let name = ident.name.as_str();
            if !is_component_ish_name(name) {
                // Still check for createContext below even without PascalCase
                self.try_wrap_context(decl, name);
                continue;
            }

            let Some(ref init) = decl.init else {
                continue;
            };

            // Check for component (arrow/function expression)
            let unwrapped = unwrap_expression(init);
            if is_valid_function_for_component(unwrapped) {
                let init_expr = decl.init.take();
                if let Some(expr) = init_expr {
                    let wrapped = self.build_component_call(name, expr, None);
                    decl.init = Some(wrapped);
                    continue;
                }
            }

            // Check for createContext call
            self.try_wrap_context(decl, name);
        }
    }

    /// Wrap a createContext call if the init is a valid createContext callee.
    fn try_wrap_context(&mut self, decl: &mut VariableDeclarator<'a>, name: &str) {
        let Some(ref init) = decl.init else {
            return;
        };
        let unwrapped = unwrap_expression(init);
        if let Expression::CallExpression(call) = unwrapped {
            if is_valid_callee(
                &self.state,
                &call.callee,
                ImportIdentifierType::CreateContext,
            ) {
                let init_expr = decl.init.take();
                if let Some(expr) = init_expr {
                    let wrapped = self.build_context_call(name, expr);
                    decl.init = Some(wrapped);
                }
            }
        }
    }

    /// Transform a top-level `function Foo() {}` into
    /// `const Foo = $$component(_REGISTRY, "Foo", function Foo(){}, opts)`.
    fn transform_function_declaration(&mut self, program: &mut Program<'a>, stmt_idx: usize) {
        let ast = AstBuilder::new(self.allocator);

        // Remove the function declaration
        let stmt = program.body.remove(stmt_idx);
        let Statement::FunctionDeclaration(func_box) = stmt else {
            program.body.insert(stmt_idx, stmt);
            return;
        };

        let original_span = func_box.span;
        let name = match &func_box.id {
            Some(id) => id.name.as_str(),
            None => {
                program
                    .body
                    .insert(stmt_idx, Statement::FunctionDeclaration(func_box));
                return;
            }
        };
        let name_str: &'a str = ast.allocator.alloc_str(name);

        // Convert FunctionDeclaration → FunctionExpression
        let func_expr = fn_decl_to_fn_expr(ast, func_box);

        let wrapped = self.build_component_call(name_str, func_expr, Some(original_span));

        // Build: const Foo = $$component(...)
        let binding = ast.binding_pattern_binding_identifier(SPAN, name_str);
        let declarator = ast.variable_declarator(
            SPAN,
            VariableDeclarationKind::Const,
            binding,
            NONE,
            Some(wrapped),
            false,
        );
        let var_decl = Statement::VariableDeclaration(ast.alloc_variable_declaration(
            SPAN,
            VariableDeclarationKind::Const,
            ast.vec1(declarator),
            false,
        ));
        program.body.insert(stmt_idx, var_decl);
    }

    /// Transform `export function Foo() {}` into
    /// `export const Foo = $$component(...)`.
    fn transform_function_declaration_in_named_export(
        &mut self,
        program: &mut Program<'a>,
        stmt_idx: usize,
    ) {
        let ast = AstBuilder::new(self.allocator);

        let stmt = program.body.remove(stmt_idx);
        let Statement::ExportNamedDeclaration(mut export_decl) = stmt else {
            program.body.insert(stmt_idx, stmt);
            return;
        };

        let func_box = match export_decl.declaration.take() {
            Some(Declaration::FunctionDeclaration(f)) => f,
            other => {
                export_decl.declaration = other;
                program
                    .body
                    .insert(stmt_idx, Statement::ExportNamedDeclaration(export_decl));
                return;
            }
        };

        let original_span = func_box.span;
        let name = match &func_box.id {
            Some(id) => id.name.as_str(),
            None => {
                export_decl.declaration = Some(Declaration::FunctionDeclaration(func_box));
                program
                    .body
                    .insert(stmt_idx, Statement::ExportNamedDeclaration(export_decl));
                return;
            }
        };
        let name_str: &'a str = ast.allocator.alloc_str(name);

        let func_expr = fn_decl_to_fn_expr(ast, func_box);
        let wrapped = self.build_component_call(name_str, func_expr, Some(original_span));

        // Build: export const Foo = $$component(...)
        let binding = ast.binding_pattern_binding_identifier(SPAN, name_str);
        let declarator = ast.variable_declarator(
            SPAN,
            VariableDeclarationKind::Const,
            binding,
            NONE,
            Some(wrapped),
            false,
        );
        let var_decl = ast.variable_declaration(
            SPAN,
            VariableDeclarationKind::Const,
            ast.vec1(declarator),
            false,
        );
        export_decl.declaration = Some(Declaration::VariableDeclaration(ast.alloc(var_decl)));
        // Clear specifiers (was a declaration export, not a specifier export)
        program
            .body
            .insert(stmt_idx, Statement::ExportNamedDeclaration(export_decl));
    }

    /// Transform `export default function Foo() {}` into
    /// `export default $$component(...)` (the function was already bubbled,
    /// so this handles post-bubble cases or non-bubbled scenarios).
    fn transform_function_declaration_in_default_export(
        &mut self,
        program: &mut Program<'a>,
        stmt_idx: usize,
    ) {
        let ast = AstBuilder::new(self.allocator);

        let stmt = program.body.remove(stmt_idx);
        let Statement::ExportDefaultDeclaration(mut export_decl) = stmt else {
            program.body.insert(stmt_idx, stmt);
            return;
        };

        let func_box = match std::mem::replace(
            &mut export_decl.declaration,
            ExportDefaultDeclarationKind::NullLiteral(ast.alloc_null_literal(SPAN)),
        ) {
            ExportDefaultDeclarationKind::FunctionDeclaration(f) => f,
            other => {
                export_decl.declaration = other;
                program
                    .body
                    .insert(stmt_idx, Statement::ExportDefaultDeclaration(export_decl));
                return;
            }
        };

        let original_span = func_box.span;
        let name = match &func_box.id {
            Some(id) => id.name.as_str(),
            None => {
                export_decl.declaration =
                    ExportDefaultDeclarationKind::FunctionDeclaration(func_box);
                program
                    .body
                    .insert(stmt_idx, Statement::ExportDefaultDeclaration(export_decl));
                return;
            }
        };
        let name_str: &'a str = ast.allocator.alloc_str(name);

        let func_expr = fn_decl_to_fn_expr(ast, func_box);
        let wrapped = self.build_component_call(name_str, func_expr, Some(original_span));

        export_decl.declaration = ExportDefaultDeclarationKind::from(wrapped);
        program
            .body
            .insert(stmt_idx, Statement::ExportDefaultDeclaration(export_decl));
    }

    // -----------------------------------------------------------------------
    // Component / Context call builders
    // -----------------------------------------------------------------------

    /// Builds `$$component(registry, "name", component, { location?, signature?, dependencies? })`.
    fn build_component_call(
        &mut self,
        name: &str,
        component: Expression<'a>,
        original_span: Option<oxc_span::Span>,
    ) -> Expression<'a> {
        let ast = AstBuilder::new(self.allocator);
        let registry_name = create_registry(&mut self.state);
        let component_import_name = get_import_identifier(&mut self.state, &IMPORT_COMPONENT);

        let mut properties = ast.vec();

        // location property
        if let (Some(filename), Some(span)) = (self.state.filename, original_span) {
            let (line, col) = self.offset_to_line_col(span.start);
            let loc_str = format!("{filename}:{line}:{col}");
            let loc_alloc: &'a str = ast.allocator.alloc_str(&loc_str);
            let loc_key =
                PropertyKey::StaticIdentifier(ast.alloc_identifier_name(SPAN, "location"));
            let loc_val = ast.expression_string_literal(SPAN, loc_alloc, None);
            properties.push(ObjectPropertyKind::ObjectProperty(
                ast.alloc_object_property(
                    SPAN,
                    PropertyKind::Init,
                    loc_key,
                    loc_val,
                    false,
                    false,
                    false,
                ),
            ));
        }

        // signature + dependencies (granular mode)
        if self.state.granular {
            let sig = create_signature_value(&component);
            let sig_alloc: &'a str = ast.allocator.alloc_str(&sig);
            let sig_key =
                PropertyKey::StaticIdentifier(ast.alloc_identifier_name(SPAN, "signature"));
            let sig_val = ast.expression_string_literal(SPAN, sig_alloc, None);
            properties.push(ObjectPropertyKind::ObjectProperty(
                ast.alloc_object_property(
                    SPAN,
                    PropertyKind::Init,
                    sig_key,
                    sig_val,
                    false,
                    false,
                    false,
                ),
            ));

            let deps = collect_referenced_identifiers(&component);
            if !deps.is_empty() {
                // Build: () => ({ dep1: dep1, dep2: dep2, ... })
                let mut dep_props = ast.vec();
                let mut sorted_deps: Vec<&String> = deps.iter().collect();
                sorted_deps.sort();
                for dep in sorted_deps {
                    let dep_alloc: &'a str = ast.allocator.alloc_str(dep);
                    let dep_key =
                        PropertyKey::StaticIdentifier(ast.alloc_identifier_name(SPAN, dep_alloc));
                    let dep_val = ast.expression_identifier(SPAN, dep_alloc);
                    dep_props.push(ObjectPropertyKind::ObjectProperty(
                        ast.alloc_object_property(
                            SPAN,
                            PropertyKind::Init,
                            dep_key,
                            dep_val,
                            false,
                            true, // shorthand: { x: x } → { x }
                            false,
                        ),
                    ));
                }
                let dep_obj = ast.expression_object(SPAN, dep_props);

                // Wrap in arrow: () => ({...})
                let params = ast.alloc_formal_parameters(
                    SPAN,
                    FormalParameterKind::ArrowFormalParameters,
                    ast.vec(),
                    NONE,
                );
                // Parenthesized object expression for arrow body
                let paren_obj = ast.expression_parenthesized(SPAN, dep_obj);
                let body = ast.alloc_function_body(
                    SPAN,
                    ast.vec(),
                    ast.vec1(ast.statement_expression(SPAN, paren_obj)),
                );
                let arrow =
                    ast.expression_arrow_function(SPAN, true, false, NONE, params, NONE, body);

                let dep_key =
                    PropertyKey::StaticIdentifier(ast.alloc_identifier_name(SPAN, "dependencies"));
                properties.push(ObjectPropertyKind::ObjectProperty(
                    ast.alloc_object_property(
                        SPAN,
                        PropertyKind::Init,
                        dep_key,
                        arrow,
                        false,
                        false,
                        false,
                    ),
                ));
            }
        }

        let opts = ast.expression_object(SPAN, properties);

        // Build: $$component(registry, "name", component, opts)
        let callee_str: &'a str = ast.allocator.alloc_str(&component_import_name);
        let callee = ast.expression_identifier(SPAN, callee_str);
        let registry_str: &'a str = ast.allocator.alloc_str(&registry_name);
        let registry_ref = ast.expression_identifier(SPAN, registry_str);
        let name_alloc: &'a str = ast.allocator.alloc_str(name);
        let name_lit = ast.expression_string_literal(SPAN, name_alloc, None);

        let mut args = ast.vec_with_capacity(4);
        args.push(Argument::from(registry_ref));
        args.push(Argument::from(name_lit));
        args.push(Argument::from(component));
        args.push(Argument::from(opts));

        ast.expression_call(SPAN, callee, NONE, args, false)
    }

    /// Builds `$$context(registry, "name", contextCall)`.
    fn build_context_call(&mut self, name: &str, context_call: Expression<'a>) -> Expression<'a> {
        let ast = AstBuilder::new(self.allocator);
        let registry_name = create_registry(&mut self.state);
        let context_import_name = get_import_identifier(&mut self.state, &IMPORT_CONTEXT);

        let callee_str: &'a str = ast.allocator.alloc_str(&context_import_name);
        let callee = ast.expression_identifier(SPAN, callee_str);
        let registry_str: &'a str = ast.allocator.alloc_str(&registry_name);
        let registry_ref = ast.expression_identifier(SPAN, registry_str);
        let name_alloc: &'a str = ast.allocator.alloc_str(name);
        let name_lit = ast.expression_string_literal(SPAN, name_alloc, None);

        let mut args = ast.vec_with_capacity(3);
        args.push(Argument::from(registry_ref));
        args.push(Argument::from(name_lit));
        args.push(Argument::from(context_call));

        ast.expression_call(SPAN, callee, NONE, args, false)
    }

    // -----------------------------------------------------------------------
    // Phase 4: Finalize program
    // -----------------------------------------------------------------------

    /// Prepend import declarations, registry const, and append refresh calls.
    fn finalize_program(&mut self, program: &mut Program<'a>) {
        let ast = AstBuilder::new(self.allocator);

        // Only finalize if at least one component/context was wrapped
        let Some(ref registry_import_name) = self.state.registry_import_name.clone() else {
            return;
        };
        let Some(ref refresh_import_name) = self.state.refresh_import_name.clone() else {
            return;
        };
        let Some(ref registry_var_name) = self.state.imports.get("REGISTRY").cloned() else {
            return;
        };

        // 4a: Build import declarations from state.imports
        let import_stmts = self.build_import_declarations(ast);

        // 4b: Build `const _REGISTRY = $$registry()`
        let registry_var_str: &'a str = ast.allocator.alloc_str(registry_var_name);
        let registry_import_str: &'a str = ast.allocator.alloc_str(registry_import_name);
        let registry_callee = ast.expression_identifier(SPAN, registry_import_str);
        let registry_call = ast.expression_call(SPAN, registry_callee, NONE, ast.vec(), false);
        let registry_binding = ast.binding_pattern_binding_identifier(SPAN, registry_var_str);
        let registry_declarator = ast.variable_declarator(
            SPAN,
            VariableDeclarationKind::Const,
            registry_binding,
            NONE,
            Some(registry_call),
            false,
        );
        let registry_decl = Statement::VariableDeclaration(ast.alloc_variable_declaration(
            SPAN,
            VariableDeclarationKind::Const,
            ast.vec1(registry_declarator),
            false,
        ));

        // 4c: Build `if (hot) { $$refresh("bundler", hot, _REGISTRY); }`
        let refresh_import_str: &'a str = ast.allocator.alloc_str(refresh_import_name);
        let hot_test = build_hot_identifier(ast, self.state.bundler);
        let refresh_callee = ast.expression_identifier(SPAN, refresh_import_str);
        let bundler_str_alloc: &'a str = ast.allocator.alloc_str(self.state.bundler.as_str());
        let bundler_lit = ast.expression_string_literal(SPAN, bundler_str_alloc, None);
        let hot_arg = build_hot_identifier(ast, self.state.bundler);
        let registry_ref = ast.expression_identifier(SPAN, registry_var_str);
        let mut refresh_args = ast.vec_with_capacity(3);
        refresh_args.push(Argument::from(bundler_lit));
        refresh_args.push(Argument::from(hot_arg));
        refresh_args.push(Argument::from(registry_ref));
        let refresh_call = ast.expression_call(SPAN, refresh_callee, NONE, refresh_args, false);
        let refresh_stmt = ast.statement_expression(SPAN, refresh_call);
        let refresh_block =
            Statement::BlockStatement(ast.alloc_block_statement(SPAN, ast.vec1(refresh_stmt)));
        let refresh_if = ast.statement_if(SPAN, hot_test, refresh_block, None);

        // Prepend imports at position 0
        let mut insert_pos = 0;
        for stmt in import_stmts {
            program.body.insert(insert_pos, stmt);
            insert_pos += 1;
        }
        // Insert registry const after imports
        program.body.insert(insert_pos, registry_decl);

        // Append refresh if-block at end
        program.body.push(refresh_if);

        // 4d: Vite-specific: hot.accept()
        if self.state.bundler == RuntimeType::Vite {
            let accept_test = build_hot_identifier(ast, self.state.bundler);
            let accept_hot = build_hot_identifier(ast, self.state.bundler);
            let accept_callee =
                Expression::StaticMemberExpression(ast.alloc_static_member_expression(
                    SPAN,
                    accept_hot,
                    ast.identifier_name(SPAN, "accept"),
                    false,
                ));
            let accept_call = ast.expression_call(SPAN, accept_callee, NONE, ast.vec(), false);
            let accept_stmt = ast.statement_expression(SPAN, accept_call);
            let accept_block =
                Statement::BlockStatement(ast.alloc_block_statement(SPAN, ast.vec1(accept_stmt)));
            let accept_if = ast.statement_if(SPAN, accept_test, accept_block, None);
            program.body.push(accept_if);
        }
    }

    /// Build import declarations from `state.imports` entries.
    ///
    /// Keys have format `"source[name]"` (or `"REGISTRY"` for the registry var).
    /// Groups entries by source module, building one `ImportDeclaration` per source.
    fn build_import_declarations(&self, ast: AstBuilder<'a>) -> Vec<Statement<'a>> {
        // Group imports by source: source → Vec<(imported_name, local_name)>
        let mut grouped: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();

        for (key, local_name) in &self.state.imports {
            // Skip the REGISTRY key — it's not an import
            if key == "REGISTRY" {
                continue;
            }
            // Parse key format: "source[name]"
            if let Some(bracket_pos) = key.find('[') {
                if key.ends_with(']') {
                    let source = &key[..bracket_pos];
                    let imported = &key[bracket_pos + 1..key.len() - 1];
                    grouped
                        .entry(source)
                        .or_default()
                        .push((imported, local_name.as_str()));
                }
            }
        }

        let mut stmts = Vec::new();
        for (source, specifiers) in &grouped {
            let source_alloc: &'a str = ast.allocator.alloc_str(source);
            let source_lit = ast.string_literal(SPAN, source_alloc, None);

            let mut import_specifiers = ast.vec_with_capacity(specifiers.len());
            for (imported_name, local_name) in specifiers {
                let imported_alloc: &'a str = ast.allocator.alloc_str(imported_name);
                let local_alloc: &'a str = ast.allocator.alloc_str(local_name);
                let imported =
                    ModuleExportName::IdentifierName(ast.identifier_name(SPAN, imported_alloc));
                let local = ast.binding_identifier(SPAN, local_alloc);
                let spec = ast.import_specifier(SPAN, imported, local, ImportOrExportKind::Value);
                import_specifiers
                    .push(ImportDeclarationSpecifier::ImportSpecifier(ast.alloc(spec)));
            }

            let import_decl = ast.import_declaration(
                SPAN,
                Some(import_specifiers),
                source_lit,
                None,
                NONE,
                ImportOrExportKind::Value,
            );
            stmts.push(Statement::ImportDeclaration(ast.alloc(import_decl)));
        }

        stmts
    }

    // -----------------------------------------------------------------------
    // Utility helpers
    // -----------------------------------------------------------------------

    /// Compute 1-based line and 0-based column from a byte offset.
    fn offset_to_line_col(&self, offset: u32) -> (usize, usize) {
        let offset = (offset as usize).min(self.source_text.len());
        let before = &self.source_text[..offset];
        let line = before.bytes().filter(|&b| b == b'\n').count() + 1;
        let col = before.rfind('\n').map_or(offset, |pos| offset - pos - 1);
        (line, col)
    }
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

/// Checks if an expression is a valid arrow/function expression for component wrapping.
fn is_valid_function_for_component(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::ArrowFunctionExpression(f) => !f.r#async && f.params.items.len() < 2,
        Expression::FunctionExpression(f) => !f.r#async && !f.generator && f.params.items.len() < 2,
        _ => false,
    }
}

/// Checks if a FunctionDeclaration is eligible for bubbling/wrapping.
fn is_bubbleable(func: &Function<'_>) -> bool {
    func.id
        .as_ref()
        .is_some_and(|id| is_component_ish_name(&id.name))
        && !func.generator
        && !func.r#async
        && func.params.items.len() < 2
}

/// Converts a `FunctionDeclaration` (as `Box<Function>`) to a `FunctionExpression`.
fn fn_decl_to_fn_expr<'a>(
    ast: AstBuilder<'a>,
    func: oxc_allocator::Box<'a, Function<'a>>,
) -> Expression<'a> {
    let f = func.unbox();
    ast.expression_function(
        f.span,
        FunctionType::FunctionExpression,
        f.id,
        f.generator,
        f.r#async,
        f.declare,
        f.type_parameters,
        f.this_param,
        f.params,
        f.return_type,
        f.body,
    )
}

/// What kind of statement we need to transform in Phase 3.
enum WrapTarget {
    VarDecl,
    FnDecl,
    ExportVarDecl,
    ExportFnDecl,
    ExportDefaultFnDecl,
}
