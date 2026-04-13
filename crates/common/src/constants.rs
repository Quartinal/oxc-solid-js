//! Constants ported from dom-expressions/src/constants.js
//! These define which attributes are properties, delegated events, etc.

use phf::{phf_set, Set};

/// Placeholder used to encode hyphenated JSX member segments before parsing.
///
/// OXC currently rejects `<module.a-b />` during parse. We pre-rewrite `a-b` to this
/// placeholder form, parse, then decode back to `"a-b"` and emit computed member access.
pub const JSX_MEMBER_DASH_SENTINEL: &str = "$__OXC_JSX_DASH__$";

/// Properties that should be set as DOM properties rather than attributes.
///
/// Keep this list aligned with `dom-expressions/src/constants` (Babel parity target).
pub static PROPERTIES: Set<&'static str> = phf_set! {
    "value",
    "checked",
    "selected",
    "muted",
};

/// Child properties that affect children
pub static CHILD_PROPERTIES: Set<&'static str> = phf_set! {
    "innerHTML",
    "textContent",
    "innerText",
    "children",
};

/// Attribute aliases (JSX name -> DOM name)
pub static ALIASES: phf::Map<&'static str, &'static str> = phf::phf_map! {
    "htmlFor" => "for",
};

/// SVG namespaces for namespaced attributes
pub static SVG_NAMESPACE: phf::Map<&'static str, &'static str> = phf::phf_map! {
    "xlink" => "http://www.w3.org/1999/xlink",
    "xml" => "http://www.w3.org/XML/1998/namespace",
};

/// Resolve an attribute name to its DOM property alias (if any)
pub fn get_prop_alias(prop: &str, tag_name: &str) -> Option<&'static str> {
    let tag = tag_name.to_ascii_uppercase();
    let tag = tag.as_str();
    match prop {
        // locked to properties
        "class" => Some("className"),

        // booleans map
        "novalidate" if tag == "FORM" => Some("noValidate"),
        "formnovalidate" if matches!(tag, "BUTTON" | "INPUT") => Some("formNoValidate"),
        "ismap" if tag == "IMG" => Some("isMap"),
        "nomodule" if tag == "SCRIPT" => Some("noModule"),
        "playsinline" if tag == "VIDEO" => Some("playsInline"),
        "readonly" if matches!(tag, "INPUT" | "TEXTAREA") => Some("readOnly"),

        "adauctionheaders" if tag == "IFRAME" => Some("adAuctionHeaders"),
        "allowfullscreen" if tag == "IFRAME" => Some("allowFullscreen"),
        "browsingtopics" if tag == "IMG" => Some("browsingTopics"),
        "defaultchecked" if tag == "INPUT" => Some("defaultChecked"),
        "defaultmuted" if matches!(tag, "AUDIO" | "VIDEO") => Some("defaultMuted"),
        "defaultselected" if tag == "OPTION" => Some("defaultSelected"),
        "disablepictureinpicture" if tag == "VIDEO" => Some("disablePictureInPicture"),
        "disableremoteplayback" if matches!(tag, "AUDIO" | "VIDEO") => {
            Some("disableRemotePlayback")
        }
        "preservespitch" if matches!(tag, "AUDIO" | "VIDEO") => Some("preservesPitch"),
        "shadowrootclonable" if tag == "TEMPLATE" => Some("shadowRootClonable"),
        "shadowrootdelegatesfocus" if tag == "TEMPLATE" => Some("shadowRootDelegatesFocus"),
        "shadowrootserializable" if tag == "TEMPLATE" => Some("shadowRootSerializable"),
        "sharedstoragewritable" if matches!(tag, "IFRAME" | "IMG") => Some("sharedStorageWritable"),
        _ => None,
    }
}

/// Events that can be delegated (bubbling events)
pub static DELEGATED_EVENTS: Set<&'static str> = phf_set! {
    "beforeinput",
    "click",
    "dblclick",
    "contextmenu",
    "focusin",
    "focusout",
    "input",
    "keydown",
    "keyup",
    "mousedown",
    "mousemove",
    "mouseout",
    "mouseover",
    "mouseup",
    "pointerdown",
    "pointermove",
    "pointerout",
    "pointerover",
    "pointerup",
    "touchend",
    "touchmove",
    "touchstart",
};

/// SVG elements
pub static SVG_ELEMENTS: Set<&'static str> = phf_set! {
    "svg",
    "animate",
    "animateMotion",
    "animateTransform",
    "circle",
    "clipPath",
    "defs",
    "desc",
    "ellipse",
    "feBlend",
    "feColorMatrix",
    "feComponentTransfer",
    "feComposite",
    "feConvolveMatrix",
    "feDiffuseLighting",
    "feDisplacementMap",
    "feDistantLight",
    "feDropShadow",
    "feFlood",
    "feFuncA",
    "feFuncB",
    "feFuncG",
    "feFuncR",
    "feGaussianBlur",
    "feImage",
    "feMerge",
    "feMergeNode",
    "feMorphology",
    "feOffset",
    "fePointLight",
    "feSpecularLighting",
    "feSpotLight",
    "feTile",
    "feTurbulence",
    "filter",
    "foreignObject",
    "g",
    "image",
    "line",
    "linearGradient",
    "marker",
    "mask",
    "metadata",
    "mpath",
    "path",
    "pattern",
    "polygon",
    "polyline",
    "radialGradient",
    "rect",
    "set",
    "stop",
    "switch",
    "symbol",
    "text",
    "textPath",
    "title",
    "tspan",
    "use",
    "view",
};

/// Inline elements used for closing-tag omission heuristics
pub static INLINE_ELEMENTS: Set<&'static str> = phf_set! {
    "a",
    "abbr",
    "acronym",
    "b",
    "bdi",
    "bdo",
    "big",
    "br",
    "button",
    "canvas",
    "cite",
    "code",
    "data",
    "datalist",
    "del",
    "dfn",
    "em",
    "embed",
    "i",
    "iframe",
    "img",
    "input",
    "ins",
    "kbd",
    "label",
    "map",
    "mark",
    "meter",
    "noscript",
    "object",
    "output",
    "picture",
    "progress",
    "q",
    "ruby",
    "s",
    "samp",
    "script",
    "select",
    "slot",
    "small",
    "span",
    "strong",
    "sub",
    "sup",
    "svg",
    "template",
    "textarea",
    "time",
    "u",
    "tt",
    "var",
    "video",
};

/// Block elements used for closing-tag omission heuristics
pub static BLOCK_ELEMENTS: Set<&'static str> = phf_set! {
    "address",
    "article",
    "aside",
    "blockquote",
    "dd",
    "details",
    "dialog",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "li",
    "main",
    "menu",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
};

/// Tags that should default to closed output when omission logic runs
pub static ALWAYS_CLOSE: Set<&'static str> = phf_set! {
    "title",
    "style",
    "a",
    "strong",
    "small",
    "b",
    "u",
    "i",
    "em",
    "s",
    "code",
    "object",
    "table",
    "button",
    "textarea",
    "select",
    "iframe",
    "script",
    "noscript",
    "template",
    "fieldset",
};

/// Void elements (self-closing)
pub static VOID_ELEMENTS: Set<&'static str> = phf_set! {
    "area",
    "base",
    "br",
    "col",
    "embed",
    "hr",
    "img",
    "input",
    "keygen",
    "link",
    "menuitem",
    "meta",
    "param",
    "source",
    "track",
    "wbr",
};

/// Solid's built-in control flow components
pub static BUILT_INS: Set<&'static str> = phf_set! {
    "For",
    "Show",
    "Switch",
    "Match",
    "Suspense",
    "SuspenseList",
    "Portal",
    "Index",
    "Dynamic",
    "ErrorBoundary",
};
