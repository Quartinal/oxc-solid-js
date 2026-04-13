use std::sync::OnceLock;

use html5ever::serialize::{serialize, SerializeOpts};
use html5ever::tendril::TendrilSink;
use html5ever::{parse_fragment, ParseOpts, QualName};
use markup5ever::{local_name, namespace_url, ns};
use markup5ever_rcdom::{RcDom, SerializableHandle};
use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidMarkup {
    pub html: String,
    pub browser: String,
}

fn leading_text_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^[^<]+").expect("leading text-node normalization regex should compile")
    })
}

fn trailing_text_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[^>]+$").expect("trailing text-node normalization regex should compile")
    })
}

fn between_tags_text_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r">[^<]+<").expect("between-tags text-node normalization regex should compile")
    })
}

fn escaped_lt_gt_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"&lt;([^>]+)>").expect("escaped lt/gt normalization regex should compile")
    })
}

fn normalize_markup(html: &str) -> String {
    let mut normalized = html
        .replace("<!>", "<!---->")
        .replace("<!$>", "<!--$-->")
        .replace("<!/>", "<!--/-->");

    normalized = leading_text_regex()
        .replace(&normalized, "#text")
        .into_owned();
    normalized = trailing_text_regex()
        .replace(&normalized, "#text")
        .into_owned();
    normalized = between_tags_text_regex()
        .replace_all(&normalized, ">#text<")
        .into_owned();

    escaped_lt_gt_regex()
        .replace_all(&normalized, "&lt;$1&gt;")
        .into_owned()
}

fn apply_table_context_wrappers(mut html: String) -> String {
    if html.starts_with("<td>") || html.starts_with("<th>") {
        html = format!("<table><tbody><tr>{html}</tr></tbody></table>");
    }

    if html.starts_with("<tr>") {
        html = format!("<table><tbody>{html}</tbody></table>");
    }

    if html.starts_with("<col>") {
        html = format!("<table><colgroup>{html}</colgroup></table>");
    }

    if html.starts_with("<thead>")
        || html.starts_with("<tbody>")
        || html.starts_with("<tfoot>")
        || html.starts_with("<colgroup>")
        || html.starts_with("<caption>")
    {
        html = format!("<table>{html}</table>");
    }

    html
}

fn skip_validation_markup(html: &str) -> bool {
    matches!(
        html,
        "<table></table>"
            | "<table><thead></thead></table>"
            | "<table><tbody></tbody></table>"
            | "<table><thead></thead><tbody></tbody></table>"
    )
}

fn parse_fragment_as_inner_html(html_fragment: &str) -> Option<String> {
    let context = QualName::new(None, ns!(html), local_name!("body"));
    let dom =
        parse_fragment(RcDom::default(), ParseOpts::default(), context, vec![]).one(html_fragment);

    let mut output = Vec::new();
    let children = dom.document.children.borrow();
    for child in children.iter() {
        serialize(
            &mut output,
            &SerializableHandle::from(child.clone()),
            SerializeOpts::default(),
        )
        .ok()?;
    }

    String::from_utf8(output).ok()
}

pub fn is_invalid_markup(html: &str) -> Option<InvalidMarkup> {
    let mut normalized = normalize_markup(html);
    normalized = apply_table_context_wrappers(normalized);

    if skip_validation_markup(&normalized) {
        return None;
    }

    let browser = parse_fragment_as_inner_html(&normalized)?;
    if normalized.to_lowercase() != browser.to_lowercase() {
        return Some(InvalidMarkup {
            html: normalized,
            browser,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_markup_rewrites_markers_and_text_nodes() {
        let normalized = normalize_markup("lead<div>middle</div>tail");
        assert_eq!(normalized, "#text<div>#text</div>#text");

        let markers = normalize_markup("<div><!$><!/><!></div>");
        assert!(markers.contains("<!--$-->"));
        assert!(markers.contains("<!--/-->"));
        assert!(markers.contains("<!---->"));
    }

    #[test]
    fn applies_table_context_wrappers() {
        let wrapped = apply_table_context_wrappers("<tr><td>x</td></tr>".to_string());
        assert_eq!(wrapped, "<table><tbody><tr><td>x</td></tr></tbody></table>");

        let wrapped_cell = apply_table_context_wrappers("<td>x</td>".to_string());
        assert_eq!(
            wrapped_cell,
            "<table><tbody><tr><td>x</td></tr></tbody></table>"
        );
    }

    #[test]
    fn skip_known_empty_table_shapes() {
        assert!(skip_validation_markup("<table></table>"));
        assert!(skip_validation_markup("<table><tbody></tbody></table>"));
        assert!(!skip_validation_markup(
            "<table><tbody><tr><td></td></tr></tbody></table>"
        ));
    }

    #[test]
    fn detects_browser_markup_rewrites() {
        let invalid = is_invalid_markup("<p><div></div></p>");
        assert!(invalid.is_some(), "expected invalid nesting to be detected");
    }
}
