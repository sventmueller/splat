use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::text::{Byte, TextAt};
use crate::i18n::n;

mod figures;

pub use super::style::Align;
pub use figures::{
    DiagramKind, Figure, code_figure, diagram_kind, figure_at, figures, has_remote_images, is_image_name,
    is_math_fence, math_delimiters,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListKind {
    Bullet(char),
    Ordered { start: usize, delim: char },
}

/// GitHub alert types, written `> [!NOTE]` and so on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl AlertKind {
    pub const ALL: [AlertKind; 5] = [
        AlertKind::Note,
        AlertKind::Tip,
        AlertKind::Important,
        AlertKind::Warning,
        AlertKind::Caution,
    ];

    pub fn keyword(self) -> &'static str {
        match self {
            AlertKind::Note => "NOTE",
            AlertKind::Tip => "TIP",
            AlertKind::Important => "IMPORTANT",
            AlertKind::Warning => "WARNING",
            AlertKind::Caution => "CAUTION",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AlertKind::Note => n("Hinweis"),
            AlertKind::Tip => n("Tipp"),
            AlertKind::Important => n("Wichtig"),
            AlertKind::Warning => n("Warnung"),
            AlertKind::Caution => n("Vorsicht"),
        }
    }
}

/// An Obsidian callout written `> [!note]-` starts folded, `+` open; a click on its title flips the sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fold {
    pub open: bool,
    /// Where the `-` or `+` stands.
    pub sign: Byte,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub checked: bool,
    /// Where the character between the brackets stands.
    pub symbol: Byte,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading {
        level: u8,
        setext: bool,
        /// Anchor given as `{#id}` after the title.
        id: Option<Box<str>>,
        /// The `{…}` attribute list after the title, which the preview leaves out.
        attrs: Option<Range<Byte>>,
    },
    BlockQuote,
    /// A GitHub alert or an Obsidian callout.
    Alert {
        kind: AlertKind,
        title: Option<Box<str>>,
        /// `[!NOTE]` and an optional custom title on the first line.
        marker: Range<Byte>,
        fold: Option<Fold>,
    },
    List {
        kind: ListKind,
        tight: bool,
    },
    Item {
        task: Option<Task>,
        marker: Range<Byte>,
    },
    CodeBlock {
        info: Box<str>,
        literal: Box<str>,
        fenced: bool,
        content: Range<Byte>,
    },
    HtmlBlock {
        literal: Box<str>,
    },
    ThematicBreak,
    Table {
        alignments: Box<[Align]>,
    },
    TableRow {
        header: bool,
    },
    TableCell,
    FootnoteDefinition {
        name: Box<str>,
        label: Range<Byte>,
        /// The number an inline `^[note]` shows, which has no name of its own.
        number: Option<u32>,
    },
    MathBlock {
        literal: Box<str>,
    },
    /// YAML metadata between `---` lines at the very start, as static site generators and Obsidian use it.
    FrontMatter {
        content: Range<Byte>,
    },
    /// Terms followed by `: details` paragraphs, as Pandoc and PHP Markdown Extra write them.
    DefinitionList,
    DefinitionItem,
    DefinitionTerm,
    DefinitionDetails,
    /// Lines between `%%` marks, Obsidian's notes to self, which the preview leaves out.
    Comment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineKind {
    Text(Box<str>),
    SoftBreak,
    LineBreak,
    Code(Box<str>),
    Html(Box<str>),
    Emph,
    Strong,
    Strikethrough,
    Underline,
    Highlight,
    Superscript,
    Subscript,
    Link {
        url: Box<str>,
        title: Box<str>,
    },
    Image {
        url: Box<str>,
        title: Box<str>,
    },
    FootnoteRef {
        name: Box<str>,
        ix: u32,
    },
    Math {
        display: bool,
        literal: Box<str>,
    },
    /// A `:shortcode:` and the emoji it stands for.
    Emoji(Box<str>),
    /// `||text||`, hidden until selected.
    Spoiler,
    /// `[[Note]]`, `[[Note|Title]]` or, embedding the target, `![[picture.png]]`.
    WikiLink {
        target: Box<str>,
        embed: bool,
    },
    /// `%%text%%` or `<!-- text -->`, left out of the preview.
    Comment,
    /// `<kbd>`, `<code>`, `<samp>` or `<tt>` around text.
    Kbd,
    /// An HTML element that only groups text, such as `<span>` or `<small>`.
    Span,
    /// `<img>` written as HTML, with the size it asks for.
    HtmlImage {
        url: Box<str>,
        alt: Box<str>,
        width: Option<u32>,
        height: Option<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inline {
    pub kind: InlineKind,
    pub range: Range<Byte>,
    pub children: Box<[Inline]>,
}

impl InlineKind {
    /// Whether other inlines stand inside it.
    pub fn holds_inlines(&self) -> bool {
        matches!(
            self,
            InlineKind::Emph
                | InlineKind::Strong
                | InlineKind::Strikethrough
                | InlineKind::Underline
                | InlineKind::Highlight
                | InlineKind::Superscript
                | InlineKind::Subscript
                | InlineKind::Spoiler
                | InlineKind::WikiLink { .. }
                | InlineKind::Kbd
                | InlineKind::Span
                | InlineKind::Link { .. }
                | InlineKind::Image { .. }
        )
    }
}

/// An inline as its writers see it: kinds that differ only in their look are one case here, and what `TextStyle::inside`
/// and `style::html_element` say about them is all that a new look needs.
#[derive(Debug)]
pub enum Piece<'a> {
    Text(&'a str),
    Code(&'a str),
    SoftBreak,
    LineBreak,
    Html(&'a str),
    Emoji(&'a str),
    /// Its children, looking different.
    Styled,
    Link {
        url: &'a str,
        title: &'a str,
    },
    /// `[[Note]]` with its title as children.
    WikiPage(&'a str),
    Picture(Picture<'a>),
    FootnoteRef {
        name: &'a str,
        ix: u32,
    },
    Math {
        literal: &'a str,
        display: bool,
    },
    /// Nothing anyone sees.
    Hidden,
}

/// A picture, however the document spells it.
#[derive(Debug)]
pub struct Picture<'a> {
    pub url: &'a str,
    pub alt: Cow<'a, str>,
    pub title: &'a str,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Strings and lists are boxed slices without spare capacity, since a large document has hundreds of thousands of
/// blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub range: Range<Byte>,
    content: Content,
}

/// A block holds running text or other blocks, never both.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Content {
    Inlines(Box<[Inline]>),
    Blocks(Box<[Block]>),
}

/// A text and what it was parsed into. A text that stays plain or could not be parsed has no blocks.
#[derive(Debug, Default)]
pub struct Doc {
    pub blocks: Vec<Block>,
    text: Arc<str>,
    version: u64,
    too_large: bool,
}

fn next_version() -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    LAST.fetch_add(1, Ordering::Relaxed) + 1
}

impl Doc {
    pub fn new(text: Arc<str>, blocks: Vec<Block>) -> Doc {
        Doc {
            blocks,
            text,
            version: next_version(),
            too_large: false,
        }
    }

    /// A text that takes too much memory to parse, which leaves it without blocks.
    pub fn too_large(text: Arc<str>) -> Doc {
        Doc {
            too_large: true,
            ..Doc::new(text, Vec::new())
        }
    }

    /// Whether this text stays plain; looking at the bytes again would take milliseconds in a large text.
    pub fn is_too_large(&self) -> bool {
        self.too_large
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text for what outlives this tree, without a copy.
    pub fn shared_text(&self) -> Arc<str> {
        self.text.clone()
    }

    /// Differs for every parse in the process, even of the same text, so what was made from a parse can be told by it.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The blocks in a document of their own; this one keeps the text, which a panic on the way must not take along.
    pub(super) fn split_off_blocks(&mut self) -> Doc {
        Doc {
            blocks: std::mem::take(&mut self.blocks),
            text: self.text.clone(),
            version: self.version,
            too_large: self.too_large,
        }
    }

    /// A partial parse changed the blocks in place to fit `text`.
    pub(super) fn renew(&mut self, text: &str) {
        self.text = text.into();
        self.version = next_version();
        self.too_large = false;
    }
}

/// The text of inlines as a screen reader would read them, for a heading's name or a picture's alternative text.
pub fn plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for i in inlines {
        match &i.kind {
            InlineKind::Text(t) | InlineKind::Code(t) | InlineKind::Emoji(t) => out.push_str(t),
            InlineKind::Math { literal, .. } => out.push_str(literal),
            InlineKind::SoftBreak | InlineKind::LineBreak => out.push(' '),
            InlineKind::Html(_) | InlineKind::FootnoteRef { .. } | InlineKind::Comment => {}
            _ => out.push_str(&plain_text(&i.children)),
        }
    }
    out
}

impl Inline {
    /// The source range inside the backticks of a code span.
    pub fn code_content(&self, text: &str) -> Range<Byte> {
        let src = text.at(self.range.clone());
        let opening = src.bytes().take_while(|&b| b == b'`').count();
        let closing = src[opening..].bytes().rev().take_while(|&b| b == b'`').count();
        self.range.start + opening..self.range.end - closing
    }

    pub fn is_container(&self) -> bool {
        self.kind.holds_inlines()
    }

    /// What this inline is to whoever writes the document out, in the preview or an export.
    pub fn piece(&self) -> Piece<'_> {
        match &self.kind {
            InlineKind::Text(t) => Piece::Text(t),
            InlineKind::SoftBreak => Piece::SoftBreak,
            InlineKind::LineBreak => Piece::LineBreak,
            InlineKind::Code(code) => Piece::Code(code),
            InlineKind::Html(raw) => Piece::Html(raw),
            InlineKind::Emoji(emoji) => Piece::Emoji(emoji),
            InlineKind::Comment => Piece::Hidden,
            InlineKind::Link { url, title } => Piece::Link { url, title },
            InlineKind::FootnoteRef { name, ix } => Piece::FootnoteRef { name, ix: *ix },
            InlineKind::Math { display, literal } => Piece::Math {
                literal,
                display: *display,
            },
            InlineKind::Image { url, title } => Piece::Picture(Picture {
                url,
                alt: plain_text(&self.children).into(),
                title,
                width: None,
                height: None,
            }),
            InlineKind::HtmlImage {
                url,
                alt,
                width,
                height,
            } => Piece::Picture(Picture {
                url,
                alt: Cow::Borrowed(alt),
                title: "",
                width: *width,
                height: *height,
            }),
            InlineKind::WikiLink { target, embed: true } if is_image_name(target) => Piece::Picture(Picture {
                url: target,
                alt: Cow::Borrowed(target),
                title: "",
                width: None,
                height: None,
            }),
            InlineKind::WikiLink { target, .. } => Piece::WikiPage(target),
            InlineKind::Emph
            | InlineKind::Strong
            | InlineKind::Strikethrough
            | InlineKind::Underline
            | InlineKind::Highlight
            | InlineKind::Superscript
            | InlineKind::Subscript
            | InlineKind::Spoiler
            | InlineKind::Kbd
            | InlineKind::Span => Piece::Styled,
        }
    }

    /// Source range between the opening and closing delimiters of a container inline.
    pub fn content_range(&self) -> Range<Byte> {
        match (self.children.first(), self.children.last()) {
            (Some(first), Some(last)) => first.range.start..last.range.end,
            // Only `[](url)` and `![](url)` are empty without a child: `pair_html` gives its pairs an empty text.
            _ => {
                let opening = if matches!(self.kind, InlineKind::Image { .. }) {
                    2
                } else {
                    1
                };
                let at = (self.range.start + opening).min(self.range.end);
                at..at
            }
        }
    }
}

impl Block {
    pub fn leaf(kind: BlockKind, range: Range<Byte>, inlines: impl Into<Box<[Inline]>>) -> Block {
        Block {
            kind,
            range,
            content: Content::Inlines(inlines.into()),
        }
    }

    pub fn container(kind: BlockKind, range: Range<Byte>, children: impl Into<Box<[Block]>>) -> Block {
        Block {
            kind,
            range,
            content: Content::Blocks(children.into()),
        }
    }

    pub fn inlines(&self) -> &[Inline] {
        match &self.content {
            Content::Inlines(inlines) => inlines,
            Content::Blocks(_) => &[],
        }
    }

    pub fn children(&self) -> &[Block] {
        match &self.content {
            Content::Blocks(children) => children,
            Content::Inlines(_) => &[],
        }
    }

    /// The inlines and the blocks it holds, one of them empty, to change their positions.
    pub(super) fn content_mut(&mut self) -> (&mut [Inline], &mut [Block]) {
        match &mut self.content {
            Content::Inlines(inlines) => (inlines, &mut []),
            Content::Blocks(children) => (&mut [], children),
        }
    }

    pub fn is_leaf(&self) -> bool {
        matches!(
            self.kind,
            BlockKind::Paragraph
                | BlockKind::Heading { .. }
                | BlockKind::TableCell
                | BlockKind::CodeBlock { .. }
                | BlockKind::HtmlBlock { .. }
                | BlockKind::ThematicBreak
                | BlockKind::MathBlock { .. }
        )
    }

    /// Source range covered by the inline content of a leaf block.
    pub fn inline_range(&self) -> Option<Range<Byte>> {
        Some(self.inlines().first()?.range.start..self.inlines().last()?.range.end)
    }
}

/// Path from the document root to the innermost block containing `offset`.
pub fn block_path(blocks: &[Block], offset: Byte) -> Vec<&Block> {
    let mut path = Vec::new();
    let mut level = blocks;
    loop {
        let found = level
            .iter()
            .find(|b| b.range.start <= offset && offset < b.range.end)
            .or_else(|| {
                level
                    .iter()
                    .rev()
                    .find(|b| b.range.start <= offset && offset == b.range.end)
            });
        match found {
            Some(b) => {
                path.push(b);
                level = b.children();
            }
            None => return path,
        }
    }
}

/// Path of inline containers around `offset` inside the given inlines, outermost first.
pub fn inline_path(inlines: &[Inline], offset: Byte) -> Vec<&Inline> {
    let mut path = Vec::new();
    let mut level = inlines;
    loop {
        let found = level
            .iter()
            .find(|i| i.range.start <= offset && offset <= i.range.end && i.is_container());
        match found {
            Some(i) => {
                path.push(i);
                level = &i.children;
            }
            None => return path,
        }
    }
}

pub fn walk_inlines<'a>(inlines: &'a [Inline], f: &mut impl FnMut(&'a Inline)) {
    for i in inlines {
        f(i);
        walk_inlines(&i.children, f);
    }
}

pub fn walk_blocks<'a>(blocks: &'a [Block], f: &mut impl FnMut(&'a Block)) {
    for b in blocks {
        f(b);
        walk_blocks(b.children(), f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::reader::parse;
    use crate::markdown::text::bytes;

    fn slice<'a>(t: &'a str, r: &Range<Byte>) -> &'a str {
        t.at(r.clone())
    }

    #[test]
    fn an_empty_link_has_its_content_inside_the_brackets() {
        let t = "[](ä) ![](ö)\n";
        let d = parse(t);
        let ranges: Vec<_> = d.blocks[0]
            .inlines()
            .iter()
            .filter(|i| i.is_container())
            .map(Inline::content_range)
            .collect();
        let picture = t.find("![").unwrap() + 2;
        assert_eq!(ranges, [bytes(1..1), bytes(picture..picture)]);
    }

    #[test]
    fn the_alternative_text_of_a_picture_reads_breaks_as_spaces() {
        let d = parse("![a\nb](x.png)\n");
        let image = d.blocks[0]
            .inlines()
            .iter()
            .find(|i| matches!(i.kind, InlineKind::Image { .. }))
            .unwrap();
        assert_eq!(plain_text(&image.children), "a b");
    }

    #[test]
    fn only_blocks_without_blocks_inside_are_leaves() {
        let d = parse("- a\n\n> b\n");
        assert!(!d.blocks[0].is_leaf());
        assert!(!d.blocks[1].is_leaf());
        assert!(d.blocks[1].children()[0].is_leaf());
    }

    #[test]
    fn the_range_of_a_leaf_spans_its_inlines() {
        let t = "# a *b*\n";
        let d = parse(t);
        assert_eq!(slice(t, &d.blocks[0].inline_range().unwrap()), "a *b*");
        assert_eq!(parse("---\n\nx\n").blocks[0].inline_range(), None);
    }

    #[test]
    fn the_path_to_a_position_between_two_blocks_takes_the_later_one() {
        let t = "- a\n- b\n";
        let d = parse(t);
        let inner = |at: usize| slice(t, &block_path(&d.blocks, Byte::new(at)).last().unwrap().range).to_string();
        assert_eq!(inner(4), "- b");
        assert_eq!(inner(t.len() - 1), "b");
    }
}
