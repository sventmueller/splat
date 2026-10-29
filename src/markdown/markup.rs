//! A forgiving reader for the HTML that Markdown documents embed, as READMEs do for centered logos, collapsible
//! sections and sized pictures. It keeps where every element and text stands in the source, so the preview can show
//! the HTML rendered and still map it back. Scripts, styles, frames and forms are left out entirely.

use std::ops::Range;

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
    pub fn align(&self) -> Option<&'static str> {
        let value = self.attr("align").map(str::to_ascii_lowercase).or_else(|| {
            let style = self.attr("style")?.to_ascii_lowercase();
            let at = style.find("text-align")?;
            let rest = style[at + "text-align".len()..].trim_start().strip_prefix(':')?;
            Some(rest.split(';').next()?.trim().to_string())
        })?;
        match value.as_str() {
            "center" | "middle" => Some("center"),
            "right" => Some("right"),
            "left" => Some("left"),
            _ => None,
        }
    }

    /// A size attribute in pixels; `width="50%"` gives `None`.
    pub fn pixels(&self, name: &str) -> Option<u32> {
        let v = self.attr(name)?.trim().trim_end_matches("px");
        v.parse::<f64>()
            .ok()
            .filter(|n| *n > 0.0 && *n < 100_000.0)
            .map(|n| n as u32)
    }
}

/// Elements with no content and no closing tag.
const VOID: [&str; 10] = [
    "br", "img", "hr", "input", "source", "meta", "link", "wbr", "col", "area",
];
/// Elements dropped with everything inside them: they run code, load pages or ask for input.
pub const DROPPED: [&str; 14] = [
    "script", "style", "iframe", "frame", "object", "embed", "form", "input", "button", "select", "textarea",
    "template", "noscript", "applet",
];
/// Elements whose opening closes an open `<p>`, as browsers do.
const BLOCKS: [&str; 24] = [
    "p",
    "div",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "table",
    "tr",
    "td",
    "th",
    "blockquote",
    "pre",
    "hr",
    "details",
    "summary",
    "dl",
    "dt",
    "dd",
    "center",
];

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
    let bytes = html.as_bytes();
    while i < html.len() {
        if bytes[i] == b'<' {
            if html[i..].starts_with("<!--") {
                i = html[i + 4..].find("-->").map_or(html.len(), |e| i + 4 + e + 3);
                continue;
            }
            if let Some((tag, end)) = read_tag(html, i) {
                match tag {
                    Tag::Open {
                        name,
                        attrs,
                        self_closing,
                    } => {
                        if DROPPED.contains(&name.as_str()) {
                            i = if self_closing || VOID.contains(&name.as_str()) {
                                end
                            } else {
                                skip_to_close(html, end, &name)
                            };
                            continue;
                        }
                        if BLOCKS.contains(&name.as_str()) {
                            close_implicitly(&mut stack, &name, i);
                        }
                        let element = Element {
                            name: name.clone(),
                            attrs,
                            children: Vec::new(),
                            tag: i..end,
                            range: i..end,
                            closed: false,
                        };
                        if self_closing || VOID.contains(&name.as_str()) {
                            push_child(
                                &mut stack,
                                Node::Element(Element {
                                    closed: true,
                                    ..element
                                }),
                            );
                        } else {
                            stack.push(element);
                        }
                    }
                    Tag::Close { name } => {
                        if let Some(at) = stack.iter().rposition(|e| e.name == name)
                            && at > 0
                        {
                            while stack.len() > at + 1 {
                                pop(&mut stack, i, false);
                            }
                            pop(&mut stack, end, true);
                        }
                    }
                }
                i = end;
                continue;
            }
        }
        let step = html[i..].chars().next().map_or(1, char::len_utf8);
        let next = html[i + step..].find('<').map_or(html.len(), |n| i + step + n);
        let raw = &html[i..next];
        push_child(
            &mut stack,
            Node::Text {
                text: decode(raw),
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

/// A new `<li>` ends the open one, a new row or cell the open one, a block element an open `<p>`.
fn close_implicitly(stack: &mut Vec<Element>, name: &str, at: usize) {
    let ends = |open: &str| match name {
        "li" => open == "li" || open == "p",
        "tr" => matches!(open, "tr" | "td" | "th" | "p"),
        "td" | "th" => matches!(open, "td" | "th" | "p"),
        "dt" | "dd" => matches!(open, "dt" | "dd" | "p"),
        _ => open == "p",
    };
    let barrier = |open: &str| matches!(open, "ul" | "ol" | "table" | "dl" | "details" | "blockquote" | "div");
    if let Some(pos) = stack.iter().rposition(|e| ends(&e.name) || barrier(&e.name))
        && pos > 0
        && ends(&stack[pos].name)
    {
        while stack.len() > pos {
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
    let bytes = html.as_bytes();
    let mut attrs = Vec::new();
    loop {
        while i < html.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= html.len() {
            return None;
        }
        match bytes[i] {
            b'>' => {
                let tag = if closing {
                    Tag::Close { name }
                } else {
                    Tag::Open {
                        name,
                        attrs,
                        self_closing: false,
                    }
                };
                return Some((tag, i + 1));
            }
            b'/' if html[i + 1..].starts_with('>') => {
                let tag = if closing {
                    Tag::Close { name }
                } else {
                    Tag::Open {
                        name,
                        attrs,
                        self_closing: true,
                    }
                };
                return Some((tag, i + 2));
            }
            _ => {
                let key_len = html[i..]
                    .bytes()
                    .take_while(|b| !b.is_ascii_whitespace() && !matches!(b, b'=' | b'>' | b'/'))
                    .count();
                if key_len == 0 {
                    i += 1;
                    continue;
                }
                let key = html[i..i + key_len].to_ascii_lowercase();
                i += key_len;
                while i < html.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                let mut value = String::new();
                if i < html.len() && bytes[i] == b'=' {
                    i += 1;
                    while i < html.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    match bytes.get(i) {
                        Some(&q @ (b'"' | b'\'')) => {
                            let end = html[i + 1..].find(q as char).map_or(html.len(), |e| i + 1 + e);
                            value = decode(&html[i + 1..end]);
                            i = (end + 1).min(html.len());
                        }
                        _ => {
                            let len = html[i..]
                                .bytes()
                                .take_while(|b| !b.is_ascii_whitespace() && *b != b'>')
                                .count();
                            value = decode(&html[i..i + len]);
                            i += len;
                        }
                    }
                }
                attrs.push((key, value));
            }
        }
    }
}

/// The offset after the closing tag of `name`, searched from `from`.
fn skip_to_close(html: &str, from: usize, name: &str) -> usize {
    let lower = html[from..].to_ascii_lowercase();
    let close = format!("</{name}");
    lower
        .find(&close)
        .and_then(|at| html[from + at..].find('>').map(|e| from + at + e + 1))
        .unwrap_or(html.len())
}

/// Text with HTML entities decoded.
pub fn decode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        match crate::preview::layout::decode_entity(rest) {
            Some((c, len)) => {
                out.push(c);
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

/// The names of the elements a block closes without opening them, as `</details>` after the wrapped Markdown.
pub fn stray_closes(html: &str) -> Vec<String> {
    let mut open: Vec<String> = Vec::new();
    let mut stray = Vec::new();
    let mut i = 0;
    while let Some(at) = html[i..].find('<') {
        let start = i + at;
        match read_tag(html, start) {
            Some((Tag::Open { name, self_closing, .. }, end)) => {
                if !self_closing && !VOID.contains(&name.as_str()) {
                    open.push(name);
                }
                i = end;
            }
            Some((Tag::Close { name }, end)) => {
                match open.iter().rposition(|n| *n == name) {
                    Some(p) => open.truncate(p),
                    None => stray.push(name),
                }
                i = end;
            }
            None => i = start + 1,
        }
    }
    stray
}

/// URL schemes a link or picture in embedded HTML may use.
pub fn safe_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    match lower.find(':') {
        Some(at)
            if lower[..at]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) =>
        {
            matches!(&lower[..at], "http" | "https" | "mailto")
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!((p.name.as_str(), p.align(), p.closed), ("p", Some("center"), true));
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
        assert!(safe_url("https://x") && safe_url("img/a.png") && !safe_url("javascript:alert(1)"));
    }
}
