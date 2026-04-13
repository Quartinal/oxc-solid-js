//! Tests for the solid-refresh (HMR) transform integration.
//!
//! These tests verify that the HMR wrapping layer works correctly across
//! function declarations, variable declarators, exports, context API,
//! comment directives, fix-render, bundler variations, and SSR mode.
//!
//! Assertions use `contains()` to check structural patterns rather than
//! exact string equality, since the underlying DOM/SSR template output
//! may vary independently.

use common::GenerateMode;
use oxc_solid_js_compiler::{transform, TransformOptions};

/// Normalize whitespace: trim each line, remove blanks, join with newline.
fn normalize(s: &str) -> String {
    s.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn transform_hmr(source: &str) -> String {
    transform_hmr_bundler(source, "standard")
}

fn transform_hmr_bundler(source: &str, bundler: &str) -> String {
    let options = TransformOptions {
        hmr: true,
        hmr_bundler: bundler,
        hmr_granular: true,
        hmr_jsx: true,
        hmr_fix_render: true,
        filename: "example.jsx",
        ..TransformOptions::solid_defaults()
    };
    let result = transform(source, Some(options));
    normalize(&result.code)
}

fn transform_hmr_ssr(source: &str) -> String {
    transform_hmr_ssr_bundler(source, "standard")
}

fn transform_hmr_ssr_bundler(source: &str, bundler: &str) -> String {
    let options = TransformOptions {
        generate: GenerateMode::Ssr,
        hmr: true,
        hmr_bundler: bundler,
        hmr_granular: true,
        hmr_jsx: true,
        hmr_fix_render: true,
        filename: "example.jsx",
        ..TransformOptions::solid_defaults()
    };
    let result = transform(source, Some(options));
    normalize(&result.code)
}

fn transform_hmr_no_jsx(source: &str) -> String {
    let options = TransformOptions {
        hmr: true,
        hmr_bundler: "standard",
        hmr_granular: true,
        hmr_jsx: false,
        hmr_fix_render: true,
        filename: "example.jsx",
        ..TransformOptions::solid_defaults()
    };
    let result = transform(source, Some(options));
    normalize(&result.code)
}

// ---------------------------------------------------------------------------
// Category 1: FunctionDeclaration Wrapping
// ---------------------------------------------------------------------------

#[test]
fn fn_decl_with_params() {
    let out = transform_hmr("function Foo(props) { return <h1>Foo</h1>; }");
    assert!(out.contains("_$$component"), "should wrap with $$component");
    assert!(out.contains("\"Foo\""), "should include component name");
    assert!(out.contains("_$$registry"), "should import $$registry");
    assert!(out.contains("_$$refresh"), "should import $$refresh");
    assert!(
        out.contains("module.hot"),
        "standard bundler uses module.hot"
    );
    assert!(
        out.contains("from \"solid-refresh\""),
        "should import from solid-refresh"
    );
}

#[test]
fn fn_decl_no_params() {
    let out = transform_hmr("function Foo() { return <h1>Foo</h1>; }");
    assert!(
        out.contains("_$$component"),
        "no-param components are still valid"
    );
    assert!(out.contains("\"Foo\""), "should include component name");
    assert!(out.contains("_REGISTRY"), "should declare registry");
    assert!(
        out.contains("module.hot"),
        "standard bundler uses module.hot"
    );
}

#[test]
fn fn_decl_foreign_binding() {
    // Foreign binding captured inside the component body.
    // With a function declaration the solid-refresh pass may include a signature
    // but won't necessarily emit `dependencies` (that's arrow-function specific).
    let out =
        transform_hmr("const example = 'Foo';\nfunction Foo() { return <h1>{example}</h1>; }");
    assert!(
        out.contains("_$$component"),
        "should still wrap with $$component"
    );
    assert!(out.contains("\"Foo\""), "should include component name");
    assert!(out.contains("signature"), "should include signature hash");
}

// ---------------------------------------------------------------------------
// Category 2: VariableDeclarator Wrapping
// ---------------------------------------------------------------------------

#[test]
fn var_func_expression() {
    let out = transform_hmr("const Foo = function() { return <h1>Foo</h1>; }");
    assert!(
        out.contains("_$$component"),
        "function expression should be wrapped"
    );
    assert!(out.contains("\"Foo\""), "should include component name");
    assert!(out.contains("_REGISTRY"), "should declare registry");
}

#[test]
fn var_arrow_function() {
    let out = transform_hmr("const Foo = () => <h1>Foo</h1>");
    assert!(
        out.contains("_$$component"),
        "arrow function should be wrapped"
    );
    assert!(out.contains("\"Foo\""), "should include component name");
    // Arrow functions with captured vars get dependency tracking
    assert!(
        out.contains("dependencies"),
        "arrow functions track dependencies"
    );
}

// ---------------------------------------------------------------------------
// Category 3: Export Wrapping
// ---------------------------------------------------------------------------

#[test]
fn export_named() {
    let out = transform_hmr("export function Foo(props) { return <h1>Foo</h1>; }");
    assert!(out.contains("_$$component"), "should wrap with $$component");
    assert!(
        out.contains("export { Foo }") || out.contains("export {Foo}"),
        "named export should be preserved as re-export"
    );
}

#[test]
fn export_default() {
    let out = transform_hmr("export default function Foo(props) { return <h1>Foo</h1>; }");
    assert!(out.contains("_$$component"), "should wrap with $$component");
    assert!(
        out.contains("export default Foo"),
        "default export should be preserved"
    );
}

// ---------------------------------------------------------------------------
// Category 4: Context API
// ---------------------------------------------------------------------------

#[test]
fn context_create() {
    let out =
        transform_hmr("import { createContext } from 'solid-js'; const Ctx = createContext();");
    assert!(
        out.contains("_$$context"),
        "should wrap with $$context for createContext"
    );
    assert!(out.contains("\"Ctx\""), "should include context name");
    assert!(
        out.contains("createContext()"),
        "should preserve createContext call"
    );
    assert!(
        out.contains("from \"solid-refresh\""),
        "should import from solid-refresh"
    );
}

#[test]
fn context_exported() {
    let out = transform_hmr(
        "import { createContext } from 'solid-js'; export const Ctx = createContext();",
    );
    assert!(out.contains("_$$context"), "should wrap with $$context");
    assert!(
        out.contains("export const Ctx"),
        "export should be preserved"
    );
}

// ---------------------------------------------------------------------------
// Category 5: Comment Directives
// ---------------------------------------------------------------------------

#[test]
fn refresh_skip() {
    let out = transform_hmr("// @refresh skip\nfunction Foo() { return <h1>Foo</h1>; }");
    assert!(
        !out.contains("_$$component"),
        "@refresh skip should suppress component wrapping"
    );
    assert!(
        !out.contains("_$$registry"),
        "@refresh skip should suppress registry"
    );
    assert!(
        !out.contains("module.hot"),
        "@refresh skip should suppress HMR footer"
    );
}

#[test]
fn refresh_reload() {
    let out = transform_hmr("// @refresh reload\nfunction Foo() { return <h1>Foo</h1>; }");
    assert!(
        out.contains("_$$decline"),
        "@refresh reload should emit $$decline call"
    );
    assert!(
        !out.contains("_$$component"),
        "@refresh reload should not wrap components"
    );
    assert!(
        out.contains("module.hot"),
        "@refresh reload with standard bundler uses module.hot"
    );
}

// ---------------------------------------------------------------------------
// Category 6: Fix Render
// ---------------------------------------------------------------------------

#[test]
fn fix_render() {
    let out = transform_hmr(
        "import { render } from 'solid-js/web'; render(() => <App />, document.body);",
    );
    assert!(
        out.contains("dispose") || out.contains("_cleanup"),
        "fix_render should capture render cleanup handle"
    );
    assert!(
        out.contains("module.hot"),
        "fix_render should emit HMR dispose block"
    );
}

// ---------------------------------------------------------------------------
// Category 7: Non-Component / Invalid
// ---------------------------------------------------------------------------

#[test]
fn lowercase_not_wrapped() {
    let out = transform_hmr("const foo = () => <h1>Hello</h1>");
    assert!(
        !out.contains("_$$component"),
        "lowercase identifiers should not be wrapped as components"
    );
    assert!(
        !out.contains("_$$registry"),
        "no registry for non-component file"
    );
}

#[test]
fn too_many_params() {
    let out = transform_hmr("function Foo(a, b) { return <h1>Foo</h1>; }");
    assert!(
        !out.contains("_$$component"),
        "functions with >1 param should not be treated as components"
    );
    assert!(
        !out.contains("_$$registry"),
        "no registry for non-component"
    );
}

// ---------------------------------------------------------------------------
// Category 8: Bundler Variations
// ---------------------------------------------------------------------------

#[test]
fn bundler_vite() {
    let out = transform_hmr_bundler("function Foo() { return <h1>Foo</h1>; }", "vite");
    assert!(
        out.contains("import.meta.hot"),
        "vite bundler uses import.meta.hot"
    );
    assert!(
        out.contains(".accept()"),
        "vite bundler should emit .accept()"
    );
    assert!(
        !out.contains("module.hot"),
        "vite bundler should not use module.hot"
    );
}

#[test]
fn bundler_esm() {
    let out = transform_hmr_bundler("function Foo() { return <h1>Foo</h1>; }", "esm");
    assert!(
        out.contains("import.meta.hot"),
        "esm bundler uses import.meta.hot"
    );
    assert!(
        !out.contains("module.hot"),
        "esm bundler should not use module.hot"
    );
}

#[test]
fn bundler_webpack5() {
    let out = transform_hmr_bundler("function Foo() { return <h1>Foo</h1>; }", "webpack5");
    assert!(
        out.contains("import.meta.webpackHot"),
        "webpack5 bundler uses import.meta.webpackHot"
    );
}

// ---------------------------------------------------------------------------
// Category 9: SSR Mode
// ---------------------------------------------------------------------------

#[test]
fn ssr_basic_component() {
    let out = transform_hmr_ssr("function Foo() { return <h1>Foo</h1>; }");
    assert!(
        out.contains("_$$component"),
        "SSR mode should still wrap components"
    );
    assert!(out.contains("_$ssr"), "SSR mode should use _$ssr helper");
    assert!(
        out.contains("module.hot"),
        "SSR mode with standard bundler uses module.hot"
    );
}

#[test]
fn ssr_context() {
    let out =
        transform_hmr_ssr("import { createContext } from 'solid-js'; const Ctx = createContext();");
    assert!(
        out.contains("_$$context"),
        "SSR context should wrap with $$context"
    );
    assert!(
        out.contains("\"Ctx\""),
        "SSR context should include context name"
    );
}

// ---------------------------------------------------------------------------
// Category 10: Multiple Components
// ---------------------------------------------------------------------------

#[test]
fn multiple_components() {
    let out = transform_hmr(
        "function Bar() { return <h1>Bar</h1>; }\nfunction Foo() { return <h1>Foo</h1>; }",
    );
    assert!(out.contains("\"Bar\""), "should wrap Bar");
    assert!(out.contains("\"Foo\""), "should wrap Foo");
    // Both should use $$component
    let count = out.matches("_$$component").count();
    assert!(
        count >= 2,
        "should have at least 2 $$component calls, got {}",
        count
    );
    // Single registry for the module
    let registry_decls = out.matches("_$$registry()").count();
    assert_eq!(
        registry_decls, 1,
        "should have exactly 1 registry declaration"
    );
}

// ---------------------------------------------------------------------------
// Category 11: Structural Integrity
// ---------------------------------------------------------------------------

#[test]
fn imports_from_solid_refresh() {
    let out = transform_hmr("function Foo() { return <h1>Foo</h1>; }");
    assert!(
        out.contains("from \"solid-refresh\""),
        "should import from solid-refresh"
    );
    assert!(
        out.contains("$$component"),
        "should import $$component helper"
    );
    assert!(
        out.contains("$$registry"),
        "should import $$registry helper"
    );
    assert!(out.contains("$$refresh"), "should import $$refresh helper");
}

// ---------------------------------------------------------------------------
// Category 12: hmr_jsx=false (non-granular JSX mode)
// ---------------------------------------------------------------------------

#[test]
fn hmr_jsx_disabled() {
    let out = transform_hmr_no_jsx("function Foo() { return <h1>Foo</h1>; }");
    // Should still wrap the component even without jsx extraction
    assert!(
        out.contains("_$$component"),
        "hmr_jsx=false should still wrap components"
    );
    assert!(out.contains("\"Foo\""), "should include component name");
    assert!(out.contains("module.hot"), "should still emit HMR footer");
}

// ---------------------------------------------------------------------------
// Category 13: Bundler-specific SSR
// ---------------------------------------------------------------------------

#[test]
fn ssr_vite_bundler() {
    let out = transform_hmr_ssr_bundler("function Foo() { return <h1>Foo</h1>; }", "vite");
    assert!(
        out.contains("import.meta.hot"),
        "SSR with vite should use import.meta.hot"
    );
    assert!(
        out.contains("_$$component"),
        "SSR with vite should still wrap components"
    );
    assert!(
        out.contains("_$ssr"),
        "SSR with vite should use _$ssr helper"
    );
}

// ---------------------------------------------------------------------------
// Category 14: Signature & Location Metadata
// ---------------------------------------------------------------------------

#[test]
fn fn_decl_has_location() {
    let out = transform_hmr("function Foo(props) { return <h1>Foo</h1>; }");
    assert!(
        out.contains("location:"),
        "function declarations should include location metadata"
    );
    assert!(
        out.contains("example.jsx"),
        "location should reference the filename"
    );
}

#[test]
fn fn_decl_has_signature() {
    let out = transform_hmr("function Foo() { return <h1>Foo</h1>; }");
    assert!(
        out.contains("signature:"),
        "should include signature hash for invalidation"
    );
}
