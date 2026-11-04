//! A forgiving reader for the HTML that Markdown documents embed, as READMEs do for centered logos, collapsible
//! sections and sized pictures. It keeps where every element and text stands in the source, so the preview can show
//! the HTML rendered and still map it back. Scripts, styles, frames and forms are left out entirely.

use std::collections::HashMap;
use std::ops::Range;

use super::doc::Align;

/// An element or text, with byte ranges into the HTML it was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    /// Text with entities decoded.
    Text {
        text: String,
        range: Range<usize>,
    },
}

impl Node {
    pub fn element(&self) -> Option<&Element> {
        match self {
            Node::Element(e) => Some(e),
            Node::Text { .. } => None,
        }
    }
}

/// The first element named `name` among `nodes` and what they hold.
pub fn find<'n>(nodes: &'n [Node], name: &str) -> Option<&'n Element> {
    nodes.iter().filter_map(Node::element).find_map(|e| {
        if e.name == name {
            Some(e)
        } else {
            find(&e.children, name)
        }
    })
}

/// The text inside `nodes` as it stands, with a line for every `<br>`.
pub fn raw_text(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Text { text, .. } => out.push_str(text),
            Node::Element(e) if e.name == "br" => out.push('\n'),
            Node::Element(e) => raw_text(&e.children, out),
        }
    }
}

/// What an element is to the preview and the exports: everything said about elements by name, in one place.
#[derive(Clone, Copy, Default)]
struct ElementTraits {
    /// It has no content and no closing tag.
    void: bool,
    /// Its opening closes an open `<p>`, as browsers do.
    block: bool,
    /// It makes paragraphs of its content, whatever else it means to a browser.
    paragraphs: bool,
    /// An exported page keeps it, as GitHub allows it; any other element gives way to its content.
    kept: bool,
}

fn traits(name: &str) -> ElementTraits {
    let (void, block, paragraphs, kept) = match name {
        "br" | "img" => (true, false, false, true),
        "hr" => (true, true, false, true),
        "input" | "source" | "meta" | "link" | "wbr" | "col" | "area" => (true, false, false, false),
        "p" | "div" | "center" | "summary" => (false, true, true, true),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "li" | "table" | "tr" | "td" | "th" | "blockquote"
        | "pre" | "details" | "dl" | "dt" | "dd" => (false, true, false, true),
        "section" | "article" | "header" | "footer" | "main" | "nav" | "aside" | "figure" | "figcaption"
        | "address" => (false, false, true, true),
        "span" | "b" | "strong" | "i" | "em" | "u" | "ins" | "s" | "del" | "strike" | "mark" | "sup" | "sub"
        | "code" | "kbd" | "samp" | "tt" | "a" | "thead" | "tbody" | "tfoot" | "small" | "abbr" | "cite" | "q"
        | "dfn" | "var" => (false, false, false, true),
        _ => (false, false, false, false),
    };
    ElementTraits {
        void,
        block,
        paragraphs,
        kept,
    }
}

pub fn is_void(name: &str) -> bool {
    traits(name).void
}

pub fn makes_paragraphs(name: &str) -> bool {
    traits(name).paragraphs
}

pub fn kept_in_exports(name: &str) -> bool {
    traits(name).kept
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    /// The opening tag.
    pub tag: Range<usize>,
    /// From the opening tag to the end of the closing one, or of the content when it is missing.
    pub range: Range<usize>,
    /// Whether the source closes the element; a `<details>` left open wraps the Markdown that follows.
    pub closed: bool,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// `align="center"` or `style="text-align: center"`.
    pub fn align(&self) -> Option<Align> {
        let value = self.attr("align").map(str::to_ascii_lowercase).or_else(|| {
            let style = self.attr("style")?.to_ascii_lowercase();
            let at = style.find("text-align")?;
            let rest = style[at + "text-align".len()..].trim_start().strip_prefix(':')?;
            Some(rest.split(';').next()?.trim().to_string())
        })?;
        match value.as_str() {
            "center" | "middle" => Some(Align::Center),
            "right" => Some(Align::Right),
            "left" => Some(Align::Left),
            _ => None,
        }
    }

    /// The rows of a table, with whether each stands in its head.
    pub fn rows(&self) -> Vec<(&Element, bool)> {
        let mut rows = Vec::new();
        for child in self.children.iter().filter_map(Node::element) {
            match child.name.as_str() {
                "tr" => rows.push((child, false)),
                "thead" | "tbody" | "tfoot" => rows.extend(
                    child
                        .children
                        .iter()
                        .filter_map(Node::element)
                        .filter(|r| r.name == "tr")
                        .map(|r| (r, child.name == "thead")),
                ),
                _ => {}
            }
        }
        rows
    }

    /// The cells of a table row.
    pub fn cells(&self) -> impl Iterator<Item = &Element> {
        self.children
            .iter()
            .filter_map(Node::element)
            .filter(|c| c.name == "td" || c.name == "th")
    }

    /// How many columns a table cell spans, within what a table can take.
    pub fn span(&self) -> u32 {
        self.pixels("colspan").unwrap_or(1).clamp(1, 64)
    }

    /// A size attribute in pixels; `width="50%"` gives `None`.
    pub fn pixels(&self, name: &str) -> Option<u32> {
        pixels(self.attr(name)?)
    }
}

/// A size in pixels as HTML writes it.
pub fn pixels(value: &str) -> Option<u32> {
    let v = value.trim().trim_end_matches("px");
    v.parse::<f64>().ok().filter(|n| *n > 0.0 && *n < 100_000.0).map(|n| {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "just checked to fit"
        )]
        let pixels = n as u32;
        pixels
    })
}

/// Elements dropped with everything inside them: they run code, load pages or ask for input.
pub const DROPPED: [&str; 14] = [
    "script", "style", "iframe", "frame", "object", "embed", "form", "input", "button", "select", "textarea",
    "template", "noscript", "applet",
];
/// Elements nested deeper give way to their content: every consumer of the tree recurses, and a few kilobytes of
/// nested tags would otherwise overflow the stack.
const MAX_DEPTH: usize = 64;

pub fn parse(html: &str) -> Vec<Node> {
    let mut stack: Vec<Element> = vec![Element {
        name: String::new(),
        attrs: Vec::new(),
        children: Vec::new(),
        tag: 0..0,
        range: 0..html.len(),
        closed: true,
    }];
    let mut i = 0;
    while i < html.len() {
        if html[i..].starts_with("<!--") {
            i = html[i + 4..].find("-->").map_or(html.len(), |e| i + 4 + e + 3);
            continue;
        }
        if html.as_bytes()[i] == b'<'
            && let Some((tag, end)) = read_tag(html, i)
        {
            i = match tag {
                Tag::Open {
                    name,
                    attrs,
                    self_closing,
                } => open_element(&mut stack, html, name, attrs, self_closing, i..end),
                Tag::Close { name } => {
                    close_element(&mut stack, &name, i..end);
                    end
                }
            };
            continue;
        }
        let step = html[i..].chars().next().map_or(1, char::len_utf8);
        let next = html[i + step..].find('<').map_or(html.len(), |n| i + step + n);
        push_child(
            &mut stack,
            Node::Text {
                text: decode(&html[i..next]),
                range: i..next,
            },
        );
        i = next;
    }
    while stack.len() > 1 {
        pop(&mut stack, html.len(), false);
    }
    stack.pop().map(|root| root.children).unwrap_or_default()
}

/// Takes in an opening tag standing at `tag`; where reading goes on.
fn open_element(
    stack: &mut Vec<Element>,
    html: &str,
    name: String,
    attrs: Vec<(String, String)>,
    self_closing: bool,
    tag: Range<usize>,
) -> usize {
    let end = tag.end;
    let empty = self_closing || is_void(&name);
    if DROPPED.contains(&name.as_str()) {
        return if empty { end } else { skip_to_close(html, end, &name) };
    }
    if traits(&name).block {
        close_implicitly(stack, &name, tag.start);
    }
    let element = Element {
        name,
        attrs,
        children: Vec::new(),
        tag: tag.clone(),
        range: tag,
        closed: empty,
    };
    if empty {
        push_child(stack, Node::Element(element));
    } else if stack.len() <= MAX_DEPTH {
        stack.push(element);
    }
    end
}

/// Takes in a closing tag: it ends the innermost element of its name and what that holds; without one, it means nothing.
fn close_element(stack: &mut Vec<Element>, name: &str, tag: Range<usize>) {
    if let Some(at) = stack.iter().rposition(|e| e.name == name)
        && at > 0
    {
        while stack.len() > at + 1 {
            pop(stack, tag.start, false);
        }
        pop(stack, tag.end, true);
    }
}

fn push_child(stack: &mut [Element], node: Node) {
    if let Some(top) = stack.last_mut() {
        top.children.push(node);
    }
}

fn pop(stack: &mut Vec<Element>, end: usize, closed: bool) {
    if let Some(mut e) = stack.pop() {
        e.range.end = end.max(e.tag.end);
        e.closed = closed;
        push_child(stack, Node::Element(e));
    }
}

/// A new `<li>` ends the open one, a new row or cell the open one and all it holds, a block element an open `<p>`.
fn close_implicitly(stack: &mut Vec<Element>, name: &str, at: usize) {
    let ends = |open: &str| match name {
        "li" => open == "li" || open == "p",
        "tr" => matches!(open, "tr" | "td" | "th" | "p"),
        "td" | "th" => matches!(open, "td" | "th" | "p"),
        "dt" | "dd" => matches!(open, "dt" | "dd" | "p"),
        _ => open == "p",
    };
    let barrier = |open: &str| matches!(open, "ul" | "ol" | "table" | "dl" | "details" | "blockquote" | "div");
    let inside = stack.iter().rposition(|e| barrier(&e.name)).map_or(1, |b| b + 1);
    if let Some(pos) = stack[inside..].iter().position(|e| ends(&e.name)) {
        while stack.len() > inside + pos {
            pop(stack, at, false);
        }
    }
}

pub enum Tag {
    Open {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },
    Close {
        name: String,
    },
}

/// The tag at `start`, which holds a `<`, and the offset after it.
pub fn read_tag(html: &str, start: usize) -> Option<(Tag, usize)> {
    read_tag_noting(html, start, |_, _| {})
}

/// Where the attribute `key`, in lower case, stands in `tag`, a whole tag: from its name to the end of its value.
pub fn attribute_range(tag: &str, key: &str) -> Option<Range<usize>> {
    let mut found = None;
    read_tag_noting(tag, 0, |name, range| {
        if name == key {
            found.get_or_insert(range);
        }
    })?;
    found
}

/// `read_tag` that tells `note` the name and place of every attribute it reads.
fn read_tag_noting(html: &str, start: usize, mut note: impl FnMut(&str, Range<usize>)) -> Option<(Tag, usize)> {
    let rest = &html[start + 1..];
    let (closing, rest, offset) = match rest.strip_prefix('/') {
        Some(r) => (true, r, start + 2),
        None => (false, rest, start + 1),
    };
    let name_len = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
        .count();
    if name_len == 0 || !rest.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let name = rest[..name_len].to_ascii_lowercase();
    let mut i = offset + name_len;
    let mut attrs = Vec::new();
    let finished = |attrs, self_closing: bool, name| {
        if closing {
            Tag::Close { name }
        } else {
            Tag::Open {
                name,
                attrs,
                self_closing,
            }
        }
    };
    loop {
        i = behind_spaces(html, i);
        match html.as_bytes().get(i)? {
            b'>' => return Some((finished(attrs, false, name), i + 1)),
            b'/' if html[i + 1..].starts_with('>') => return Some((finished(attrs, true, name), i + 2)),
            // Outside a quoted value this starts the next tag; going on would read every tag of a text without an end
            // to its end, once per tag.
            b'<' => return None,
            _ => {
                let from = i;
                match read_attribute(html, &mut i) {
                    Some(attribute) => {
                        note(&attribute.0, from..i);
                        attrs.push(attribute);
                    }
                    None => i += 1,
                }
            }
        }
    }
}

fn behind_spaces(html: &str, mut i: usize) -> usize {
    while html.as_bytes().get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

/// The attribute at `i`, with `i` moved behind it; `None` if no name stands there.
fn read_attribute(html: &str, i: &mut usize) -> Option<(String, String)> {
    let key_len = html[*i..]
        .bytes()
        .take_while(|b| !b.is_ascii_whitespace() && !matches!(b, b'=' | b'>' | b'/' | b'<'))
        .count();
    if key_len == 0 {
        return None;
    }
    let key = html[*i..*i + key_len].to_ascii_lowercase();
    *i += key_len;
    let behind_key = behind_spaces(html, *i);
    let mut value = String::new();
    if html.as_bytes().get(behind_key) == Some(&b'=') {
        *i = behind_spaces(html, behind_key + 1);
        match html.as_bytes().get(*i) {
            Some(&q @ (b'"' | b'\'')) => {
                let end = html[*i + 1..].find(q as char).map_or(html.len(), |e| *i + 1 + e);
                value = decode(&html[*i + 1..end]);
                *i = (end + 1).min(html.len());
            }
            _ => {
                let len = html[*i..]
                    .bytes()
                    .take_while(|b| !b.is_ascii_whitespace() && !matches!(b, b'>' | b'<'))
                    .count();
                value = decode(&html[*i..*i + len]);
                *i += len;
            }
        }
    }
    Some((key, value))
}

/// The offset after the closing tag of `name`, searched from `from`.
fn skip_to_close(html: &str, from: usize, name: &str) -> usize {
    let close = format!("</{name}");
    html[from..]
        .match_indices('<')
        .map(|(at, _)| from + at)
        .find(|&at| {
            html.as_bytes()[at..]
                .get(..close.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(close.as_bytes()))
        })
        .and_then(|at| html[at..].find('>').map(|e| at + e + 1))
        .unwrap_or(html.len())
}

/// Text with HTML entities decoded.
pub fn decode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        match super::text::decode_entity(rest) {
            Some((decoded, len)) => {
                out.push_str(&decoded);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The elements open at the end of `nodes`, outermost first: a block that opens `<details>` or `<div>` without
/// closing it wraps the Markdown up to the block that does.
pub fn open_at_end(nodes: &[Node]) -> Vec<&Element> {
    let mut out = Vec::new();
    let mut level = nodes;
    while let Some(Node::Element(e)) = level.iter().rev().find(|n| match n {
        Node::Element(_) => true,
        Node::Text { text, .. } => !text.trim().is_empty(),
    }) {
        if e.closed {
            break;
        }
        out.push(e);
        level = &e.children;
    }
    out
}

/// Elements still open, by name. A closing tag without an opening one would otherwise look through all of them, and a
/// text of such tags would take quadratic time.
pub struct OpenElements<T> {
    stack: Vec<(String, T)>,
    counts: HashMap<String, usize>,
}

impl<T> Default for OpenElements<T> {
    fn default() -> Self {
        OpenElements {
            stack: Vec::new(),
            counts: HashMap::new(),
        }
    }
}

impl<T> OpenElements<T> {
    pub fn open(&mut self, name: String, value: T) {
        *self.counts.entry(name.clone()).or_default() += 1;
        self.stack.push((name, value));
    }

    /// Closes the innermost element called `name` and what it holds; whether there was one.
    pub fn close(&mut self, name: &str) -> bool {
        if self.counts.get(name).is_none_or(|n| *n == 0) {
            return false;
        }
        while let Some((top, _)) = self.stack.pop() {
            if let Some(n) = self.counts.get_mut(&top) {
                *n -= 1;
            }
            if top == name {
                break;
            }
        }
        true
    }

    pub fn innermost(&self) -> Option<&T> {
        self.stack.last().map(|(_, value)| value)
    }
}

/// The names of the elements a block closes without opening them, as `</details>` after the wrapped Markdown.
pub fn stray_closes(html: &str) -> Vec<String> {
    let mut open = OpenElements::default();
    let mut stray = Vec::new();
    let mut i = 0;
    while let Some(at) = html[i..].find('<') {
        let start = i + at;
        if html[start..].starts_with("<!--") {
            i = html[start..].find("-->").map_or(html.len(), |e| start + e + 3);
            continue;
        }
        match read_tag(html, start) {
            Some((Tag::Open { name, self_closing, .. }, end)) => {
                if !self_closing && !is_void(&name) {
                    open.open(name, ());
                }
                i = end;
            }
            Some((Tag::Close { name }, end)) => {
                if !open.close(&name) {
                    stray.push(name);
                }
                i = end;
            }
            None => i = start + 1,
        }
    }
    stray
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attribute_is_found_by_its_name_and_not_in_a_value() {
        let at = |tag: &'static str, key: &str| attribute_range(tag, key).map(|range| &tag[range]);
        let tag = "<details title=\"open\" OPEN = 'a b' c >";
        assert_eq!(at(tag, "open"), Some("OPEN = 'a b'"));
        assert_eq!(at(tag, "c"), Some("c"), "without what stands behind it");
        assert_eq!(at("<details a/b open>", "open"), Some("open"), "as `read_tag` reads it");
        assert_eq!(at("<details>", "open"), None);
        assert_eq!(at("<details open", "open"), None, "no whole tag");
    }

    #[test]
    fn a_comment_closes_nothing() {
        assert!(stray_closes("<!-- </details> -->x").is_empty());
        assert_eq!(stray_closes("</details>"), ["details"]);
    }

    #[test]
    fn elements_are_said_to_be_one_thing_in_one_place() {
        for name in ["b", "em", "del", "mark", "sup", "sub", "kbd", "var", "dfn", "cite", "u"] {
            assert!(
                crate::style::styled_element(name).is_some() && kept_in_exports(name),
                "{name} looks different in the preview and must in a page too"
            );
        }
        for name in ["section", "div", "p", "summary"] {
            assert!(makes_paragraphs(name) && kept_in_exports(name), "{name}");
        }
        assert!(is_void("br") && !is_void("div"));
        assert!(!kept_in_exports("script"));
    }

    #[test]
    fn a_tag_ends_where_the_next_one_starts() {
        assert!(read_tag("<div <div>", 0).is_none());
        assert!(read_tag("<div a<b>", 0).is_none());
        assert!(read_tag("<div a=<b>", 0).is_none());
        assert!(
            read_tag("<div title=\"a<b\">", 0).is_some(),
            "inside quotes it is just text"
        );
    }

    #[test]
    fn content_up_to_a_closing_tag_is_dropped_whatever_its_case() {
        assert_eq!(skip_to_close("x</STYLE >y", 1, "style"), 10);
        assert_eq!(skip_to_close("ä</style", 2, "style"), "ä</style".len());
        assert_eq!(skip_to_close("nichts", 0, "style"), 6);
    }

    #[test]
    fn named_references_in_html_decode_like_those_in_markdown() {
        assert_eq!(decode("Caf&eacute; &unbekannt; &amp;"), "Café &unbekannt; &");
    }

    #[test]
    fn nesting_ends_at_the_limit_and_keeps_the_text() {
        let html = "<div>".repeat(10 * MAX_DEPTH) + "Text" + &"</div>".repeat(10 * MAX_DEPTH);
        fn depth(nodes: &[Node]) -> usize {
            nodes
                .iter()
                .map(|n| match n {
                    Node::Element(e) => 1 + depth(&e.children),
                    Node::Text { .. } => 0,
                })
                .max()
                .unwrap_or(0)
        }
        fn text(nodes: &[Node]) -> String {
            nodes
                .iter()
                .map(|n| match n {
                    Node::Element(e) => text(&e.children),
                    Node::Text { text, .. } => text.clone(),
                })
                .collect()
        }
        let nodes = parse(&html);
        assert_eq!(depth(&nodes), MAX_DEPTH);
        assert_eq!(text(&nodes), "Text");
    }

    fn element(nodes: &[Node], n: usize) -> &Element {
        match nodes.iter().filter(|n| matches!(n, Node::Element(_))).nth(n) {
            Some(Node::Element(e)) => e,
            _ => panic!("{nodes:?}"),
        }
    }

    #[test]
    fn readme_html_reads_into_a_tree_with_source_ranges() {
        let html = "<p align=\"center\">\n  <img src=\"logo.png\" width=200 alt='Logo &amp; Co'>\n  <br/>Text &lt;b&gt;\n</p>\n<!-- hidden --><script>alert(1)</script>";
        let nodes = parse(html);
        let p = element(&nodes, 0);
        assert_eq!((p.name.as_str(), p.align(), p.closed), ("p", Some(Align::Center), true));
        let img = element(&p.children, 0);
        assert_eq!(
            (img.attr("src"), img.pixels("width"), img.attr("alt")),
            (Some("logo.png"), Some(200), Some("Logo & Co"))
        );
        assert_eq!(
            &html[img.tag.clone()],
            "<img src=\"logo.png\" width=200 alt='Logo &amp; Co'>"
        );
        let text = p.children.iter().find_map(|n| match n {
            Node::Text { text, range } if text.contains("Text") => Some((text.trim(), &html[range.clone()])),
            _ => None,
        });
        assert_eq!(text, Some(("Text <b>", "Text &lt;b&gt;\n")));
        assert_eq!(
            nodes.iter().filter(|n| matches!(n, Node::Element(_))).count(),
            1,
            "comment and script are gone"
        );
    }

    #[test]
    fn unclosed_wrappers_and_implicit_ends() {
        let nodes = parse("<details open>\n<summary>Mehr</summary>");
        let open = open_at_end(&nodes);
        assert_eq!(open.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["details"]);
        assert_eq!(open[0].attr("open"), Some(""));
        assert_eq!(stray_closes("</details>\n"), ["details"]);
        assert!(stray_closes("<details></details>").is_empty());
        let list = parse("<ul><li>a<li>b</ul>");
        assert_eq!(element(&list, 0).children.len(), 2);
    }

    #[test]
    fn raw_text_reads_through_elements_and_breaks_lines() {
        let mut out = String::new();
        raw_text(&parse("a<br>b<i>c</i>"), &mut out);
        assert_eq!(out, "a\nbc");
    }

    #[test]
    fn alignment_comes_from_the_attribute_or_the_style() {
        let align = |html: &str| element(&parse(html), 0).align();
        assert_eq!(align("<p align=\"left\">"), Some(Align::Left));
        assert_eq!(align("<p align=\"middle\">"), Some(Align::Center));
        assert_eq!(align("<p align=\"justify\">"), None);
        assert_eq!(
            align("<p style=\"color: red; text-align : right; margin: 0\">"),
            Some(Align::Right)
        );
        assert_eq!(align("<p style=\"text-align:center\">"), Some(Align::Center));
    }

    #[test]
    fn a_size_in_pixels_lies_between_nothing_and_absurd() {
        assert_eq!(pixels("10"), Some(10));
        assert_eq!(pixels(" 10px "), Some(10));
        assert_eq!(pixels("0"), None);
        assert_eq!(pixels("-5"), None);
        assert_eq!(pixels("99999"), Some(99_999));
        assert_eq!(pixels("100000"), None);
    }

    fn only_text(nodes: &[Node]) -> String {
        let mut out = String::new();
        raw_text(nodes, &mut out);
        out
    }

    #[test]
    fn what_follows_a_comment_or_a_dropped_element_stays() {
        assert_eq!(only_text(&parse("x<!--a-->b")), "xb");
        assert_eq!(only_text(&parse("<!-- a -->b")), "b");
        assert_eq!(only_text(&parse("x<script/>y")), "xy", "nothing to skip to");
        assert_eq!(only_text(&parse("x<input>y")), "xy", "no closing tag to look for");
        assert_eq!(only_text(&parse("x<style>aaaaaaaa</style>y")), "xy");
    }

    #[test]
    fn an_element_without_content_counts_as_closed() {
        assert!(element(&parse("<br>"), 0).closed);
        assert!(element(&parse("<hr/>"), 0).closed);
    }

    #[test]
    fn a_closing_tag_also_closes_what_is_open_inside() {
        let nodes = parse("<b><i>x</b>y");
        let b = element(&nodes, 0);
        assert!(b.closed);
        assert!(!element(&b.children, 0).closed);
        assert_eq!(only_text(std::slice::from_ref(nodes.last().unwrap())), "y", "{nodes:?}");
    }

    fn names(nodes: &[Node]) -> Vec<&str> {
        nodes
            .iter()
            .filter_map(|n| match n {
                Node::Element(e) => Some(e.name.as_str()),
                Node::Text { .. } => None,
            })
            .collect()
    }

    #[test]
    fn a_new_row_cell_or_term_ends_the_open_one() {
        let nodes = parse("<table><tr><td>a<td>b<tr><th>c</table>");
        let table = element(&nodes, 0);
        assert_eq!(names(&table.children), ["tr", "tr"]);
        assert_eq!(names(&element(&table.children, 0).children), ["td", "td"]);
        let nodes = parse("<dl><dt>a<dd>b<dt>c</dl>");
        assert_eq!(names(&element(&nodes, 0).children), ["dt", "dd", "dt"]);
        let nodes = parse("<ul><li><p>a<li>b</ul>");
        assert_eq!(names(&element(&nodes, 0).children), ["li", "li"]);
        let nodes = parse("<table><tr><td>a<table><tr><td>b</table>c<td>d</table>");
        let row = element(&element(&nodes, 0).children, 0);
        assert_eq!(
            names(&row.children),
            ["td", "td"],
            "a table inside a cell has its own rows"
        );
        let nodes = parse("<ul><li>a<ul><li>b<li>c</ul>d<li>e</ul>");
        let items = &element(&nodes, 0).children;
        assert_eq!(names(items), ["li", "li"]);
        assert_eq!(
            names(&element(&element(items, 0).children, 0).children),
            ["li", "li"],
            "a list inside an item too"
        );
    }

    fn opening(html: &str, start: usize) -> (String, Vec<(String, String)>, bool) {
        match read_tag(html, start) {
            Some((
                Tag::Open {
                    name,
                    attrs,
                    self_closing,
                },
                _,
            )) => (name, attrs, self_closing),
            _ => panic!("no opening tag at {start} in {html}"),
        }
    }

    fn attribute(key: &str, value: &str) -> (String, String) {
        (key.to_string(), value.to_string())
    }

    #[test]
    fn a_tag_reads_with_or_without_quotes_and_spaces() {
        assert_eq!(opening("<b>", 0), ("b".to_string(), Vec::new(), false));
        assert_eq!(opening("x<b>", 1), ("b".to_string(), Vec::new(), false));
        assert_eq!(opening("<br/>", 0), ("br".to_string(), Vec::new(), true));
        assert_eq!(opening("<a b=\"c\" d>", 0).1, [attribute("b", "c"), attribute("d", "")]);
        assert_eq!(
            opening("<a href = \"u\" x= y>", 0).1,
            [attribute("href", "u"), attribute("x", "y")]
        );
        assert_eq!(opening("<a b\n=\n'c'>", 0).1, [attribute("b", "c")]);
        assert_eq!(
            opening("<a =x / y>", 0).1,
            [attribute("x", ""), attribute("y", "")],
            "stray marks count for nothing"
        );
    }

    #[test]
    fn what_is_no_tag_reads_as_nothing() {
        assert!(read_tag("<1a>", 0).is_none());
        assert!(read_tag("<", 0).is_none());
        assert!(read_tag("<a b", 0).is_none());
        assert!(read_tag("<a b=", 0).is_none());
    }

    #[test]
    fn elements_without_an_end_close_nothing_later() {
        assert_eq!(stray_closes("<br></br>"), ["br"]);
        assert_eq!(stray_closes("<div/></div>"), ["div"]);
        assert_eq!(stray_closes("<div></div>"), Vec::<String>::new());
    }
}
