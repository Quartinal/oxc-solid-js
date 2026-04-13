//! Intermediate Representation for SSR transforms
//! Ported toward babel-plugin-jsx-dom-expressions SSR shape.

use indexmap::IndexSet;
use oxc_allocator::{Allocator, CloneIn};
use oxc_ast::ast::{Expression, JSXChild};
use oxc_ast::AstBuilder;
use oxc_span::{GetSpanMut, Span, SPAN};
use rustc_hash::{FxBuildHasher, FxHashMap};
use std::cell::RefCell;

/// Function type for transforming child JSX elements
pub type SSRChildTransformer<'a, 'b> = &'b dyn Fn(&JSXChild<'a>) -> Option<SSRResult<'a>>;

#[derive(Default)]
pub struct SSRResult<'a> {
    /// Source span of the originating JSX node
    pub span: Span,

    /// Static template parts (strings around dynamic insertions)
    pub template_parts: Vec<String>,

    /// Dynamic values to pass to `ssr(_tmpl$, ...values)`
    pub template_values: Vec<Expression<'a>>,

    /// Hoisted declarations emitted before return
    pub declarations: Vec<HoistedDeclarator<'a>>,

    /// Hoisted declarations emitted after grouped dynamics variable
    pub post_declarations: Vec<HoistedDeclarator<'a>>,

    /// Expressions for non-template outputs (e.g. `ssrElement(...)`, fragments)
    pub exprs: Vec<Expression<'a>>,

    /// Whether this node should be emitted as spread-element runtime (`ssrElement`)
    pub spread_element: bool,

    /// Whether this template should bypass extra wrapping optimizations
    pub wont_escape: bool,

    /// Optional generated group id for `ssrRunInScope([ ... ])`
    pub group_id: Option<String>,

    /// Grouped dynamic closures used with `group_id`
    pub dynamics: Vec<Expression<'a>>,
}

pub struct HoistedDeclarator<'a> {
    pub name: String,
    pub expr: Expression<'a>,
}

impl<'a> SSRResult<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append static text to the current template segment.
    pub fn push_static(&mut self, text: &str) {
        if self.template_parts.is_empty() {
            self.template_parts.push(text.to_string());
        } else {
            self.template_parts
                .last_mut()
                .expect("template_parts has at least one entry")
                .push_str(text);
        }
    }

    /// Append a dynamic interpolation slot (Babel `template.push(""); templateValues.push(expr)`).
    pub fn push_dynamic(&mut self, expr: Expression<'a>) {
        if self.template_parts.is_empty() {
            self.template_parts.push(String::new());
        }
        self.template_parts.push(String::new());
        self.template_values.push(expr);
    }

    /// Push a template chunk boundary (Babel `template.push("")` parity).
    pub fn push_template_part(&mut self, part: &str) {
        self.template_parts.push(part.to_string());
    }

    /// Push a template interpolation value without forcing an extra chunk.
    pub fn push_template_value(&mut self, expr: Expression<'a>) {
        self.template_values.push(expr);
    }

    /// Merge another SSR result into this one.
    pub fn merge(&mut self, mut other: SSRResult<'a>) {
        // Merge template parts/values preserving segment boundaries.
        if other.template_parts.is_empty() && !other.template_values.is_empty() {
            other.template_parts.push(String::new());
            while other.template_parts.len() < other.template_values.len() + 1 {
                other.template_parts.push(String::new());
            }
        }

        if self.template_parts.is_empty() {
            self.template_parts = other.template_parts;
            self.template_values = other.template_values;
        } else if !other.template_parts.is_empty() {
            if let Some(first) = other.template_parts.first() {
                self.template_parts
                    .last_mut()
                    .expect("template_parts has at least one entry")
                    .push_str(first);
            }
            self.template_values.extend(other.template_values);
            self.template_parts
                .extend(other.template_parts.into_iter().skip(1));
        }

        self.declarations.extend(other.declarations);
        self.post_declarations.extend(other.post_declarations);

        if self.group_id.is_none() {
            self.group_id = other.group_id;
        }
        self.dynamics.extend(other.dynamics);

        self.exprs.extend(other.exprs);
        self.spread_element |= other.spread_element;
        self.wont_escape |= other.wont_escape;
    }

    pub fn has_template(&self) -> bool {
        !self.template_parts.is_empty()
    }
}

pub struct TemplateInfo {
    pub parts: Vec<String>,
}

pub fn template_var_name(index: usize) -> String {
    if index == 0 {
        "_tmpl$".to_string()
    } else {
        format!("_tmpl${}", index + 1)
    }
}

pub fn helper_local_name(name: &str) -> String {
    format!("_${}", name)
}

pub fn helper_ident_expr<'a>(ast: AstBuilder<'a>, span: Span, name: &str) -> Expression<'a> {
    let local = helper_local_name(name);
    ast.expression_identifier(span, ast.allocator.alloc_str(&local))
}

#[derive(Default)]
pub struct GroupState<'a> {
    pub id: Option<String>,
    pub dynamics: Vec<Expression<'a>>,
}

/// Context for SSR block transformation
pub struct SSRContext<'a> {
    /// Helper imports needed
    pub helpers: RefCell<IndexSet<String, FxBuildHasher>>,

    /// Hoisted template declarations
    pub templates: RefCell<Vec<TemplateInfo>>,

    /// Variable counters for unique names keyed by prefix.
    pub var_counter: RefCell<FxHashMap<String, usize>>,

    /// Whether we're in hydratable mode
    pub hydratable: bool,

    /// Grouped dynamics state for current top-level transformed expression
    group_state: RefCell<Option<GroupState<'a>>>,

    source_text: &'a str,
    allocator: &'a Allocator,
}

impl<'a> SSRContext<'a> {
    pub fn new(allocator: &'a Allocator, hydratable: bool, source_text: &'a str) -> Self {
        Self {
            helpers: RefCell::new(IndexSet::with_hasher(FxBuildHasher)),
            templates: RefCell::new(Vec::new()),
            var_counter: RefCell::new(FxHashMap::default()),
            hydratable,
            group_state: RefCell::new(None),
            source_text,
            allocator,
        }
    }

    /// Generate a unique variable name using Babel-style numbering.
    ///
    /// First id: `_{prefix}`. Then `_{prefix}2`, `_{prefix}3`, ...
    pub fn generate_uid(&self, prefix: &str) -> String {
        let mut counters = self.var_counter.borrow_mut();
        let count = counters.entry(prefix.to_string()).or_insert(0);
        *count += 1;

        if *count == 1 {
            format!("_{}", prefix)
        } else {
            format!("_{}{}", prefix, *count)
        }
    }

    pub fn register_helper(&self, name: &str) {
        self.helpers.borrow_mut().insert(name.to_string());
    }

    /// Push template parts and return template index (deduplicated).
    pub fn push_template(&self, parts: Vec<String>) -> usize {
        let mut templates = self.templates.borrow_mut();
        if let Some(index) = templates.iter().position(|tmpl| tmpl.parts == parts) {
            return index;
        }
        templates.push(TemplateInfo { parts });
        templates.len() - 1
    }

    pub fn begin_group_scope(&self) {
        *self.group_state.borrow_mut() = Some(GroupState::default());
    }

    pub fn clear_group_scope(&self) {
        *self.group_state.borrow_mut() = None;
    }

    pub fn ensure_group_id(&self) -> String {
        let mut state_ref = self.group_state.borrow_mut();
        let state = state_ref.get_or_insert_with(GroupState::default);
        if state.id.is_none() {
            state.id = Some(self.generate_uid("v$"));
        }
        state.id.clone().expect("group id initialized")
    }

    pub fn push_group_dynamic(&self, expr: Expression<'a>) -> usize {
        let mut state_ref = self.group_state.borrow_mut();
        let state = state_ref.get_or_insert_with(GroupState::default);
        state.dynamics.push(expr);
        state.dynamics.len() - 1
    }

    pub fn take_group_scope(&self) -> Option<GroupState<'a>> {
        self.group_state.borrow_mut().take()
    }

    pub fn ast(&self) -> AstBuilder<'a> {
        AstBuilder::new(self.allocator)
    }

    pub fn clone_expr(&self, expr: &Expression<'a>) -> Expression<'a> {
        expr.clone_in(self.allocator)
    }

    pub fn clone_expr_without_trivia(&self, expr: &Expression<'a>) -> Expression<'a> {
        let mut cloned = expr.clone_in(self.allocator);
        *cloned.span_mut() = SPAN;
        cloned
    }

    pub fn has_static_marker_comment(&self, span: Span, marker: &str) -> bool {
        let start = span.start as usize;
        let end = span.end as usize;

        if start >= end || end > self.source_text.len() {
            return false;
        }

        let snippet = &self.source_text[start..end];
        if !snippet.contains(marker) {
            return false;
        }

        let bytes = snippet.as_bytes();
        let mut index = 0usize;

        loop {
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }

            if index >= bytes.len() {
                return false;
            }

            match bytes[index] {
                b'{' | b'(' => {
                    index += 1;
                    continue;
                }
                _ => break,
            }
        }

        let rest = &snippet[index..];
        let Some(after_open) = rest.strip_prefix("/*") else {
            return false;
        };
        let Some(comment_end) = after_open.find("*/") else {
            return false;
        };

        after_open[..comment_end].trim() == marker
    }
}

impl<'a> HoistedDeclarator<'a> {
    pub fn new(name: String, expr: Expression<'a>) -> Self {
        Self { name, expr }
    }
}

impl<'a> SSRResult<'a> {
    pub fn with_expr(span: Span, expr: Expression<'a>) -> Self {
        Self {
            span,
            exprs: vec![expr],
            ..Default::default()
        }
    }

    pub fn empty_expr(span: Span) -> Self {
        Self {
            span,
            exprs: Vec::new(),
            ..Default::default()
        }
    }

    pub fn ensure_template_shape(&mut self) {
        if self.template_parts.is_empty() && !self.template_values.is_empty() {
            self.template_parts.push(String::new());
        }
        while self.template_parts.len() < self.template_values.len() + 1 {
            self.template_parts.push(String::new());
        }
    }

    pub fn mark_wont_escape(mut self) -> Self {
        self.wont_escape = true;
        self
    }

    pub fn append_expr(&mut self, expr: Expression<'a>) {
        self.exprs.push(expr);
    }

    pub fn first_expr(&self) -> Option<&Expression<'a>> {
        self.exprs.first()
    }

    pub fn default_span() -> Span {
        SPAN
    }
}
