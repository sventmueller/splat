//! The tree a Markdown text is read into: blocks and inlines with their source ranges in bytes, and the document that
//! holds them with its text.

use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::text::{Byte, TextAt};
use facts::TextFacts;
use splat_i18n::n;

pub mod facts;
mod figures;
pub mod output;
mod shift;

pub use figures::{
    DiagramKind, Figure, code_figure, diagram_kind, figure_at, figures, is_image_name, is_math_fence, macros_of,
    math_delimiters,
};
pub use shift::{Splice, shift_block};

/// How the items of a list are marked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListKind {
    /// Items marked with `-`, `*` or `+`, the character the first item uses.
    Bullet(char),
    /// Numbered items, `1.` or `1)`.
    Ordered {
        /// The number of the first item.
        start: usize,
        /// The `.` or `)` behind each number.
        delim: char,
    },
}

/// GitHub alert types, written `> [!NOTE]` and so on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlertKind {
    /// `> [!NOTE]`, and Obsidian callouts of a type GitHub has no alert for.
    Note,
    /// `> [!TIP]`, and Obsidian's `hint`, `success`, `check` and `done`.
    Tip,
    /// `> [!IMPORTANT]`, and Obsidian's `question`, `help` and `faq`.
    Important,
    /// `> [!WARNING]`, and Obsidian's `attention`.
    Warning,
    /// `> [!CAUTION]`, and Obsidian's `danger`, `error`, `bug`, `failure` and the like.
    Caution,
}

impl AlertKind {
    /// Every kind, in the order GitHub lists them.
    pub const ALL: [AlertKind; 5] = [
        AlertKind::Note,
        AlertKind::Tip,
        AlertKind::Important,
        AlertKind::Warning,
        AlertKind::Caution,
    ];

    /// The word between `[!` and `]` that writes this kind.
    pub fn keyword(self) -> &'static str {
        match self {
            AlertKind::Note => "NOTE",
            AlertKind::Tip => "TIP",
            AlertKind::Important => "IMPORTANT",
            AlertKind::Warning => "WARNING",
            AlertKind::Caution => "CAUTION",
        }
    }

    /// The title the alert shows when it has none of its own, as the German key of the interface texts.
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
    /// Whether the body shows, `+`, rather than the title alone, `-`.
    pub open: bool,
    /// Where the `-` or `+` stands.
    pub sign: Byte,
}

/// The checkbox of a task list item, `- [ ]` or `- [x]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Whether the box is ticked, `[x]`.
    pub checked: bool,
    /// Where the character between the brackets stands.
    pub symbol: Byte,
}

/// How a block, a cell or a column aligns its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Align {
    /// As the direction of the text has it.
    #[default]
    None,
    /// Against the left edge, `:---` in a table's delimiter row.
    Left,
    /// In the middle, `:---:` in a table's delimiter row.
    Center,
    /// Against the right edge, `---:` in a table's delimiter row.
    Right,
}

impl Align {
    /// The word HTML has for it.
    pub fn html_name(self) -> Option<&'static str> {
        match self {
            Align::None => None,
            Align::Left => Some("left"),
            Align::Center => Some("center"),
            Align::Right => Some("right"),
        }
    }
}

/// What a block is, with what its source says beyond the text it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// Running text, ended by a blank line or the start of another block.
    Paragraph,
    /// A heading, `# Title`, or a title underlined with `=` or `-`.
    Heading {
        /// 1 to 6: the number of `#`, or 1 under `=` and 2 under `-`.
        level: u8,
        /// Whether it is underlined rather than opened with `#`.
        setext: bool,
        /// Anchor given as `{#id}` after the title.
        id: Option<Box<str>>,
        /// The `{…}` attribute list after the title, which the preview leaves out.
        attrs: Option<Range<Byte>>,
    },
    /// Lines opened with `>` that make no alert or callout.
    BlockQuote,
    /// A GitHub alert or an Obsidian callout.
    Alert {
        /// Which of the five alert kinds it shows as; an Obsidian callout's type maps onto one.
        kind: AlertKind,
        /// The title shown instead of the kind's label: one written behind the marker, or the type of an Obsidian
        /// callout that GitHub has no alert for.
        title: Option<Box<str>>,
        /// `[!NOTE]` and an optional custom title on the first line.
        marker: Range<Byte>,
        /// The `-` or `+` behind an Obsidian callout's marker; `None` for one that cannot fold.
        fold: Option<Fold>,
    },
    /// A bullet or ordered list, holding its items.
    List {
        /// How its items are marked.
        kind: ListKind,
        /// Whether no blank line parts its items, which then show without the spacing of paragraphs.
        tight: bool,
    },
    /// An item of a list, holding its blocks.
    Item {
        /// The checkbox of a task list item.
        task: Option<Task>,
        /// Where the bullet, or the number with its `.` or `)`, stands.
        marker: Range<Byte>,
    },
    /// Code indented by four spaces or between fences of three backticks or tildes.
    CodeBlock {
        /// The info string behind the opening fence, such as `rust`; empty for indented code.
        info: Box<str>,
        /// The code without its fences and without the indentation of the block.
        literal: Box<str>,
        /// Whether it stands between fences rather than being indented.
        fenced: bool,
        /// Where the code stands in the source: between the fence lines, or all of an indented block.
        content: Range<Byte>,
    },
    /// Raw HTML at the start of a line, such as a `<div>` or `<details>`, up to its end.
    HtmlBlock {
        /// The HTML as it stands in the source.
        literal: Box<str>,
    },
    /// A horizontal rule, `---`, `***` or `___`.
    ThematicBreak,
    /// A GitHub table, holding its rows.
    Table {
        /// The alignment of each column, from the colons in the delimiter row.
        alignments: Box<[Align]>,
    },
    /// A row of a table, holding its cells.
    TableRow {
        /// Whether it is the header row above the delimiter row.
        header: bool,
    },
    /// A cell of a table row, holding running text.
    TableCell,
    /// The text a footnote reference points to: `[^label]: text`, or the note of an inline `^[note]`.
    FootnoteDefinition {
        /// The label between `[^` and `]`, or comrak's `__inline_` name for an inline note.
        name: Box<str>,
        /// Where `[^label]:` stands; empty, at the reference, for an inline note.
        label: Range<Byte>,
        /// The number an inline `^[note]` shows, which has no name of its own.
        number: Option<u32>,
    },
    /// A display formula, `$$…$$` or `\[…\]`, that fills a paragraph on its own.
    MathBlock {
        /// The TeX without its delimiters.
        literal: Box<str>,
    },
    /// YAML metadata between `---` lines at the very start, as static site generators and Obsidian use it.
    FrontMatter {
        /// The lines between the two `---` delimiters.
        content: Range<Byte>,
    },
    /// Terms followed by `: details` paragraphs, as Pandoc and PHP Markdown Extra write them.
    DefinitionList,
    /// A term with its details, inside a definition list.
    DefinitionItem,
    /// The term an item explains, the line before the `:`.
    DefinitionTerm,
    /// What follows the `: ` and explains the term.
    DefinitionDetails,
    /// Lines between `%%` marks, Obsidian's notes to self, which the preview leaves out.
    Comment,
}

/// What an inline is, with what its source says beyond the inlines it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineKind {
    /// Running text as it reads, with backslash escapes and character references resolved.
    Text(Box<str>),
    /// A line ending inside running text, which reads as a space.
    SoftBreak,
    /// A hard line break: two spaces or a backslash at the end of a line, or `<br>`.
    LineBreak,
    /// A code span between backticks, with the text comrak reads in it.
    Code(Box<str>),
    /// Inline HTML that no other kind stands for, as written.
    Html(Box<str>),
    /// `*emphasis*` or `_emphasis_`, or `<em>`, `<i>` and the like.
    Emph,
    /// `**strong**` or `__strong__`, or `<strong>` and `<b>`.
    Strong,
    /// `~~struck~~` text, or `<del>`, `<s>` and `<strike>`.
    Strikethrough,
    /// `<u>` or `<ins>`, for which Markdown has no syntax.
    Underline,
    /// `==marked==` text, or `<mark>`.
    Highlight,
    /// `^raised^` text, or `<sup>`.
    Superscript,
    /// `~lowered~` text, or `<sub>`.
    Subscript,
    /// A link, `[text](url)`, a reference link, an autolink or `<a href>`, with its text as children.
    Link {
        /// Where it leads, with escapes and link references resolved.
        url: Box<str>,
        /// The title in quotes behind the address; empty without one.
        title: Box<str>,
    },
    /// A picture, `![alt](url)`, with its alternative text as children.
    Image {
        /// Where the picture is, with escapes and link references resolved.
        url: Box<str>,
        /// The title in quotes behind the address; empty without one.
        title: Box<str>,
    },
    /// A footnote reference, `[^label]`, or an inline note, `^[note]`.
    FootnoteRef {
        /// The label of the footnote it points to, or comrak's `__inline_` name for an inline note.
        name: Box<str>,
        /// The number it shows, which comrak gives each footnote in the order of its first reference.
        ix: u32,
    },
    /// A formula: `$…$`, `$$…$$`, `` $`…`$ ``, `\(…\)` or `\[…\]`.
    Math {
        /// Whether it is display math, `$$…$$` or `\[…\]`, set apart from the line.
        display: bool,
        /// The TeX without its delimiters.
        literal: Box<str>,
    },
    /// A `:shortcode:` and the emoji it stands for.
    Emoji(Box<str>),
    /// `||text||`, hidden until selected.
    Spoiler,
    /// `[[Note]]`, `[[Note|Title]]` or, embedding the target, `![[picture.png]]`.
    WikiLink {
        /// The note or file it names, without a title behind `|`.
        target: Box<str>,
        /// Whether it is written `![[…]]` and shows its target in place.
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
        /// The `src` attribute, kept only when its scheme is harmless.
        url: Box<str>,
        /// The `alt` attribute; empty without one.
        alt: Box<str>,
        /// The `width` attribute in pixels; `None` without one or for a percentage.
        width: Option<u32>,
        /// The `height` attribute in pixels; `None` without one or for a percentage.
        height: Option<u32>,
    },
}

/// A piece of running text in the tree, with where it stands and the inlines it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inline {
    /// The construct it is and what the source says about it.
    pub kind: InlineKind,
    /// Where it stands in the source, its delimiters included.
    pub range: Range<Byte>,
    /// The inlines inside it, for a kind that holds some; empty for any other.
    pub children: Box<[Inline]>,
}

impl BlockKind {
    /// Whether other blocks stand inside it, for each kind by name so that a new one has to answer.
    pub fn holds_blocks(&self) -> bool {
        match self {
            BlockKind::BlockQuote
            | BlockKind::Alert { .. }
            | BlockKind::List { .. }
            | BlockKind::Item { .. }
            | BlockKind::Table { .. }
            | BlockKind::TableRow { .. }
            | BlockKind::FootnoteDefinition { .. }
            | BlockKind::DefinitionList
            | BlockKind::DefinitionItem
            | BlockKind::DefinitionTerm
            | BlockKind::DefinitionDetails => true,
            BlockKind::Paragraph
            | BlockKind::Heading { .. }
            | BlockKind::CodeBlock { .. }
            | BlockKind::HtmlBlock { .. }
            | BlockKind::ThematicBreak
            | BlockKind::TableCell
            | BlockKind::MathBlock { .. }
            | BlockKind::FrontMatter { .. }
            | BlockKind::Comment => false,
        }
    }
}

impl Inline {
    /// Whether it is written as an HTML tag in `text`, its source: `<em>a</em>` is the same `Emph` as `*a*`.
    pub fn written_as_html(&self, text: &str) -> bool {
        text.as_bytes().get(self.range.start.get()) == Some(&b'<')
    }
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
    /// Running text as it reads.
    Text(&'a str),
    /// The text of a code span.
    Code(&'a str),
    /// A line ending that reads as a space.
    SoftBreak,
    /// A hard line break.
    LineBreak,
    /// Raw inline HTML, as written.
    Html(&'a str),
    /// The emoji a `:shortcode:` stands for.
    Emoji(&'a str),
    /// Its children, looking different.
    Styled,
    /// A link, with its text as children.
    Link {
        /// Where it leads.
        url: &'a str,
        /// The title in quotes behind the address; empty without one.
        title: &'a str,
    },
    /// `[[Note]]` with its title as children.
    WikiPage(&'a str),
    /// A picture, from `![alt](url)`, `<img>` or an embedded `![[picture.png]]`.
    Picture(Picture<'a>),
    /// A footnote reference.
    FootnoteRef {
        /// The label of the footnote it points to.
        name: &'a str,
        /// The number it shows.
        ix: u32,
    },
    /// A formula.
    Math {
        /// The TeX without its delimiters.
        literal: &'a str,
        /// Whether it is display math, set apart from the line.
        display: bool,
    },
    /// Nothing anyone sees.
    Hidden,
}

/// A picture, however the document spells it.
#[derive(Debug)]
pub struct Picture<'a> {
    /// Where the picture is, as written; the file name for an embedded `![[picture.png]]`.
    pub url: &'a str,
    /// The alternative text: the text of `![alt]`, the `alt` attribute, or the file name of an embed.
    pub alt: Cow<'a, str>,
    /// The title in quotes behind the address; empty for `<img>` and embeds.
    pub title: &'a str,
    /// The width an `<img>` asks for, in pixels.
    pub width: Option<u32>,
    /// The height an `<img>` asks for, in pixels.
    pub height: Option<u32>,
}

/// Strings and lists are boxed slices without spare capacity, since a large document has hundreds of thousands of
/// blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The construct it is and what the source says about it.
    pub kind: BlockKind,
    /// Where it stands in the source, from its first character to its last, the final line ending left out.
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
    /// Changed only by the reader and the partial parse, which keep it fitting `text`.
    pub(crate) blocks: Vec<Block>,
    text: Arc<str>,
    version: u64,
    too_large: bool,
    facts: std::sync::OnceLock<TextFacts>,
}

fn next_version() -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    LAST.fetch_add(1, Ordering::Relaxed) + 1
}

impl Doc {
    /// The tree `blocks` read from `text`, with a version no other parse has.
    pub fn new(text: Arc<str>, blocks: Vec<Block>) -> Doc {
        Doc {
            blocks,
            text,
            version: next_version(),
            too_large: false,
            facts: std::sync::OnceLock::new(),
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

    /// The source text the blocks were read from.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The top-level blocks, in the order of the text.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
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
    pub fn split_off_blocks(&mut self) -> Doc {
        Doc {
            blocks: std::mem::take(&mut self.blocks),
            text: self.text.clone(),
            version: self.version,
            too_large: self.too_large,
            facts: std::mem::take(&mut self.facts),
        }
    }

    /// A partial parse changed the blocks in place to fit `text`, of which `facts` hold.
    pub(super) fn renew(&mut self, text: &str, facts: TextFacts) {
        self.text = text.into();
        self.version = next_version();
        self.too_large = false;
        self.facts = std::sync::OnceLock::from(facts);
    }

    /// What decides how a part of the text is read, worked out once.
    pub fn facts(&self) -> &TextFacts {
        self.facts.get_or_init(|| TextFacts::of(self))
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

    /// Whether other inlines stand inside it.
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
    /// A block of `kind` that holds the running text `inlines`.
    pub fn leaf(kind: BlockKind, range: Range<Byte>, inlines: impl Into<Box<[Inline]>>) -> Block {
        Block {
            kind,
            range,
            content: Content::Inlines(inlines.into()),
        }
    }

    /// A block of `kind` with `children`, which a kind that holds none only has for the moment of reading it.
    pub fn container(kind: BlockKind, range: Range<Byte>, children: impl Into<Box<[Block]>>) -> Block {
        let content = if kind.holds_blocks() {
            Content::Blocks(children.into())
        } else {
            Content::Inlines(Box::default())
        };
        Block { kind, range, content }
    }

    /// The running text it holds; empty for a block that holds blocks.
    pub fn inlines(&self) -> &[Inline] {
        match &self.content {
            Content::Inlines(inlines) => inlines,
            Content::Blocks(_) => &[],
        }
    }

    /// The blocks it holds; empty for a leaf.
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

    /// Whether it holds running text rather than blocks.
    pub fn is_leaf(&self) -> bool {
        !self.kind.holds_blocks()
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

/// Visits every inline in source order, each before those inside it.
pub fn walk_inlines<'a>(inlines: &'a [Inline], f: &mut impl FnMut(&'a Inline)) {
    for i in inlines {
        f(i);
        walk_inlines(&i.children, f);
    }
}

/// Visits every block in source order, each before those inside it; inlines are not visited.
pub fn walk_blocks<'a>(blocks: &'a [Block], f: &mut impl FnMut(&'a Block)) {
    for b in blocks {
        f(b);
        walk_blocks(b.children(), f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::parse;
    use crate::text::bytes;

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
