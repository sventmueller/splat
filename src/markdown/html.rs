//! The document as a self-contained HTML page. A document can come from anyone, so the page carries no script:
//! raw HTML is kept only for the few tags Splat's own formatting writes, links keep only harmless schemes, and a
//! content security policy forbids scripts even if something slipped through.

use std::collections::HashMap;

use super::commands::heading_anchors;
use super::doc::{
    AlertKind, Align, Block, BlockKind, Figure, Inline, InlineKind, ListKind, Piece, code_figure, parse, plain_text,
};
use super::markup::{self, Element, Node, is_void, kept_in_exports, read_tag, stray_closes};
use super::style::html_element;
use super::text::right_to_left;
use super::typography::{Mark, Open, Quotes, before_text, marks, smarten};
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

/// Attributes kept on embedded HTML; addresses are checked separately.
const KEPT_ATTRIBUTES: [&str; 11] = [
    "align", "title", "alt", "width", "height", "colspan", "rowspan", "start", "open", "href", "src",
];

pub fn to_html(text: &str, export: &Export) -> String {
    let doc = parse(text);
    let mut page = Page {
        text,
        export,
        anchors: heading_anchors(&doc)
            .into_iter()
            .map(|h| (h.range.start, h.anchor))
            .collect(),
        notes: Vec::new(),
        open: Open::default(),
        body: String::with_capacity(text.len() + text.len() / 2),
    };
    for block in &doc.blocks {
        page.block(block);
    }
    page.footnotes();
    format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src 'self' data: https: file:; style-src 'unsafe-inline'\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n<style>{STYLE}</style>\n\
         </head>\n<body>\n<article>\n{}</article>\n</body>\n</html>\n",
        escape(export.title),
        page.body,
    )
}

/// Paragraphs, list items and terms take the direction of their first letter, so Hebrew and Arabic read right to left.
/// Quotes, callouts, lists and tables take the direction of their text as a whole, as in the preview, set explicitly:
/// a browser resolving `dir="auto"` skips descendants that have a direction of their own. Code stays left to right.
fn direction(right_to_left: Option<bool>) -> &'static str {
    if right_to_left == Some(true) { "rtl" } else { "ltr" }
}

/// Whether the first letter in `blocks`, outside code, reads right to left; `None` without letters.
fn blocks_direction(blocks: &[Block]) -> Option<bool> {
    blocks.iter().find_map(|block| match &block.kind {
        BlockKind::CodeBlock { .. } => None,
        BlockKind::HtmlBlock { literal } => {
            let mut text = String::new();
            markup::raw_text(&markup::parse(literal), &mut text);
            right_to_left(&text)
        }
        _ => right_to_left(&plain_text(block.inlines())).or_else(|| blocks_direction(block.children())),
    })
}

struct Page<'a> {
    text: &'a str,
    export: &'a Export<'a>,
    /// The anchor of every heading, by where it starts in the source, as links and the table of contents name it.
    anchors: HashMap<usize, String>,
    notes: Vec<&'a Block>,
    /// Quotes the runs of the paragraph being written left open.
    open: Open,
    body: String,
}

/// What surrounds running text and changes what it may turn into.
#[derive(Clone, Copy, Default)]
struct Inside {
    link: bool,
    code: bool,
}

impl<'a> Page<'a> {
    fn children(&mut self, blocks: &'a [Block]) {
        for block in blocks {
            self.block(block);
        }
    }

    fn block(&mut self, block: &'a Block) {
        match &block.kind {
            BlockKind::Paragraph => {
                self.body.push_str("<p dir=\"auto\">");
                self.leaf(block.inlines());
                self.body.push_str("</p>\n");
            }
            BlockKind::Heading { level, .. } => {
                let anchor = self.anchors.get(&block.range.start).map_or("", String::as_str);
                self.body
                    .push_str(&format!("<h{level} id=\"{}\" dir=\"auto\">", escape(anchor)));
                self.leaf(block.inlines());
                self.body.push_str(&format!("</h{level}>\n"));
            }
            BlockKind::BlockQuote => {
                let dir = direction(blocks_direction(block.children()));
                self.body.push_str(&format!("<blockquote dir=\"{dir}\">\n"));
                self.children(block.children());
                self.body.push_str("</blockquote>\n");
            }
            BlockKind::Alert { kind, title, .. } => self.alert(block, *kind, title.as_deref()),
            BlockKind::List { kind, tight } => self.list(block, kind, *tight),
            BlockKind::CodeBlock { info, literal, .. } => self.code_block(info, literal),
            BlockKind::HtmlBlock { literal } => {
                // An element closed in a later block, as `</details>` after the Markdown it wraps, closes here.
                for name in stray_closes(literal) {
                    if kept_in_exports(&name) {
                        self.body.push_str(&format!("</{name}>"));
                    }
                }
                self.body.push_str(&clean_html(&markup::parse(literal), self.export));
                self.body.push('\n');
            }
            BlockKind::ThematicBreak => self.body.push_str("<hr />\n"),
            BlockKind::Table { alignments } => self.table(block, alignments),
            BlockKind::FootnoteDefinition { .. } => self.notes.push(block),
            BlockKind::MathBlock { literal } => {
                let figure = Figure::Math {
                    tex: literal,
                    display: true,
                };
                match (self.export.figure)(&figure) {
                    Some(img) => self.body.push_str(&format!("<p class=\"diagram\">{img}</p>\n")),
                    None => self.body.push_str(&format!(
                        "<p dir=\"auto\"><span data-math-style=\"display\">{}</span></p>\n",
                        escape(literal)
                    )),
                }
            }
            BlockKind::DefinitionList => {
                self.body.push_str("<dl>\n");
                for part in block.children().iter().flat_map(|item| item.children()) {
                    let name = if matches!(part.kind, BlockKind::DefinitionTerm) {
                        "dt"
                    } else {
                        "dd"
                    };
                    self.body.push_str(&format!("<{name} dir=\"auto\">"));
                    match part.children() {
                        [only] if matches!(only.kind, BlockKind::Paragraph) => self.leaf(only.inlines()),
                        children => self.children(children),
                    }
                    self.body.push_str(&format!("</{name}>\n"));
                }
                self.body.push_str("</dl>\n");
            }
            BlockKind::Item { .. }
            | BlockKind::TableRow { .. }
            | BlockKind::TableCell
            | BlockKind::DefinitionItem
            | BlockKind::DefinitionTerm
            | BlockKind::DefinitionDetails
            | BlockKind::FrontMatter { .. }
            | BlockKind::Comment => {}
        }
    }

    fn alert(&mut self, block: &'a Block, kind: AlertKind, title: Option<&str>) {
        let label = title.unwrap_or(t(kind.label()));
        // A callout decides by its body, and by its title only when the body has no letters.
        let dir = direction(blocks_direction(block.children()).or_else(|| right_to_left(label)));
        self.body.push_str(&format!(
            "<div dir=\"{dir}\" class=\"markdown-alert markdown-alert-{}\">\n<p class=\"markdown-alert-title\">{}</p>\n",
            kind.keyword().to_ascii_lowercase(),
            escape(label)
        ));
        self.children(block.children());
        self.body.push_str("</div>\n");
    }

    fn list(&mut self, block: &'a Block, kind: &ListKind, tight: bool) {
        let dir = direction(blocks_direction(block.children()));
        let name = match kind {
            ListKind::Bullet(_) => "ul",
            ListKind::Ordered { .. } => "ol",
        };
        self.body.push_str(&format!("<{name} dir=\"{dir}\""));
        if let ListKind::Ordered { start, .. } = kind
            && *start != 1
        {
            self.body.push_str(&format!(" start=\"{start}\""));
        }
        self.body.push_str(">\n");
        for item in block.children() {
            self.body.push_str("<li dir=\"auto\">");
            if let BlockKind::Item { task: Some(task), .. } = &item.kind {
                let checked = if task.checked { " checked" } else { "" };
                self.body
                    .push_str(&format!("<input type=\"checkbox\" disabled{checked} /> "));
            }
            for child in item.children() {
                if tight && matches!(child.kind, BlockKind::Paragraph) {
                    self.leaf(child.inlines());
                } else {
                    if !self.body.ends_with('\n') {
                        self.body.push('\n');
                    }
                    self.block(child);
                }
            }
            self.body.push_str("</li>\n");
        }
        self.body.push_str(&format!("</{name}>\n"));
    }

    fn code_block(&mut self, info: &str, literal: &str) {
        if let Some(img) = code_figure(info, literal).and_then(|figure| (self.export.figure)(&figure)) {
            self.body.push_str(&format!("<p class=\"diagram\">{img}</p>\n"));
            return;
        }
        let language = info.split_whitespace().next().unwrap_or("");
        let class = if language.is_empty() {
            String::new()
        } else {
            format!(" class=\"language-{}\"", escape(language))
        };
        self.body
            .push_str(&format!("<pre><code{class}>{}</code></pre>\n", escape(literal)));
    }

    fn table(&mut self, table: &'a Block, alignments: &[Align]) {
        let dir = direction(blocks_direction(table.children()));
        self.body.push_str(&format!("<table dir=\"{dir}\">\n"));
        let header_rows = table
            .children()
            .iter()
            .take_while(|row| matches!(row.kind, BlockKind::TableRow { header: true }))
            .count();
        let (head, body) = table.children().split_at(header_rows);
        for (section, rows, cell) in [("thead", head, "th"), ("tbody", body, "td")] {
            if rows.is_empty() {
                continue;
            }
            self.body.push_str(&format!("<{section}>\n"));
            for row in rows {
                self.body.push_str("<tr>\n");
                for (col, content) in row.children().iter().enumerate() {
                    let align = match alignments.get(col).copied().and_then(Align::html_name) {
                        Some("left") => " align=\"left\"",
                        Some("center") => " align=\"center\"",
                        Some("right") => " align=\"right\"",
                        _ => "",
                    };
                    self.body.push_str(&format!("<{cell}{align}>"));
                    self.leaf(content.inlines());
                    self.body.push_str(&format!("</{cell}>\n"));
                }
                self.body.push_str("</tr>\n");
            }
            self.body.push_str(&format!("</{section}>\n"));
        }
        self.body.push_str("</table>\n");
    }

    fn footnotes(&mut self) {
        let notes = std::mem::take(&mut self.notes);
        if notes.is_empty() {
            return;
        }
        self.body
            .push_str("<section class=\"footnotes\" data-footnotes>\n<ol>\n");
        for note in notes {
            let BlockKind::FootnoteDefinition { name, .. } = &note.kind else {
                continue;
            };
            let id = escape(name);
            self.body.push_str(&format!("<li id=\"fn-{id}\" dir=\"auto\">\n"));
            self.children(note.children());
            self.body.push_str(&format!(
                "<a href=\"#fnref-{id}\" class=\"footnote-backref\" data-footnote-backref>↩</a>\n</li>\n"
            ));
        }
        self.body.push_str("</ol>\n</section>\n");
    }

    /// The running text of a block.
    fn leaf(&mut self, inlines: &[Inline]) {
        self.open = Open::default();
        self.inlines(inlines, Inside::default());
    }

    fn inlines(&mut self, inlines: &[Inline], around: Inside) {
        for inline in inlines {
            self.inline(inline, around);
        }
    }

    fn inline(&mut self, i: &Inline, around: Inside) {
        // A pair of HTML tags keeps its tags, cleaned, whatever it means to the other outputs.
        if let Some((open, close)) = self.html_tags(i) {
            self.body.push_str(&clean_tag(open, self.export));
            let code = matches!(i.kind, InlineKind::Kbd);
            let link = matches!(i.kind, InlineKind::Link { .. });
            self.inlines(
                &i.children,
                Inside {
                    link: around.link || link,
                    code: around.code || code,
                },
            );
            self.body.push_str(&clean_tag(close, self.export));
            return;
        }
        match i.piece() {
            Piece::Text(t) => self.text(t, i.range.start, around),
            Piece::SoftBreak => self.body.push('\n'),
            Piece::LineBreak => self.body.push_str("<br />\n"),
            Piece::Code(code) => self.body.push_str(&format!("<code>{}</code>", escape(code))),
            Piece::Html(raw) => self.body.push_str(&clean_tag(raw, self.export)),
            Piece::Styled => match html_element(&i.kind) {
                Some(name) => self.wrapped(name, i, around),
                None if matches!(i.kind, InlineKind::Spoiler) => {
                    self.body.push_str("<span class=\"spoiler\">");
                    self.inlines(&i.children, around);
                    self.body.push_str("</span>");
                }
                None => self.inlines(&i.children, around),
            },
            Piece::Link { url, title } => {
                let url = safe_url(url, false, self.export);
                self.link(&url, title, i, around);
            }
            Piece::WikiPage(target) => {
                let file = if target.contains('.') {
                    target.to_string()
                } else {
                    format!("{target}.md")
                };
                let url = safe_url(&file, false, self.export);
                self.link(&url, "", i, around);
            }
            // Written as a tag, it keeps its size and all else the tag says.
            Piece::Picture(_) if matches!(i.kind, InlineKind::HtmlImage { .. }) => {
                let tag = self.text.get(i.range.clone()).unwrap_or("");
                self.body.push_str(&clean_tag(tag, self.export));
            }
            Piece::Picture(picture) => {
                let src = self.image_source(picture.url);
                self.image(&src, &picture.alt, picture.title);
            }
            Piece::FootnoteRef { name, ix } => {
                let id = escape(name);
                self.body.push_str(&format!(
                    "<sup class=\"footnote-ref\"><a href=\"#fn-{id}\" id=\"fnref-{id}\" data-footnote-ref>{ix}</a></sup>"
                ));
            }
            Piece::Math { literal, display } => {
                let figure = Figure::Math { tex: literal, display };
                match (self.export.figure)(&figure) {
                    Some(img) => self.body.push_str(&img),
                    None => {
                        let style = if display { "display" } else { "inline" };
                        self.body
                            .push_str(&format!("<span data-math-style=\"{style}\">{}</span>", escape(literal)));
                    }
                }
            }
            Piece::Emoji(emoji) => self.body.push_str(&escape(emoji)),
            Piece::Hidden => {}
        }
    }

    /// The opening and closing tag in the source of an inline written as HTML.
    fn html_tags(&self, i: &Inline) -> Option<(&'a str, &'a str)> {
        let first = i.children.first()?;
        let last = i.children.last()?;
        let open = self.text.get(i.range.start..first.range.start)?;
        let close = self.text.get(last.range.end..i.range.end)?;
        (open.starts_with('<') && close.starts_with("</")).then_some((open, close))
    }

    fn wrapped(&mut self, name: &str, i: &Inline, around: Inside) {
        self.body.push_str(&format!("<{name}>"));
        let code = matches!(i.kind, InlineKind::Kbd);
        self.inlines(
            &i.children,
            Inside {
                code: around.code || code,
                ..around
            },
        );
        self.body.push_str(&format!("</{name}>"));
    }

    fn link(&mut self, url: &str, title: &str, i: &Inline, around: Inside) {
        self.body.push_str(&format!("<a href=\"{}\"", escape(url)));
        if !title.is_empty() {
            self.body.push_str(&format!(" title=\"{}\"", escape(title)));
        }
        self.body.push('>');
        self.inlines(&i.children, Inside { link: true, ..around });
        self.body.push_str("</a>");
    }

    fn image_source(&self, url: &str) -> String {
        (self.export.embed)(url).unwrap_or_else(|| safe_url(url, true, self.export))
    }

    fn image(&mut self, src: &str, alt: &str, title: &str) {
        self.body
            .push_str(&format!("<img src=\"{}\" alt=\"{}\"", escape(src), escape(alt)));
        if !title.is_empty() {
            self.body.push_str(&format!(" title=\"{}\"", escape(title)));
        }
        self.body.push_str(" />");
    }

    /// Running text, with typographic punctuation, tags, references and citations as in the preview.
    fn text(&mut self, literal: &str, at: usize, around: Inside) {
        if around.code {
            self.body.push_str(&escape(literal));
            return;
        }
        let before = before_text(self.text, at);
        let shown = match self.export.quotes {
            Some(quotes) => smarten(literal, before, quotes, &mut self.open),
            None => literal.to_string(),
        };
        if around.link {
            self.body.push_str(&escape(&shown));
            return;
        }
        let mut done = 0;
        for (range, mark) in marks(&shown, before, true) {
            let piece = &shown[range.clone()];
            let replacement = match &mark {
                Mark::Tag(name) => Some(format!("<span class=\"tag\">#{}</span>", escape(name))),
                Mark::Reference(reference) => (self.export.reference)(reference)
                    .map(|url| format!("<a href=\"{}\">{}</a>", escape(&url), escape(reference))),
                Mark::Citation(keys) => {
                    let names: Option<Vec<String>> = keys.iter().map(|k| (self.export.citation)(k)).collect();
                    let shown_as = names.map_or_else(|| piece.to_string(), |n| format!("({})", n.join("; ")));
                    Some(format!("<cite>{}</cite>", escape(&shown_as)))
                }
            };
            let Some(replacement) = replacement else { continue };
            self.body.push_str(&escape(&shown[done..range.start]));
            self.body.push_str(&replacement);
            done = range.end;
        }
        self.body.push_str(&escape(&shown[done..]));
    }
}

/// The direction an embedded element gets, as Markdown's blocks of the same kind get it.
fn element_direction(e: &Element) -> Option<&'static str> {
    match e.name.as_str() {
        "p" | "li" | "dt" | "dd" => Some("auto"),
        "blockquote" | "ul" | "ol" | "table" => {
            let mut text = String::new();
            markup::raw_text(&e.children, &mut text);
            Some(direction(right_to_left(&text)))
        }
        _ => None,
    }
}

/// Embedded HTML with only the elements and attributes a page may show without running anything.
fn clean_html(nodes: &[Node], export: &Export) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text { text, .. } => out.push_str(&escape(text)),
            Node::Element(e) => {
                let inner = clean_html(&e.children, export);
                if !kept_in_exports(&e.name) {
                    out.push_str(&inner);
                    continue;
                }
                let align = e.align().and_then(Align::html_name);
                out.push_str(&open_tag(&e.name, &e.attrs, align, element_direction(e), export));
                if is_void(&e.name) {
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

fn open_tag(name: &str, attrs: &[(String, String)], align: Option<&str>, dir: Option<&str>, export: &Export) -> String {
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
    if let Some(dir) = dir {
        tag.push_str(&format!(" dir=\"{dir}\""));
    }
    tag.push('>');
    tag
}

/// A single inline tag, cleaned like embedded HTML; comments and other elements drop out.
fn clean_tag(raw: &str, export: &Export) -> String {
    let trimmed = raw.trim();
    match read_tag(trimmed, 0) {
        Some((markup::Tag::Open { name, attrs, .. }, _)) if kept_in_exports(&name) => {
            let align = attrs
                .iter()
                .find(|(k, _)| k == "align")
                .map(|(_, v)| v.to_ascii_lowercase())
                .filter(|v| matches!(v.as_str(), "left" | "center" | "right"));
            open_tag(&name, &attrs, align.as_deref(), None, export)
        }
        Some((markup::Tag::Close { name }, _)) if kept_in_exports(&name) => format!("</{name}>"),
        _ => String::new(),
    }
}

/// Web and mail links, in-page anchors and relative paths stay; every other scheme, such as `javascript:` or
/// `file:`, is dropped. Images may also be embedded PNG, JPEG, GIF or WebP data.
fn safe_url(url: &str, image: bool, export: &Export) -> String {
    let lower = url.trim().to_ascii_lowercase();
    match super::url::scheme(url).as_deref() {
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
        assert!(page.contains("<p align=\"center\" dir=\"auto\">"), "{page}");
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
    fn headings_read_as_in_the_preview_and_keep_their_own_anchors() {
        let page = export("> # Zitat\n\n# \"Hi\" #tag\n\n# Hi\n\n**\"Hallo\"** [\"Link\" #kein](https://a.de)\n");
        assert!(page.contains("<h1 id=\"zitat\" dir=\"auto\">Zitat</h1>"), "{page}");
        assert!(page.contains("„Hi“ <span class=\"tag\">#tag</span></h1>"), "{page}");
        assert!(page.contains("<h1 id=\"hi\" dir=\"auto\">Hi</h1>"), "{page}");
        assert!(page.contains("<strong>„Hallo“</strong>"), "{page}");
        assert!(page.contains(">„Link“ #kein</a>"), "{page}");
    }

    #[test]
    fn named_references_in_embedded_html_stay_what_they_are() {
        let page = export("<p>Caf&eacute;</p>\n");
        assert!(page.contains("Café"), "{page}");
        assert!(!page.contains("&amp;eacute;"), "{page}");
    }

    #[test]
    fn anchors_of_headings_deeper_than_the_model_goes_do_not_shift_the_others() {
        let page = export(&format!("{}# tief\n\n# danach\n\n# zuletzt\n", "> ".repeat(70)));
        assert!(page.contains("<h1 id=\"danach\" dir=\"auto\">danach</h1>"), "{page}");
        assert!(page.contains("<h1 id=\"zuletzt\" dir=\"auto\">zuletzt</h1>"), "{page}");
    }

    #[test]
    fn everything_a_browser_reads_keeps_its_tags() {
        let page = export(
            "- [x] fertig\n- [ ] offen\n\n1. a\n\n   b\n\n~~weg~~ ~tief~ <u>unter</u> ![Bild](img/a.png \"T\")\n\n\
             | L | R |\n|:--|--:|\n| 1 | 2 |\n\nText[^n]\n\n[^n]: Die Note.\n\n```rust\nlet a = 1 < 2;\n```\n\n---\n",
        );
        for wanted in [
            "<input type=\"checkbox\" disabled checked /> fertig</li>",
            "<input type=\"checkbox\" disabled /> offen</li>",
            "<li dir=\"auto\">\n<p dir=\"auto\">a</p>\n<p dir=\"auto\">b</p>\n</li>",
            "<del>weg</del>",
            "<sub>tief</sub>",
            "<u>unter</u>",
            "<img src=\"../docs/img/a.png\" alt=\"Bild\" title=\"T\" />",
            "<thead>\n<tr>\n<th align=\"left\">L</th>\n<th align=\"right\">R</th>\n</tr>\n</thead>",
            "<tbody>\n<tr>\n<td align=\"left\">1</td>",
            "<sup class=\"footnote-ref\"><a href=\"#fn-n\" id=\"fnref-n\" data-footnote-ref>1</a></sup>",
            "<li id=\"fn-n\" dir=\"auto\">\n<p dir=\"auto\">Die Note.</p>",
            "<pre><code class=\"language-rust\">let a = 1 &lt; 2;\n</code></pre>",
            "<hr />",
        ] {
            assert!(page.contains(wanted), "{wanted}: {page}");
        }
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

    #[test]
    fn the_direction_of_a_page_also_reads_embedded_html() {
        let direction = |t: &str| blocks_direction(&crate::markdown::doc::parse(t).blocks);
        assert_eq!(direction("<p>שלום</p>\n"), Some(true));
        assert_eq!(direction("```\nשלום\n```\n\nabc\n"), Some(false), "code does not count");
    }

    #[test]
    fn a_definition_of_anything_but_one_paragraph_keeps_its_blocks() {
        let page = export("T\n: - a\n");
        assert!(page.contains("<li"), "{page}");
    }

    #[test]
    fn every_alignment_of_a_column_reaches_the_page() {
        let page = export("| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n");
        for align in ["left", "center", "right"] {
            assert!(page.contains(&format!("align=\"{align}\"")), "{align}: {page}");
        }
    }

    #[test]
    fn no_link_grows_inside_a_link_however_deep() {
        let links = |t: &str| export(t).matches("<a ").count();
        assert_eq!(links("[<b>#12</b>](https://x.de)\n"), 1);
        assert_eq!(links("<a href=\"https://x.de\">#12</a>\n"), 1);
    }

    #[test]
    fn keyboard_keys_keep_their_straight_quotes_however_deep() {
        for t in [
            "<kbd>\"a\"</kbd>\n",
            "<kbd><b>\"a\"</b></kbd>\n",
            "<kbd>**\"a\"**</kbd>\n",
        ] {
            let page = export(t);
            assert!(!page.contains("„a“"), "{t}: {page}");
        }
    }

    #[test]
    fn elements_that_make_paragraphs_elsewhere_stay_apart_here() {
        let page = export("<section>a</section><section>b</section>\n");
        assert!(
            page.contains("<section>a</section>") || page.contains("<section dir=\"auto\">a</section>"),
            "{page}"
        );
        assert!(export("<var>x</var>\n").contains("<var>x</var>"));
    }

    #[test]
    fn a_wiki_link_is_a_picture_only_when_embedded_and_a_picture() {
        assert!(export("![[a.png]]\n").contains("<img"));
        assert!(!export("[[a.png]]\n").contains("<img"));
        assert!(!export("![[a.md]]\n").contains("<img"));
    }

    #[test]
    fn an_address_in_angle_brackets_is_a_link_and_no_tag() {
        let page = export("<https://x.de>\n");
        assert!(page.contains("<a href=\"https://x.de\""), "{page}");
    }

    #[test]
    fn a_link_has_a_title_only_when_it_was_given_one() {
        assert!(export("[a](https://x.de \"t\")\n").contains("title=\"t\""));
        assert!(!export("[a](https://x.de)\n").contains("title="));
    }

    #[test]
    fn embedded_quotes_and_lists_read_in_the_direction_of_their_text() {
        assert!(export("<blockquote>שלום</blockquote>\n").contains("<blockquote dir=\"rtl\""));
        assert!(export("<ul><li>שלום</li></ul>\n").contains("<ul dir=\"rtl\""));
        assert!(export("<table><tr><td>abc</td></tr></table>\n").contains("<table dir=\"ltr\""));
    }

    #[test]
    fn an_inline_tag_keeps_its_alignment_whatever_else_it_says() {
        let page = export("a <span title=\"t\" align=\"right\">b</span>\n");
        assert!(page.contains("align=\"right\""), "{page}");
    }

    #[test]
    fn links_and_pictures_each_allow_their_own_schemes() {
        let e = Export {
            title: "",
            figure: &|_| None,
            embed: &|_| None,
            rebase: &|p| p.to_string(),
            quotes: None,
            reference: &|_| None,
            citation: &|_| None,
        };
        let png = "data:image/png;base64,AA";
        assert_eq!(safe_url("mailto:a@b.de", false, &e), "mailto:a@b.de");
        assert_eq!(safe_url("mailto:a@b.de", true, &e), "#");
        assert_eq!(safe_url(png, true, &e), png);
        assert_eq!(safe_url(png, false, &e), "#");
        assert_eq!(safe_url("data:text/html;base64,AA", true, &e), "#");
        assert_eq!(safe_url("data:image/svg+xml;base64,AA", true, &e), "#");
    }

    #[test]
    fn webp_needs_both_of_its_marks() {
        assert_eq!(web_image_type(b"RIFF\0\0\0\0AVI "), None);
        assert_eq!(web_image_type(b"XXXX\0\0\0\0WEBP"), None);
    }
}
