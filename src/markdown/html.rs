//! The document as a self-contained HTML page. A document can come from anyone, so the page carries no script:
//! raw HTML is kept only for the few tags Splat's own formatting writes, links keep only harmless schemes, and a
//! content security policy forbids scripts even if something slipped through.

use std::collections::VecDeque;
use std::fmt;
use std::sync::Mutex;

use comrak::adapters::{HeadingAdapter, HeadingMeta};
use std::cell::RefCell;

use comrak::nodes::{AlertType, Ast, AstNode, LineColumn, NodeAlert, NodeLink, NodeValue, Sourcepos};
use comrak::options::Plugins;
use comrak::{Arena, format_html_with_plugins, parse_document};

use super::doc::{AlertKind, Figure, diagram_kind, is_diagram, is_math_fence, options};
use super::text::{LineStarts, right_to_left};
use super::typography::{Mark, Open, Quotes, marks, smarten};
use crate::i18n::t;

/// What the page needs from outside the text.
pub struct Export<'a> {
    pub title: &'a str,
    /// The rendered formula or diagram as `<img>` element, if one is ready.
    pub figure: &'a dyn Fn(&Figure) -> Option<String>,
    /// A local image as `data:` URL, so the page needs no files beside it.
    pub embed: &'a dyn Fn(&str) -> Option<String>,
    /// A relative link as seen from the exported file.
    pub rebase: &'a dyn Fn(&str) -> String,
    /// Quotation marks for straight quotes, when typographic punctuation is on.
    pub quotes: Option<Quotes>,
    /// The web page of `@name`, `#123` or `!123`, when the document lies in a repository.
    pub reference: &'a dyn Fn(&str) -> Option<String>,
    /// How a citation key reads, such as `Doe 2020`.
    pub citation: &'a dyn Fn(&str) -> Option<String>,
}

/// HTML elements a document may embed, as GitHub allows them; any other element gives way to its content.
const KEPT_TAGS: [&str; 52] = [
    "p",
    "div",
    "span",
    "center",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "b",
    "strong",
    "i",
    "em",
    "u",
    "ins",
    "s",
    "del",
    "strike",
    "mark",
    "sup",
    "sub",
    "code",
    "kbd",
    "samp",
    "tt",
    "pre",
    "br",
    "hr",
    "img",
    "a",
    "ul",
    "ol",
    "li",
    "blockquote",
    "details",
    "summary",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "td",
    "th",
    "dl",
    "dt",
    "dd",
    "small",
    "abbr",
    "cite",
    "q",
    "figure",
];
/// Attributes kept on embedded HTML; addresses are checked separately.
const KEPT_ATTRIBUTES: [&str; 11] = [
    "align", "title", "alt", "width", "height", "colspan", "rowspan", "start", "open", "href", "src",
];
const VOID_TAGS: [&str; 3] = ["br", "hr", "img"];

pub fn to_html(text: &str, export: &Export) -> String {
    let arena = Arena::new();
    let mut opts = options();
    // Raw HTML is filtered node by node below, which the default escaping cannot do for single tags.
    opts.render.r#unsafe = true;
    let root = parse_document(&arena, text, &opts);
    let nodes: Vec<_> = root.descendants().collect();
    for node in nodes {
        rewrite(node, export);
    }
    let lines = LineStarts::new(text);
    let texts: Vec<_> = root
        .descendants()
        .filter(|n| matches!(n.data().value, NodeValue::Text(_)))
        .collect();
    let mut paragraph = None;
    let mut open = Open::default();
    for node in texts {
        let block = node.ancestors().find(|a| a.data().value.block());
        if block.map(|b| b as *const AstNode) != paragraph {
            paragraph = block.map(|b| b as *const AstNode);
            open = Open::default();
        }
        decorate(&arena, node, text, &lines, export, &mut open);
    }
    let anchors = Anchors(Mutex::new(
        super::commands::heading_anchors(&super::doc::parse(text))
            .into_iter()
            .map(|h| h.2)
            .collect(),
    ));
    let mut plugins = Plugins::default();
    plugins.render.heading_adapter = Some(&anchors);
    let mut body = String::new();
    let _ = format_html_with_plugins(root, &opts, &mut body, &plugins);
    let body = with_directions(&body);
    format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src 'self' data: https: file:; style-src 'unsafe-inline'\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n<style>{STYLE}</style>\n\
         </head>\n<body>\n<article>\n{body}</article>\n</body>\n</html>\n",
        escape(export.title)
    )
}

/// Gives every heading the anchor the document's links and its table of contents point to.
struct Anchors(Mutex<VecDeque<String>>);

impl HeadingAdapter for Anchors {
    fn enter(&self, output: &mut dyn fmt::Write, heading: &HeadingMeta, _: Option<Sourcepos>) -> fmt::Result {
        let id = self.0.lock().ok().and_then(|mut a| a.pop_front()).unwrap_or_default();
        write!(output, "<h{} id=\"{}\" dir=\"auto\">", heading.level, escape(&id))
    }

    fn exit(&self, output: &mut dyn fmt::Write, heading: &HeadingMeta) -> fmt::Result {
        writeln!(output, "</h{}>", heading.level)
    }
}

/// Paragraphs, list items and terms take the direction of their first letter, so Hebrew and Arabic read right to left.
/// Quotes, callouts, lists and tables take the direction of their text as a whole, as in the preview, set explicitly:
/// a browser resolving `dir="auto"` skips descendants that have a direction of their own. Code stays left to right.
fn with_directions(body: &str) -> String {
    const LEAVES: [&str; 4] = ["<p>", "<li>", "<dt>", "<dd>"];
    const CONTAINERS: [(&str, &str); 5] = [
        ("<blockquote", "blockquote"),
        ("<ul", "ul"),
        ("<ol", "ol"),
        ("<table", "table"),
        ("<div class=\"markdown-alert ", "div"),
    ];
    let mut out = String::with_capacity(body.len() + body.len() / 4);
    let mut at = 0;
    while let Some(i) = body[at..].find('<').map(|i| at + i) {
        out.push_str(&body[at..i]);
        let rest = &body[i..];
        if let Some(leaf) = LEAVES.iter().find(|leaf| rest.starts_with(**leaf)) {
            out.push_str(&leaf[..leaf.len() - 1]);
            out.push_str(" dir=\"auto\">");
            at = i + leaf.len();
        } else if let Some((_, name)) = CONTAINERS
            .iter()
            .find(|(open, name)| rest.starts_with(open) && opens(rest, name))
        {
            let element = &body[i..element_end(body, i, name)];
            let dir = if reads_right_to_left(element) { "rtl" } else { "ltr" };
            out.push_str(&format!("<{name} dir=\"{dir}\""));
            at = i + 1 + name.len();
        } else {
            out.push('<');
            at = i + 1;
        }
    }
    out.push_str(&body[at..]);
    out
}

/// Whether `html` starts with an opening tag named `name`, not with a longer name.
fn opens(html: &str, name: &str) -> bool {
    html.strip_prefix('<')
        .and_then(|r| r.strip_prefix(name))
        .is_some_and(|r| r.starts_with([' ', '>']))
}

/// The end of the element opening at `start`, after its closing tag, or the end of `html`.
fn element_end(html: &str, start: usize, name: &str) -> usize {
    let close = format!("</{name}>");
    let mut depth = 0;
    let mut at = start;
    while let Some(i) = html[at..].find('<').map(|i| at + i) {
        if html[i..].starts_with(&close) {
            depth -= 1;
            at = i + close.len();
            if depth == 0 {
                return at;
            }
        } else {
            depth += usize::from(opens(&html[i..], name));
            at = i + 1;
        }
    }
    html.len()
}

/// Whether the first letter of an element's text, outside tags, entities and code blocks, reads right to left. A
/// callout decides by its body, and by its title only when the body has no letters.
fn reads_right_to_left(element: &str) -> bool {
    let text = |with_title: bool| {
        let mut out = String::new();
        let mut rest = element;
        while let Some(c) = rest.chars().next() {
            let skip = if rest.starts_with("<pre") {
                rest.find("</pre>").map_or(rest.len(), |e| e + "</pre>".len())
            } else if !with_title && rest.starts_with("<p class=\"markdown-alert-title\"") {
                rest.find("</p>").map_or(rest.len(), |e| e + "</p>".len())
            } else if c == '<' {
                rest.find('>').map_or(rest.len(), |e| e + 1)
            } else if c == '&' {
                rest.find(';').map_or(1, |e| e + 1)
            } else {
                out.push(c);
                c.len_utf8()
            };
            rest = &rest[skip..];
        }
        out
    };
    right_to_left(&text(false)).or_else(|| right_to_left(&text(true))) == Some(true)
}

fn rewrite<'a>(node: &'a AstNode<'a>, export: &Export) {
    let replacement = {
        let mut data = node.data_mut();
        match &mut data.value {
            NodeValue::HtmlInline(raw) => {
                *raw = clean_tag(raw, export);
                None
            }
            NodeValue::HtmlBlock(block) => {
                // An element closed in a later block, as `</details>` after the Markdown it wraps, closes here.
                let closes: String = super::markup::stray_closes(&block.literal)
                    .into_iter()
                    .filter(|n| KEPT_TAGS.contains(&n.as_str()))
                    .map(|n| format!("</{n}>"))
                    .collect();
                block.literal = closes + &clean_html(&super::markup::parse(&block.literal), export) + "\n";
                None
            }
            NodeValue::Math(math) => (export.figure)(&Figure::Math {
                tex: &math.literal,
                display: math.display_math,
            })
            .map(NodeValue::HtmlInline),
            NodeValue::CodeBlock(code) if is_diagram(&code.info) || is_math_fence(&code.info) => {
                let figure = match diagram_kind(&code.info) {
                    Some(kind) => Figure::Diagram {
                        source: &code.literal,
                        kind,
                    },
                    None => Figure::Math {
                        tex: code.literal.strip_suffix('\n').unwrap_or(&code.literal),
                        display: true,
                    },
                };
                (export.figure)(&figure).map(|img| {
                    NodeValue::HtmlBlock(comrak::nodes::NodeHtmlBlock {
                        block_type: 6,
                        literal: format!("<p class=\"diagram\">{img}</p>\n"),
                    })
                })
            }
            NodeValue::Link(link) => {
                link.url = safe_url(&link.url, false, export);
                None
            }
            NodeValue::Image(image) => {
                image.url = (export.embed)(&image.url).unwrap_or_else(|| safe_url(&image.url, true, export));
                None
            }
            NodeValue::WikiLink(link) => {
                let target = link.url.clone();
                if link.embed && crate::preview::layout::is_image_name(&target) {
                    let url = (export.embed)(&target).unwrap_or_else(|| safe_url(&target, true, export));
                    Some(NodeValue::Image(Box::new(NodeLink {
                        url,
                        title: String::new(),
                    })))
                } else {
                    let file = if target.contains('.') {
                        target
                    } else {
                        format!("{target}.md")
                    };
                    link.url = safe_url(&file, false, export);
                    None
                }
            }
            NodeValue::Callout(callout) => {
                let (kind, github) = super::doc::callout_kind(&callout.callout_type);
                let custom = node
                    .children()
                    .find(|n| matches!(n.data().value, NodeValue::CalloutTitle))
                    .map(|n| {
                        let text = super::doc::node_text(n);
                        n.detach();
                        text
                    })
                    .filter(|t| !t.trim().is_empty());
                let title = custom.or_else(|| (!github).then(|| super::doc::capitalized(&callout.callout_type)));
                Some(NodeValue::Alert(Box::new(NodeAlert {
                    alert_type: match kind {
                        AlertKind::Note => AlertType::Note,
                        AlertKind::Tip => AlertType::Tip,
                        AlertKind::Important => AlertType::Important,
                        AlertKind::Warning => AlertType::Warning,
                        AlertKind::Caution => AlertType::Caution,
                    },
                    title: Some(title.unwrap_or_else(|| t(kind.label()).to_string())),
                    multiline: false,
                    fence_length: 0,
                    fence_offset: 0,
                })))
            }
            NodeValue::Alert(alert) => {
                if alert.title.is_none() {
                    let kind = match alert.alert_type {
                        comrak::nodes::AlertType::Note => AlertKind::Note,
                        comrak::nodes::AlertType::Tip => AlertKind::Tip,
                        comrak::nodes::AlertType::Important => AlertKind::Important,
                        comrak::nodes::AlertType::Warning => AlertKind::Warning,
                        comrak::nodes::AlertType::Caution => AlertKind::Caution,
                    };
                    alert.title = Some(t(kind.label()).to_string());
                }
                None
            }
            _ => None,
        }
    };
    if let Some(value) = replacement {
        // An alert keeps the callout's content; a replaced HTML block or code block keeps no children.
        let keeps_children = matches!(value, NodeValue::Alert(_));
        node.data_mut().value = value;
        while !keeps_children && let Some(child) = node.first_child() {
            child.detach();
        }
    }
}

/// Typographic punctuation, tags, references and citations in running text, as in the preview.
fn decorate<'a>(
    arena: &'a Arena<'a>,
    node: &'a AstNode<'a>,
    text: &str,
    lines: &LineStarts,
    export: &Export,
    open: &mut Open,
) {
    if node.ancestors().any(|a| {
        matches!(
            a.data().value,
            NodeValue::Link(_) | NodeValue::Image(_) | NodeValue::WikiLink(_) | NodeValue::Heading(_)
        )
    }) {
        return;
    }
    let (literal, start) = {
        let data = node.data();
        let NodeValue::Text(t) = &data.value else { return };
        let at = lines
            .start(data.sourcepos.start.line, data.sourcepos.start.column)
            .min(text.len());
        (t.to_string(), at)
    };
    let before = text[..text.floor_char_boundary(start)].chars().next_back();
    let shown = match export.quotes {
        Some(q) => smarten(&literal, before, q, open),
        None => literal,
    };
    let found = marks(&shown, before, true);
    let new = |value: NodeValue| {
        arena.alloc(AstNode::new(RefCell::new(Ast::new(
            value,
            LineColumn { line: 0, column: 0 },
        ))))
    };
    let mut at = 0;
    for (range, mark) in found {
        let replacement = match &mark {
            Mark::Tag(name) => Some(NodeValue::HtmlInline(format!(
                "<span class=\"tag\">#{}</span>",
                escape(name)
            ))),
            Mark::Reference(r) => (export.reference)(r)
                .map(|url| NodeValue::HtmlInline(format!("<a href=\"{}\">{}</a>", escape(&url), escape(r)))),
            Mark::Citation(keys) => {
                let names: Option<Vec<String>> = keys.iter().map(|k| (export.citation)(k)).collect();
                Some(NodeValue::HtmlInline(format!(
                    "<cite>{}</cite>",
                    escape(&names.map_or_else(|| shown[range.clone()].to_string(), |n| format!("({})", n.join("; "))))
                )))
            }
        };
        let Some(replacement) = replacement else { continue };
        if range.start > at {
            node.insert_before(new(NodeValue::Text(shown[at..range.start].to_string().into())));
        }
        node.insert_before(new(replacement));
        at = range.end;
    }
    node.data_mut().value = NodeValue::Text(shown[at..].to_string().into());
}

/// Embedded HTML with only the elements and attributes a page may show without running anything.
fn clean_html(nodes: &[super::markup::Node], export: &Export) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            super::markup::Node::Text { text, .. } => out.push_str(&escape(text)),
            super::markup::Node::Element(e) => {
                let inner = clean_html(&e.children, export);
                if !KEPT_TAGS.contains(&e.name.as_str()) {
                    out.push_str(&inner);
                    continue;
                }
                out.push_str(&open_tag(&e.name, &e.attrs, e.align(), export));
                if VOID_TAGS.contains(&e.name.as_str()) {
                    continue;
                }
                out.push_str(&inner);
                // Left open, it wraps the Markdown up to the block that closes it.
                if e.closed {
                    out.push_str(&format!("</{}>", e.name));
                }
            }
        }
    }
    out
}

fn open_tag(name: &str, attrs: &[(String, String)], align: Option<&str>, export: &Export) -> String {
    let mut tag = format!("<{name}");
    for (key, value) in attrs {
        if !KEPT_ATTRIBUTES.contains(&key.as_str()) || key == "align" {
            continue;
        }
        let value = match key.as_str() {
            "href" => safe_url(value, false, export),
            "src" => (export.embed)(value).unwrap_or_else(|| safe_url(value, true, export)),
            _ => value.clone(),
        };
        if key == "open" {
            tag.push_str(" open");
        } else {
            tag.push_str(&format!(" {key}=\"{}\"", escape(&value)));
        }
    }
    if let Some(align) = align {
        tag.push_str(&format!(" align=\"{align}\""));
    }
    tag.push('>');
    tag
}

/// A single inline tag, cleaned like embedded HTML; comments and other elements drop out.
fn clean_tag(raw: &str, export: &Export) -> String {
    let trimmed = raw.trim();
    match super::markup::read_tag(trimmed, 0) {
        Some((super::markup::Tag::Open { name, attrs, .. }, _)) if KEPT_TAGS.contains(&name.as_str()) => {
            let align = attrs
                .iter()
                .find(|(k, _)| k == "align")
                .map(|(_, v)| v.to_ascii_lowercase())
                .filter(|v| matches!(v.as_str(), "left" | "center" | "right"));
            open_tag(&name, &attrs, align.as_deref(), export)
        }
        Some((super::markup::Tag::Close { name }, _)) if KEPT_TAGS.contains(&name.as_str()) => format!("</{name}>"),
        _ => String::new(),
    }
}

/// Web and mail links, in-page anchors and relative paths stay; every other scheme, such as `javascript:` or
/// `file:`, is dropped. Images may also be embedded PNG, JPEG, GIF or WebP data.
fn safe_url(url: &str, image: bool, export: &Export) -> String {
    // Browsers drop tabs and line breaks anywhere in a URL and control characters around it, so `java&#9;script:`
    // is a scheme to them.
    let lower: String = url
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect::<String>()
        .to_ascii_lowercase();
    let scheme = lower.find(':').filter(|&i| {
        lower[..i]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    });
    match scheme.map(|i| &lower[..i]) {
        None => (export.rebase)(url),
        Some("http" | "https") => url.to_string(),
        Some("mailto") if !image => url.to_string(),
        Some("data")
            if image
                && [
                    "data:image/png;",
                    "data:image/jpeg;",
                    "data:image/gif;",
                    "data:image/webp;",
                ]
                .iter()
                .any(|p| lower.starts_with(p)) =>
        {
            url.to_string()
        }
        _ => "#".to_string(),
    }
}

/// The MIME type of image data browsers show, recognised by its content.
pub fn web_image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF8") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

pub fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64(bytes))
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const STYLE: &str = "\
body { font: 16px/1.55 -apple-system, BlinkMacSystemFont, 'Helvetica Neue', sans-serif; color: #1d1d1f; margin: 0; }
article { max-width: 46em; margin: 0 auto; padding: 2em 1.5em 4em; }
h1, h2, h3 { line-height: 1.25; }
a { color: #0066cc; }
code, pre { font: 0.9em ui-monospace, Menlo, monospace; background: #f3f3f5; border-radius: 4px; }
code { padding: 0.1em 0.3em; }
pre { padding: 0.8em 1em; overflow-x: auto; }
pre code { padding: 0; background: none; }
blockquote { margin: 0; padding: 0 1em; border-inline-start: 3px solid #d2d2d7; color: #555; }
table { border-collapse: collapse; }
th, td { border: 1px solid #d2d2d7; padding: 0.35em 0.7em; }
th { background: #f5f5f7; }
img { max-width: 100%; }
mark { background: #fff3a3; }
hr { border: 0; border-top: 1px solid #d2d2d7; }
.markdown-alert { border-inline-start: 4px solid #0969da; padding: 0.2em 1em; margin: 1em 0; background: #f6f8fa; }
.markdown-alert-title { font-weight: 600; }
.markdown-alert-tip { border-color: #1a7f37; }
.markdown-alert-important { border-color: #8250df; }
.markdown-alert-warning { border-color: #9a6700; }
.markdown-alert-caution { border-color: #cf222e; }
.diagram { text-align: center; }
.spoiler { background: currentColor; border-radius: 3px; }
.tag { color: #0066cc; background: #0066cc1f; border-radius: 4px; padding: 0 0.2em; }
cite { font-style: normal; color: #7a5c2e; }
.spoiler:hover { background: none; }
dt { font-weight: 600; }
dd { margin: 0 0 0.6em; margin-inline-start: 1.5em; }
dd > p { margin: 0.2em 0; }
@media (prefers-color-scheme: dark) {
  body { background: #1e1e1e; color: #e8e8ed; }
  a { color: #4ea1ff; }
  code, pre, th, .markdown-alert { background: #2a2a2d; }
  blockquote { color: #aaa; border-color: #48484a; }
  th, td { border-color: #48484a; }
  mark { background: #6b5d00; color: inherit; }
  img.math, .diagram img { filter: invert(1) hue-rotate(180deg); }
}
";

#[cfg(test)]
mod tests {
    use super::*;

    fn export(text: &str) -> String {
        to_html(
            text,
            &Export {
                title: "T <1>",
                figure: &|f| match f {
                    Figure::Math { tex, .. } => Some(format!("<img class=\"math\" alt=\"{}\">", escape(tex))),
                    Figure::Diagram { .. } => Some("<img class=\"diagram-image\">".into()),
                },
                embed: &|url| (url == "img/eingebettet.png").then(|| data_url("image/png", b"PNG")),
                rebase: &|p| format!("../docs/{p}"),
                quotes: Some(Quotes {
                    open: '„',
                    close: '“',
                    open_single: '‚',
                    close_single: '‘',
                }),
                reference: &|r| (r == "#12").then(|| "https://example.com/issues/12".to_string()),
                citation: &|k| (k == "doe").then(|| "Doe 2020".to_string()),
            },
        )
    }

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xFF, 0xFE, 0xFD, 0x00]), "//79AA==");
        assert_eq!(web_image_type(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(web_image_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(web_image_type(b"II*\0"), None);
    }

    #[test]
    fn exported_pages_carry_no_script() {
        let page = export(
            "<script>alert(1)</script>\n\nText <img src=x onerror=alert(2)> and <mark>hell</mark>\n\n\
             [a](javascript:alert(3)) [b](https://example.com) [c](file:///etc/passwd)",
        );
        assert!(!page.contains("<script>"), "{page}");
        assert!(!page.contains("<img src=x"), "{page}");
        assert!(!page.contains("javascript:"), "{page}");
        assert!(!page.contains("file:///etc"), "{page}");
        assert!(page.contains("<mark>hell</mark>"));
        assert!(page.contains("href=\"https://example.com\""));
        assert!(page.contains("default-src 'none'"));
        assert!(page.contains("<title>T &lt;1&gt;</title>"));
    }

    #[test]
    fn extended_syntax_reaches_the_page() {
        let page = export(
            "# Titel {#eigene-id}\n\n## Zweiter\n\n==hell== x^2^ :rocket: ||geheim|| %%still%% [[Seite|T]] ![[img/eingebettet.png]]\n\n\
             Begriff\n\n: Erklärung\n\n> [!bug]- Absturz\n> Inhalt\n\n```math\nx^2\n```\n",
        );
        assert!(page.contains("<h1 id=\"eigene-id\" dir=\"auto\">Titel</h1>"), "{page}");
        assert!(page.contains("<h2 id=\"zweiter\" dir=\"auto\">Zweiter</h2>"), "{page}");
        assert!(page.contains("<mark>hell</mark>") && page.contains("<sup>2</sup>") && page.contains("🚀"));
        assert!(page.contains("<span class=\"spoiler\">geheim</span>") && !page.contains("still"));
        assert!(page.contains("href=\"../docs/Seite.md\""), "{page}");
        assert!(page.contains("src=\"data:image/png;base64,UE5H\""), "{page}");
        assert!(page.contains("<dt dir=\"auto\">Begriff</dt>"));
        assert!(
            page.contains("markdown-alert-caution") && page.contains("Absturz"),
            "{page}"
        );
        assert!(page.contains("<img class=\"math\" alt=\"x^2\">"), "{page}");
    }

    #[test]
    fn blocks_take_the_direction_of_their_text() {
        let page = export(
            "פסקה\n\n- אחד\n\n> \"ציטוט\" & עוד\n>\n> > English\n\n| א | ב |\n|---|---|\n| 1 | 2 |\n\n\
             3. שלוש\n\n> [!NOTE] כותרת\n\n> [!NOTE] Title\n> גוף\n\n```\nכתוב\n```\n",
        );
        for tag in [
            "<p dir=\"auto\">פסקה",
            "<ul dir=\"rtl\">\n<li dir=\"auto\">אחד",
            "<blockquote dir=\"rtl\">\n<p dir=\"auto\">",
            "<blockquote dir=\"ltr\">\n<p dir=\"auto\">English",
            "<table dir=\"rtl\">",
            "<ol dir=\"rtl\" start=\"3\">",
        ] {
            assert!(page.contains(tag), "{tag}: {page}");
        }
        assert_eq!(
            page.matches("<div dir=\"rtl\" class=\"markdown-alert ").count(),
            2,
            "{page}"
        );
        assert!(
            page.contains("<pre><code>כתוב") && page.contains("border-inline-start"),
            "{page}"
        );
    }

    #[test]
    fn embedded_html_keeps_layout_but_loses_everything_active() {
        let page = export(
            "<p align=\"center\" onclick=\"x()\" style=\"color:red\">\n<img src=\"img/eingebettet.png\" width=\"80\" onerror=\"y()\">\n<a href=\"javascript:z()\">böse</a> <a href=\"https://ok.de\">gut</a>\n</p>\n\n\
             <details>\n<summary>Mehr</summary>\n\nInhalt\n\n</details>\n\nText <kbd>⌘</kbd> <span onmouseover=\"q()\">s</span> <iframe src=\"https://x\"></iframe>\n",
        );
        assert!(page.contains("<p align=\"center\">"), "{page}");
        assert!(
            page.contains("<img src=\"data:image/png;base64,UE5H\" width=\"80\">"),
            "{page}"
        );
        assert!(
            page.contains("<a href=\"#\">böse</a>") && page.contains("<a href=\"https://ok.de\">gut</a>"),
            "{page}"
        );
        for bad in ["onclick", "onerror", "onmouseover", "javascript", "style=", "iframe"] {
            assert!(!page.contains(bad), "{bad}: {page}");
        }
        let hidden = export("<a href=\"java&#9;script:x()\">a</a> <a href=\"&#1;javascript:y()\">b</a>\n");
        assert_eq!(hidden.matches("<a href=\"#\">").count(), 2, "{hidden}");
        let details = page.find("<details>\n<summary>Mehr</summary>").expect(&page);
        let content = page.find("<p dir=\"auto\">Inhalt</p>").unwrap();
        let close = page.find("</details>").unwrap();
        assert!(details < content && content < close, "{page}");
        assert!(page.contains("<kbd>⌘</kbd>"));
    }

    #[test]
    fn running_text_gets_typography_tags_references_and_citations() {
        let page = export("Er sagte \"Ja\" -- siehe #idee, #12 und [@doe].\n\n`\"code\"`\n");
        assert!(page.contains("Er sagte „Ja“ – siehe"), "{page}");
        assert!(page.contains("<span class=\"tag\">#idee</span>"), "{page}");
        assert!(
            page.contains("<a href=\"https://example.com/issues/12\">#12</a>"),
            "{page}"
        );
        assert!(page.contains("<cite>(Doe 2020)</cite>"), "{page}");
        assert!(page.contains("<code>&quot;code&quot;</code>"), "{page}");
    }

    #[test]
    fn figures_paths_and_alerts_are_prepared_for_the_page() {
        let page = export(
            "---\ntitle: x\n---\n\nFormel $E = mc^2$\n\n```mermaid\nflowchart LR\n  A --> B\n```\n\n\
             ![Bild](img/a.png) ![Eingebettet](img/eingebettet.png)\n\n[Link](notes/b.md)\n\n> [!NOTE]\n> Inhalt",
        );
        assert!(page.contains("<img class=\"math\" alt=\"E = mc^2\">"), "{page}");
        assert!(
            page.contains("<p class=\"diagram\"><img class=\"diagram-image\"></p>"),
            "{page}"
        );
        assert!(!page.contains("flowchart"), "{page}");
        assert!(page.contains("src=\"../docs/img/a.png\""), "{page}");
        assert!(page.contains("src=\"data:image/png;base64,UE5H\""), "{page}");
        assert!(page.contains("href=\"../docs/notes/b.md\""), "{page}");
        assert!(page.contains(crate::i18n::t("Hinweis")), "{page}");
        assert!(!page.contains("title: x"), "front matter stays out: {page}");
    }
}
