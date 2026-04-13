//! Main SSR transform logic
//!
//! This implements the Traverse trait to walk the AST and transform JSX for SSR.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrayExpressionElement, Expression, JSXChild, JSXElement, JSXExpressionContainer, JSXFragment,
    JSXText, Program, Statement, VariableDeclarationKind,
};
use oxc_ast::NONE;
use oxc_span::SPAN;
use oxc_traverse::{Traverse, TraverseCtx};

use common::{
    build_named_value_import_statement, collect_value_import_local_names, get_tag_name,
    is_component, prepend_program_statements, traverse_program_with_semantic, TransformOptions,
};

use crate::component::transform_component;
use crate::element::transform_element;
use crate::ir::{helper_local_name, template_var_name, SSRContext, SSRResult};
use crate::template::create_template_expression;

/// The main SSR JSX transformer
pub struct SSRTransform<'a> {
    allocator: &'a Allocator,
    options: &'a TransformOptions<'a>,
    context: SSRContext<'a>,
}

#[derive(Clone, Copy)]
struct TransformInfo {
    top_level: bool,
}

impl<'a> SSRTransform<'a> {
    pub fn new(
        allocator: &'a Allocator,
        options: &'a TransformOptions<'a>,
        source_text: &'a str,
    ) -> Self {
        Self {
            allocator,
            options,
            context: SSRContext::new(allocator, options.hydratable, source_text),
        }
    }

    /// Run the transform on a program
    pub fn transform(mut self, program: &mut Program<'a>) {
        let allocator = self.allocator;
        traverse_program_with_semantic(&mut self, allocator, program);
    }

    /// Transform a JSX node and return the SSR result
    fn transform_node(&self, node: &JSXChild<'a>, info: TransformInfo) -> Option<SSRResult<'a>> {
        match node {
            JSXChild::Element(element) => Some(self.transform_jsx_element(element, info)),
            JSXChild::Fragment(fragment) => Some(self.transform_fragment(fragment, info)),
            JSXChild::Text(text) => self.transform_text(text),
            JSXChild::ExpressionContainer(container) => {
                self.transform_expression_container(container)
            }
            JSXChild::Spread(spread) => {
                // Spread children - treat as dynamic
                let mut result = SSRResult::new();
                result.span = spread.span;
                self.context.register_helper("escape");
                result.push_dynamic(self.context.clone_expr(&spread.expression));
                Some(result)
            }
        }
    }

    /// Transform a JSX element
    fn transform_jsx_element(
        &self,
        element: &JSXElement<'a>,
        info: TransformInfo,
    ) -> SSRResult<'a> {
        let tag_name = get_tag_name(element);

        if is_component(&tag_name) {
            // Create child transformer closure that can recursively transform children.
            // Component children are never top-level roots for hydration key insertion.
            let child_info = TransformInfo { top_level: false };
            let child_transformer = |child: &JSXChild<'a>| -> Option<SSRResult<'a>> {
                self.transform_node(child, child_info)
            };
            transform_component(
                element,
                &tag_name,
                &self.context,
                self.options,
                &child_transformer,
            )
        } else {
            transform_element(
                element,
                &tag_name,
                info.top_level,
                &self.context,
                self.options,
            )
        }
    }

    /// Transform a JSX fragment
    fn transform_fragment(&self, fragment: &JSXFragment<'a>, info: TransformInfo) -> SSRResult<'a> {
        let ast = self.context.ast();
        let mut children = Vec::new();

        for child in &fragment.children {
            let child_info = TransformInfo {
                // Top-level fragments should treat direct children as top-level roots.
                top_level: info.top_level,
            };

            self.context.begin_group_scope();
            let child_result = self.transform_node(child, child_info);
            let child_group_state = self.context.take_group_scope();
            self.context.clear_group_scope();

            if let Some(child_result) = child_result {
                children.push((child_result, child_group_state));
            }
        }

        if children.is_empty() {
            return SSRResult::empty_expr(fragment.span);
        }

        if children.len() == 1 {
            let (child_result, child_group_state) =
                children.pop().expect("single child result exists");
            let expr =
                create_template_expression(&self.context, ast, &child_result, child_group_state);
            return SSRResult::with_expr(fragment.span, expr);
        }

        let mut elements = ast.vec_with_capacity(children.len());
        for (child_result, child_group_state) in children {
            let expr =
                create_template_expression(&self.context, ast, &child_result, child_group_state);
            elements.push(ArrayExpressionElement::from(expr));
        }

        SSRResult::with_expr(fragment.span, ast.expression_array(SPAN, elements))
    }

    /// Transform JSX text
    fn transform_text(&self, text: &JSXText<'a>) -> Option<SSRResult<'a>> {
        let content = common::expression::trim_whitespace(&text.value);
        if content.is_empty() {
            return None;
        }

        let mut result = SSRResult::new();
        result.span = text.span;
        result.push_static(&common::expression::escape_html(&content, false));
        Some(result)
    }

    /// Transform a JSX expression container
    fn transform_expression_container(
        &self,
        container: &JSXExpressionContainer<'a>,
    ) -> Option<SSRResult<'a>> {
        if let Some(expr) = container.expression.as_expression() {
            self.context.register_helper("escape");
            let mut result = SSRResult::new();
            result.span = container.span;
            result.push_dynamic(self.context.clone_expr(expr));
            Some(result)
        } else {
            None
        }
    }
}

impl<'a> Traverse<'a, ()> for SSRTransform<'a> {
    // Use exit_expression instead of enter_expression to avoid
    // oxc_traverse walking into our newly created nodes (which lack scope info)
    fn exit_expression(&mut self, node: &mut Expression<'a>, ctx: &mut TraverseCtx<'a, ()>) {
        let new_expr = match node {
            Expression::JSXElement(element) => {
                self.context.begin_group_scope();
                let result = self.transform_jsx_element(element, TransformInfo { top_level: true });
                let group_state = self.context.take_group_scope();
                self.context.clear_group_scope();
                Some(self.build_ssr_expression(&result, group_state, ctx))
            }
            Expression::JSXFragment(fragment) => {
                self.context.begin_group_scope();
                let result = self.transform_fragment(fragment, TransformInfo { top_level: true });
                let group_state = self.context.take_group_scope();
                self.context.clear_group_scope();
                Some(self.build_ssr_expression(&result, group_state, ctx))
            }
            _ => None,
        };

        if let Some(expr) = new_expr {
            *node = expr;
        }
    }

    fn exit_program(&mut self, program: &mut Program<'a>, ctx: &mut TraverseCtx<'a, ()>) {
        let mut helper_names: Vec<String> = self.context.helpers.borrow().iter().cloned().collect();
        if helper_names.len() == 2
            && helper_names.iter().any(|h| h == "ssr")
            && helper_names.iter().any(|h| h == "ssrHydrationKey")
        {
            helper_names = vec!["ssr".to_string(), "ssrHydrationKey".to_string()];
        } else {
            let helper_priority = |name: &str| match name {
                "ssrClassName" => 0usize,
                "ssrStyle" => 1,
                "ssrStyleProperty" => 2,
                "ssrAttribute" => 3,
                "ssrHydrationKey" => 4,
                "ssrRunInScope" => 5,
                "escape" => 6,
                "ssrElement" => 7,
                "mergeProps" => 8,
                "ssr" => 9,
                "NoHydration" => 10,
                "createComponent" => 11,
                _ => 100,
            };
            helper_names.sort_by_key(|name| (helper_priority(name.as_str()), name.clone()));
        }
        let templates = self.context.templates.borrow();

        if helper_names.is_empty() && templates.is_empty() {
            return;
        }

        let ast = ctx.ast;
        let span = SPAN;
        let module_name = self.options.module_name;
        let mut prepend = Vec::new();

        if !helper_names.is_empty() {
            let mut existing_import_locals = collect_value_import_local_names(program);

            for helper in &helper_names {
                let local_name = helper_local_name(helper);
                if existing_import_locals.contains(&local_name) {
                    continue;
                }

                prepend.push(build_named_value_import_statement(
                    ast,
                    span,
                    module_name,
                    helper,
                    &local_name,
                ));
                existing_import_locals.insert(local_name);
            }
        }

        if !templates.is_empty() {
            let mut declarators = ast.vec_with_capacity(templates.len());
            for (i, tmpl) in templates.iter().enumerate() {
                let tmpl_name = template_var_name(i);
                let init = if tmpl.parts.len() <= 1 {
                    let content = tmpl.parts.first().map_or("", String::as_str);
                    ast.expression_string_literal(span, ast.allocator.alloc_str(content), None)
                } else {
                    let mut elements = ast.vec_with_capacity(tmpl.parts.len());
                    for part in &tmpl.parts {
                        elements.push(ArrayExpressionElement::from(ast.expression_string_literal(
                            span,
                            ast.allocator.alloc_str(part),
                            None,
                        )));
                    }
                    ast.expression_array(span, elements)
                };

                declarators.push(ast.variable_declarator(
                    span,
                    VariableDeclarationKind::Var,
                    ast.binding_pattern_binding_identifier(
                        span,
                        ast.allocator.alloc_str(&tmpl_name),
                    ),
                    NONE,
                    Some(init),
                    false,
                ));
            }

            prepend.push(Statement::VariableDeclaration(
                ast.alloc_variable_declaration(
                    span,
                    VariableDeclarationKind::Var,
                    declarators,
                    false,
                ),
            ));
        }

        prepend_program_statements(program, prepend);
    }
}

impl<'a> SSRTransform<'a> {
    /// Build the SSR expression from the transform result
    fn build_ssr_expression(
        &self,
        result: &SSRResult<'a>,
        group_state: Option<crate::ir::GroupState<'a>>,
        ctx: &mut TraverseCtx<'a, ()>,
    ) -> Expression<'a> {
        create_template_expression(&self.context, ctx.ast, result, group_state)
    }
}
