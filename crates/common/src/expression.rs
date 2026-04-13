//! Expression utilities for working with OXC AST

use std::borrow::Cow;

use oxc_ast::ast::{Expression, JSXChild, JSXElement, JSXText, Statement};
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_span::Span;

/// Convert an Expression AST node to its source code string
pub fn expr_to_string(expr: &Expression<'_>) -> String {
    let mut codegen = Codegen::new().with_options(CodegenOptions::default());
    codegen.print_expression(expr);
    codegen.into_source_text()
}

/// Convert a Statement AST node to its source code string
pub fn stmt_to_string(stmt: &Statement<'_>) -> String {
    // For statements, we need to wrap in a minimal program context
    // But for most cases we just need expression statements
    match stmt {
        Statement::ExpressionStatement(expr_stmt) => expr_to_string(&expr_stmt.expression),
        _ => {
            // Fallback - this is less common
            format!("/* unsupported statement */")
        }
    }
}

/// A simple expression node that tracks static vs dynamic
pub struct SimpleExpression<'a> {
    pub content: String,
    pub is_static: bool,
    pub expr: Option<&'a Expression<'a>>,
    pub span: Span,
}

impl<'a> SimpleExpression<'a> {
    pub fn static_value(content: String, span: Span) -> Self {
        Self {
            content,
            is_static: true,
            expr: None,
            span,
        }
    }

    pub fn dynamic(content: String, expr: &'a Expression<'a>, span: Span) -> Self {
        Self {
            content,
            is_static: false,
            expr: Some(expr),
            span,
        }
    }
}

/// Escape HTML special characters.
///
/// Parity with Babel's `escapeHTML`:
/// - text mode (`quote_escape = false`): escape `&` and `<`
/// - attr mode (`quote_escape = true`): escape `&` and `"`
/// - never escape `>`
pub fn escape_html<'a>(text: &'a str, quote_escape: bool) -> Cow<'a, str> {
    let needs_escape = if quote_escape {
        text.contains('&') || text.contains('"')
    } else {
        text.contains('&') || text.contains('<')
    };

    if !needs_escape {
        return Cow::Borrowed(text);
    }

    let mut result = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => result.push_str("&amp;"),
            '"' if quote_escape => result.push_str("&quot;"),
            '<' if !quote_escape => result.push_str("&lt;"),
            _ => result.push(c),
        }
    }
    Cow::Owned(result)
}

/// Return JSX text exactly as authored (parser raw text when available).
pub fn jsx_text_source<'a>(text: &'a JSXText<'a>) -> &'a str {
    text.raw
        .as_ref()
        .map(|raw| raw.as_str())
        .unwrap_or_else(|| text.value.as_str())
}

/// Normalize JSX text whitespace while preserving authored HTML entities.
pub fn normalize_jsx_text<'a>(text: &'a JSXText<'a>) -> Cow<'a, str> {
    trim_whitespace(jsx_text_source(text))
}

/// Decode HTML entities for JS string-literal contexts (fragments/components).
pub fn decode_html_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }

    fn decode_named_entity(entity: &str) -> Option<&'static str> {
        match entity {
            "amp" => Some("&"),
            "lt" => Some("<"),
            "gt" => Some(">"),
            "quot" => Some("\""),
            "apos" => Some("'"),
            "nbsp" => Some("\u{00A0}"),
            "hellip" => Some("\u{2026}"),
            _ => None,
        }
    }

    fn decode_numeric_entity(entity: &str) -> Option<String> {
        let codepoint = if let Some(hex) = entity
            .strip_prefix("#x")
            .or_else(|| entity.strip_prefix("#X"))
        {
            u32::from_str_radix(hex, 16).ok()?
        } else if let Some(decimal) = entity.strip_prefix('#') {
            decimal.parse::<u32>().ok()?
        } else {
            return None;
        };

        let normalized = match codepoint {
            0x00 | 0xD800..=0xDFFF => 0xFFFD,
            cp if cp > 0x10FFFF => 0xFFFD,
            _ => codepoint,
        };
        char::from_u32(normalized).map(|c| c.to_string())
    }

    let mut output = String::with_capacity(text.len());
    let mut index = 0usize;

    while index < text.len() {
        let mut chars = text[index..].chars();
        let ch = chars.next().unwrap();

        if ch == '&' {
            if let Some(semi_offset) = text[index + 1..].find(';') {
                let end = index + 1 + semi_offset;
                let entity = &text[index + 1..end];
                if let Some(decoded) = decode_named_entity(entity) {
                    output.push_str(decoded);
                    index = end + 1;
                    continue;
                }
                if let Some(decoded) = decode_numeric_entity(entity) {
                    output.push_str(&decoded);
                    index = end + 1;
                    continue;
                }
            }
        }

        output.push(ch);
        index += ch.len_utf8();
    }

    output
}

/// Trim whitespace from JSX text (preserving significant spaces)
///
/// JSX whitespace rules:
/// - Text with newlines: trim leading/trailing whitespace (indentation)
/// - Inline text (no newlines): preserve trailing space (e.g., ". " between expressions)
/// - Multiple whitespace collapses to single space
pub fn trim_whitespace(text: &str) -> Cow<'_, str> {
    if text.is_empty() {
        return Cow::Borrowed("");
    }

    let has_newline = text.contains('\n');

    // Fast path: whitespace-only text with newlines trims to empty.
    if has_newline && text.bytes().all(|b| b.is_ascii_whitespace()) {
        return Cow::Borrowed("");
    }

    // Fast path: no whitespace at all means text is returned as-is.
    if !text.bytes().any(|b| b.is_ascii_whitespace()) {
        return Cow::Borrowed(text);
    }

    // Collapse multiple whitespace into single space
    let mut result = String::new();
    let mut prev_was_space = false;

    for c in text.chars() {
        if c.is_whitespace() {
            if has_newline {
                // Ignore leading indentation/newlines; we'll trim later.
                if !prev_was_space && !result.is_empty() {
                    result.push(' ');
                    prev_was_space = true;
                }
                continue;
            }

            // Inline text: preserve a single leading space (e.g., " Click" after an element)
            if !prev_was_space {
                result.push(' ');
                prev_was_space = true;
            }
        } else {
            result.push(c);
            prev_was_space = false;
        }
    }

    // Only trim if text contained newlines (multi-line JSX text with indentation)
    // Preserve trailing space for inline text like ". " between expressions
    if has_newline {
        let trimmed = result.trim();
        if trimmed.is_empty() {
            Cow::Borrowed("")
        } else {
            Cow::Owned(trimmed.to_string())
        }
    } else {
        Cow::Owned(result)
    }
}

/// Convert event name from JSX format (onClick or on:click) to DOM format (click)
pub fn to_event_name(name: &str) -> Cow<'_, str> {
    if name.starts_with("on:") {
        // Handle on:click -> click (namespaced form)
        Cow::Borrowed(&name[3..])
    } else if name.starts_with("on") {
        // Handle onClick -> click, onMouseDown -> mousedown (lowercase entire name)
        Cow::Owned(name[2..].to_lowercase())
    } else {
        Cow::Borrowed(name)
    }
}

/// Convert property name to proper case (kebab-case -> camelCase)
pub fn to_property_name(name: &str) -> String {
    let mut result = String::new();
    let lower = name.to_ascii_lowercase();
    let mut chars = lower.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '-' {
            if let Some(next) = chars.next() {
                result.push(next.to_ascii_uppercase());
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Get children as a callback expression from a JSX element.
///
/// Used for control flow components (For, Index, etc.) that expect
/// arrow function children like: `<For each={items}>{item => <div>{item}</div>}</For>`
///
/// Returns the expression string, or "() => undefined" if no expression child found.
pub fn get_children_callback(element: &JSXElement<'_>) -> String {
    for child in &element.children {
        if let JSXChild::ExpressionContainer(container) = child {
            if let Some(expr) = container.expression.as_expression() {
                return expr_to_string(expr);
            }
        }
    }
    "() => undefined".to_string()
}
