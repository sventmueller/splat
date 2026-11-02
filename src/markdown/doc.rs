use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;

use crate::i18n::n;
use comrak::nodes::{AlertType, ListDelimType, ListType, NodeValue, Sourcepos, TableAlignment};
use comrak::{Arena, Options, parse_document};

pub use super::style::Align;
use super::text::LineStarts;

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
    /// Byte offset of the `-` or `+`.
    pub sign: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub checked: bool,
    /// Byte offset of the character between the brackets.
    pub symbol: usize,
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
        attrs: Option<Range<usize>>,
    },
    BlockQuote,
    /// A GitHub alert or an Obsidian callout.
    Alert {
        kind: AlertKind,
        title: Option<Box<str>>,
        /// `[!NOTE]` and an optional custom title on the first line.
        marker: Range<usize>,
        fold: Option<Fold>,
    },
    List {
        kind: ListKind,
        tight: bool,
    },
    Item {
        task: Option<Task>,
        marker: Range<usize>,
    },
    CodeBlock {
        info: Box<str>,
        literal: Box<str>,
        fenced: bool,
        content: Range<usize>,
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
        label: Range<usize>,
        /// The number an inline `^[note]` shows, which has no name of its own.
        number: Option<u32>,
    },
    MathBlock {
        literal: Box<str>,
    },
    /// YAML metadata between `---` lines at the very start, as static site generators and Obsidian use it.
    FrontMatter {
        content: Range<usize>,
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
    pub range: Range<usize>,
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
    pub range: Range<usize>,
    content: Content,
}

/// A block holds running text or other blocks, never both.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Content {
    Inlines(Box<[Inline]>),
    Blocks(Box<[Block]>),
}

#[derive(Debug, Default)]
pub struct Doc {
    pub blocks: Vec<Block>,
}

pub fn options() -> Options<'static> {
    let mut o = Options::default();
    o.extension.strikethrough = true;
    o.extension.table = true;
    o.extension.tasklist = true;
    o.extension.footnotes = true;
    o.extension.math_dollars = true;
    o.extension.autolink = true;
    o.extension.alerts = true;
    o.extension.front_matter_delimiter = Some("---".into());
    o.extension.superscript = true;
    // Single tildes then mean subscript, two still strike through.
    o.extension.subscript = true;
    o.extension.highlight = true;
    o.extension.shortcodes = true;
    o.extension.description_lists = true;
    o.extension.header_attributes = true;
    o.extension.wikilinks_title_after_pipe = true;
    o.extension.ofm_callouts = true;
    o.extension.ofm_comments = true;
    o.extension.inline_footnotes = true;
    o.extension.spoiler = true;
    o.extension.math_code = true;
    o.extension.math_latex = true;
    o
}

/// How many more `^[` than `]` a text may hold before its inline notes are switched off.
const UNCLOSED_NOTES: usize = 1000;

/// comrak searches the rest of the text for the end of every `^[`, which takes minutes when thousands have none.
fn limit_inline_notes(options: &mut Options, text: &str) {
    if inline_notes_may_be_limited(text) && unclosed(text, "^[", "]") > UNCLOSED_NOTES {
        options.extension.inline_footnotes = false;
    }
}

/// comrak searches the rest of a paragraph for the end of every opening delimiter of a formula as well.
fn limit_formulas(options: &mut Options, text: &str) {
    let (latex, code) = unclosed_formulas(text);
    if latex > UNCLOSED_NOTES {
        options.extension.math_latex = false;
    }
    if code > UNCLOSED_NOTES {
        options.extension.math_code = false;
    }
}

/// Whether `limit_formulas` applies to `text`, which a partial parse has to know: only the whole text can tell.
pub(super) fn formulas_are_limited(text: &str) -> bool {
    let (latex, code) = unclosed_formulas(text);
    latex > UNCLOSED_NOTES || code > UNCLOSED_NOTES
}

/// How many `\\(` and `\\[`, and how many `` $` ``, have no closing delimiter of their own behind them. Asked at every
/// keystroke, so it looks only at the characters they all contain, which the search for one character passes quickly.
fn unclosed_formulas(text: &str) -> (usize, usize) {
    let bytes = text.as_bytes();
    let (mut round, mut square, mut latex) = (0usize, 0usize, 0usize);
    for (at, _) in text.rmatch_indices('\\') {
        let mut count = |closers: &mut usize| match closers.checked_sub(1) {
            Some(left) => *closers = left,
            None => latex += 1,
        };
        match bytes.get(at + 1) {
            Some(b')') => round += 1,
            Some(b']') => square += 1,
            Some(b'(') => count(&mut round),
            Some(b'[') => count(&mut square),
            _ => {}
        }
    }
    let (mut ticks, mut code) = (0usize, 0usize);
    for (at, _) in text.rmatch_indices('$') {
        if at > 0 && bytes[at - 1] == b'`' {
            ticks += 1;
        } else if bytes.get(at + 1) == Some(&b'`') {
            match ticks.checked_sub(1) {
                Some(left) => ticks = left,
                None => code += 1,
            }
        }
    }
    (latex, code)
}

/// How many `open` have no `close` of their own behind them; a `close` in front of an `open` closes nothing.
fn unclosed(text: &str, open: &str, close: &str) -> usize {
    let (mut closers, mut unclosed) = (0usize, 0usize);
    let bytes = text.as_bytes();
    for i in (0..bytes.len()).rev() {
        if bytes[i..].starts_with(close.as_bytes()) {
            closers += 1;
        } else if bytes[i..].starts_with(open.as_bytes()) {
            if closers > 0 {
                closers -= 1;
            } else {
                unclosed += 1;
            }
        }
    }
    unclosed
}

/// Whether `limit_inline_notes` could apply to `text`, whatever its number of `]`.
pub(super) fn inline_notes_may_be_limited(text: &str) -> bool {
    text.matches("^[").count() > UNCLOSED_NOTES
}

pub fn parse(text: &str) -> Doc {
    Doc {
        blocks: parse_part(text, true),
    }
}

/// The top-level blocks of a part of a document; only the document's start may hold front matter.
pub fn parse_part(text: &str, at_start: bool) -> Vec<Block> {
    let arena = Arena::new();
    let mut opts = options();
    if !at_start {
        opts.extension.front_matter_delimiter = None;
    }
    limit_inline_notes(&mut opts, text);
    limit_formulas(&mut opts, text);
    let root = parse_document(&arena, text, &opts);
    let lines = LineStarts::new(text);
    let cx = Reader {
        text,
        lines: &lines,
        depth: std::cell::Cell::new(0),
        escapes: Default::default(),
    };
    let mut blocks = Vec::new();
    // comrak lists footnotes in the order of their references; an inline one keeps its place among them.
    let mut inline_notes = Vec::new();
    for node in root.children() {
        match &node.data().value {
            NodeValue::FootnoteDefinition(f) if f.name.starts_with(INLINE_NOTE) => {
                inline_notes.push((blocks.len() + inline_notes.len(), f.name.clone(), node));
            }
            _ => blocks.extend(cx.block(node)),
        }
    }
    if !inline_notes.is_empty() {
        let mut references = HashMap::new();
        walk_blocks(&blocks, &mut |b| {
            walk_inlines(b.inlines(), &mut |i| {
                if let InlineKind::FootnoteRef { name, ix } = &i.kind {
                    references.insert(&**name, (i.range.clone(), *ix));
                }
            })
        });
        let notes: Vec<(usize, Option<Block>)> = inline_notes
            .into_iter()
            .map(|(at, name, node)| {
                (
                    at,
                    inline_note(text, references.get(name.as_str()).cloned(), &name, node),
                )
            })
            .collect();
        let mut rest = std::mem::take(&mut blocks).into_iter();
        for (at, note) in notes {
            if let Some(note) = note {
                blocks.extend(rest.by_ref().take(at.saturating_sub(blocks.len())));
                blocks.push(note);
            }
        }
        blocks.extend(rest);
    }
    blocks.shrink_to_fit();
    blocks
}

/// comrak's name for the footnote an inline `^[note]` stands for.
const INLINE_NOTE: &str = "__inline_";

/// The footnote behind an inline `^[note]`. comrak gives its content positions relative to the note text, so it is
/// converted from that text and moved to where it stands; the definition covers the reference.
fn inline_note<'a>(
    text: &str,
    reference: Option<(Range<usize>, u32)>,
    name: &str,
    node: comrak::nodes::Node<'a>,
) -> Option<Block> {
    let (at, number) = reference?;
    let content = (at.start + 2).min(at.end)..at.end.saturating_sub(1).max((at.start + 2).min(at.end));
    let note = text.get(content.clone())?;
    // Over several lines comrak counts positions from quote and list prefixes it removed, so such a note is kept
    // as plain text.
    if note.contains('\n') {
        let text = Inline {
            kind: InlineKind::Text(note.into()),
            range: content.clone(),
            children: Box::default(),
        };
        let paragraph = Block::leaf(BlockKind::Paragraph, content.clone(), [text]);
        return Some(Block::container(
            BlockKind::FootnoteDefinition {
                name: name.into(),
                label: at.start..at.start,
                number: Some(number),
            },
            at,
            [paragraph],
        ));
    }
    let lines = LineStarts::new(note);
    let cx = Reader {
        text: note,
        lines: &lines,
        depth: std::cell::Cell::new(0),
        escapes: Default::default(),
    };
    let children: Vec<Block> = node
        .children()
        .filter_map(|c| cx.block(c))
        .map(|mut b| {
            super::incremental::shift_block(&mut b, super::text::growth(content.start, 0));
            b
        })
        .collect();
    Some(Block::container(
        BlockKind::FootnoteDefinition {
            name: name.into(),
            label: at.start..at.start,
            number: Some(number),
        },
        at,
        children,
    ))
}

fn alert_kind(alert: AlertType) -> AlertKind {
    match alert {
        AlertType::Note => AlertKind::Note,
        AlertType::Tip => AlertKind::Tip,
        AlertType::Important => AlertKind::Important,
        AlertType::Warning => AlertKind::Warning,
        AlertType::Caution => AlertKind::Caution,
    }
}

fn table_align(alignment: &TableAlignment) -> Align {
    match alignment {
        TableAlignment::None => Align::None,
        TableAlignment::Left => Align::Left,
        TableAlignment::Center => Align::Center,
        TableAlignment::Right => Align::Right,
    }
}

/// A callout type as the alert kind closest to it, and whether it is one of GitHub's.
fn callout_kind(name: &str) -> (AlertKind, bool) {
    match name.to_ascii_lowercase().as_str() {
        "note" => (AlertKind::Note, true),
        "tip" => (AlertKind::Tip, true),
        "important" => (AlertKind::Important, true),
        "warning" => (AlertKind::Warning, true),
        "caution" => (AlertKind::Caution, true),
        "hint" | "success" | "check" | "done" => (AlertKind::Tip, false),
        "question" | "help" | "faq" => (AlertKind::Important, false),
        "attention" => (AlertKind::Warning, false),
        "failure" | "fail" | "missing" | "danger" | "error" | "bug" => (AlertKind::Caution, false),
        _ => (AlertKind::Note, false),
    }
}

fn capitalized(name: &str) -> String {
    let lower = name.to_lowercase();
    let mut chars = lower.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
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

/// The formula or diagram a code block with this info string holds.
pub fn code_figure<'a>(info: &str, literal: &'a str) -> Option<Figure<'a>> {
    match diagram_kind(info) {
        Some(kind) => Some(Figure::Diagram { source: literal, kind }),
        None if is_math_fence(info) => Some(Figure::Math {
            tex: literal.strip_suffix('\n').unwrap_or(literal),
            display: true,
        }),
        None => None,
    }
}

/// File names Obsidian embeds as pictures.
pub fn is_image_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg", ".heic", ".tif", ".tiff", ".avif",
    ]
    .iter()
    .any(|e| lower.ends_with(e))
}

fn node_text<'a>(node: comrak::nodes::Node<'a>) -> String {
    node.descendants()
        .filter_map(|n| match &n.data().value {
            NodeValue::Text(t) => Some(t.to_string()),
            NodeValue::Code(c) => Some(c.literal.clone()),
            NodeValue::SoftBreak | NodeValue::LineBreak => Some(" ".to_string()),
            _ => None,
        })
        .collect()
}

/// Nesting beyond this stays raw text: every consumer of the tree recurses, and a file with thousands of nested
/// quotes would otherwise overflow the stack.
const MAX_DEPTH: usize = 64;

struct Reader<'t> {
    text: &'t str,
    lines: &'t LineStarts,
    depth: std::cell::Cell<usize>,
    /// The `\|` of the table cell being read.
    escapes: std::cell::RefCell<Vec<usize>>,
}

struct Level<'c>(&'c std::cell::Cell<usize>);

impl Drop for Level<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

impl Reader<'_> {
    fn enter(&self) -> Option<Level<'_>> {
        let d = self.depth.get();
        (d < MAX_DEPTH).then(|| {
            self.depth.set(d + 1);
            Level(&self.depth)
        })
    }

    fn raw_text(&self, range: Range<usize>) -> Inline {
        Inline {
            kind: InlineKind::Text(self.text[range.clone()].into()),
            range,
            children: Box::default(),
        }
    }

    fn range(&self, sp: Sourcepos) -> Range<usize> {
        let escapes = self.escapes.borrow();
        let start =
            behind_escaped_pipes(&escapes, self.lines.start(sp.start.line, sp.start.column)).min(self.text.len());
        let end = behind_escaped_pipes(&escapes, self.lines.end(sp.end.line, sp.end.column)).min(self.text.len());
        let start = floor_char(self.text, start);
        let end = ceil_char(self.text, end.max(start));
        start..end
    }

    /// The inlines of a leaf, their positions checked against the source; if those cannot be told, the source as it
    /// stands, which shows the markup but edits what it says.
    fn leaf_inlines<'a>(&self, node: comrak::nodes::Node<'a>, range: &Range<usize>) -> Vec<Inline> {
        super::positions::checked(self.text, range, self.inlines(node)).unwrap_or_else(|| {
            vec![Inline {
                kind: InlineKind::Text(self.text[range.clone()].replace(['\r', '\n'], " ").into()),
                range: range.clone(),
                children: Box::default(),
            }]
        })
    }

    /// A paragraph, or the formula it holds and nothing else.
    fn paragraph<'a>(&self, node: comrak::nodes::Node<'a>, mut range: Range<usize>) -> Block {
        let inlines = self.leaf_inlines(node, &range);
        // Link reference definitions at its start are part of comrak's paragraph, but of nothing it shows.
        if let Some(first) = inlines.first() {
            range.start = range.start.max(first.range.start.min(range.end));
        }
        if let [only] = inlines.as_slice()
            && let InlineKind::Math { display: true, literal } = &only.kind
        {
            return Block::leaf(
                BlockKind::MathBlock {
                    literal: literal.clone(),
                },
                range,
                Box::default(),
            );
        }
        Block::leaf(BlockKind::Paragraph, range, inlines)
    }

    fn callout<'a>(&self, node: comrak::nodes::Node<'a>, callout_type: &str, range: &Range<usize>) -> BlockKind {
        let (kind, github) = callout_kind(callout_type);
        let custom = node
            .children()
            .find(|n| matches!(n.data().value, NodeValue::CalloutTitle))
            .map(node_text)
            .filter(|t| !t.trim().is_empty());
        let title = custom.or_else(|| (!github).then(|| capitalized(callout_type)));
        let marker = self.alert_marker(range);
        let sign = self.text[marker.clone()].find("]").map(|i| marker.start + i + 1);
        let fold = sign.and_then(|at| match self.text.as_bytes().get(at) {
            Some(b'-') => Some(Fold { open: false, sign: at }),
            Some(b'+') => Some(Fold { open: true, sign: at }),
            _ => None,
        });
        BlockKind::Alert {
            kind,
            title: title.map(Into::into),
            marker,
            fold,
        }
    }

    fn list_kind(&self, list: &comrak::nodes::NodeList, range: &Range<usize>) -> ListKind {
        match list.list_type {
            ListType::Bullet => ListKind::Bullet(self.text[range.start..].chars().next().unwrap_or('-')),
            ListType::Ordered => ListKind::Ordered {
                start: list.start,
                delim: match list.delimiter {
                    ListDelimType::Period => '.',
                    ListDelimType::Paren => ')',
                },
            },
        }
    }

    fn block<'a>(&self, node: comrak::nodes::Node<'a>) -> Option<Block> {
        let data = node.data();
        let range = self.range(data.sourcepos);
        let Some(_level) = self.enter() else {
            let raw = self.raw_text(range.clone());
            return Some(Block::leaf(BlockKind::Paragraph, range, [raw]));
        };
        let kind = match &data.value {
            NodeValue::Paragraph => return Some(self.paragraph(node, range)),
            NodeValue::Heading(h) => {
                let attributes = data.attrs.as_ref();
                let kind = BlockKind::Heading {
                    level: h.level,
                    setext: h.setext,
                    id: attributes.and_then(|a| a.id.as_deref()).map(Into::into),
                    attrs: attributes.and_then(|_| self.heading_attrs(&range)),
                };
                let inlines = self.leaf_inlines(node, &range);
                return Some(Block::leaf(kind, range, inlines));
            }
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => BlockKind::BlockQuote,
            NodeValue::Alert(a) => BlockKind::Alert {
                kind: alert_kind(a.alert_type),
                title: a.title.as_deref().map(Into::into),
                marker: self.alert_marker(&range),
                fold: None,
            },
            NodeValue::Callout(c) => self.callout(node, &c.callout_type, &range),
            NodeValue::CalloutTitle => return None,
            NodeValue::DescriptionList => BlockKind::DefinitionList,
            NodeValue::DescriptionItem(_) => BlockKind::DefinitionItem,
            NodeValue::DescriptionTerm => BlockKind::DefinitionTerm,
            NodeValue::DescriptionDetails => BlockKind::DefinitionDetails,
            NodeValue::MultilineComment(_) => BlockKind::Comment,
            NodeValue::List(l) => BlockKind::List {
                kind: self.list_kind(l, &range),
                tight: l.tight,
            },
            NodeValue::Item(_) => BlockKind::Item {
                task: None,
                marker: self.marker(range.start),
            },
            NodeValue::TaskItem(t) => BlockKind::Item {
                task: Some(Task {
                    checked: t.symbol.is_some(),
                    symbol: self.range(t.symbol_sourcepos).start,
                }),
                marker: self.marker(range.start),
            },
            NodeValue::CodeBlock(c) => {
                let content = self.code_content(&range, c.fenced);
                BlockKind::CodeBlock {
                    info: c.info.as_str().into(),
                    literal: c.literal.as_str().into(),
                    fenced: c.fenced,
                    content,
                }
            }
            NodeValue::HtmlBlock(h) => BlockKind::HtmlBlock {
                literal: h.literal.as_str().into(),
            },
            NodeValue::ThematicBreak => BlockKind::ThematicBreak,
            NodeValue::FrontMatter(_) => BlockKind::FrontMatter {
                content: self.front_matter_content(&range),
            },
            NodeValue::Table(t) => BlockKind::Table {
                alignments: t.alignments.iter().map(table_align).collect(),
            },
            NodeValue::TableRow(header) => BlockKind::TableRow { header: *header },
            NodeValue::TableCell => return Some(self.table_cell(node, range)),
            NodeValue::FootnoteDefinition(f) => {
                let label_end = self.text[range.clone()].find("]:").map(|i| range.start + i + 2);
                BlockKind::FootnoteDefinition {
                    name: f.name.as_str().into(),
                    label: range.start..label_end.unwrap_or(range.start),
                    number: None,
                }
            }
            _ => return None,
        };
        drop(data);
        let children: Vec<Block> = node.children().filter_map(|c| self.block(c)).collect();
        Some(Block::container(kind, range, children))
    }

    /// What stands between the delimiter lines of front matter.
    fn front_matter_content(&self, range: &Range<usize>) -> Range<usize> {
        let slice = &self.text[range.clone()];
        let start = range.start + slice.find('\n').map_or(slice.len(), |i| i + 1);
        let end = range.start + slice.trim_end().rfind('\n').map_or(0, |i| i + 1);
        start..end.max(start)
    }

    fn table_cell<'a>(&self, node: comrak::nodes::Node<'a>, range: Range<usize>) -> Block {
        *self.escapes.borrow_mut() = self.text[range.clone()]
            .match_indices("\\|")
            .map(|(i, _)| range.start + i)
            .collect();
        let inlines = self.leaf_inlines(node, &range);
        self.escapes.borrow_mut().clear();
        Block::leaf(BlockKind::TableCell, range, inlines)
    }

    /// The `{…}` at the end of a heading's first line.
    fn heading_attrs(&self, range: &Range<usize>) -> Option<Range<usize>> {
        let line = &self.text[range.clone()];
        let line = &line[..line.find('\n').unwrap_or(line.len())];
        let trimmed = line.trim_end().trim_end_matches('#').trim_end();
        let open = trimmed.ends_with('}').then(|| trimmed.rfind('{')).flatten()?;
        Some(range.start + open..range.start + trimmed.len())
    }

    fn alert_marker(&self, range: &Range<usize>) -> Range<usize> {
        let first_line = self.text[range.clone()]
            .find('\n')
            .map_or(range.end, |i| range.start + i);
        let start = self.text[range.start..first_line]
            .find("[!")
            .map_or(range.start, |i| range.start + i);
        let end = range.start + self.text[range.start..first_line].trim_end().len();
        start..end.max(start)
    }

    fn marker(&self, start: usize) -> Range<usize> {
        let rest = &self.text[start..];
        let len = match rest.chars().next() {
            Some('-' | '*' | '+') => 1,
            _ => {
                let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
                digits + usize::from(matches!(rest.as_bytes().get(digits), Some(b'.' | b')')))
            }
        };
        start..start + len
    }

    fn code_content(&self, range: &Range<usize>, fenced: bool) -> Range<usize> {
        let slice = &self.text[range.clone()];
        if !fenced {
            return range.clone();
        }
        let start = slice.find('\n').map(|i| range.start + i + 1).unwrap_or(range.end);
        let trimmed = slice.trim_end_matches('\n');
        let last_line_start = trimmed.rfind('\n').map(|i| range.start + i + 1).unwrap_or(range.start);
        let last_line = self.text[last_line_start..range.start + trimmed.len()]
            .trim_start_matches(['>', ' ', '\t'])
            .trim_end();
        let closed = last_line_start > range.start
            && (last_line.starts_with("```") || last_line.starts_with("~~~"))
            && last_line.chars().all(|c| c == '`' || c == '~');
        let end = if closed { last_line_start } else { range.end };
        start.min(end)..end
    }

    fn inlines<'a>(&self, node: comrak::nodes::Node<'a>) -> Vec<Inline> {
        let mut raw: Vec<Inline> = node.children().filter_map(|c| self.inline(c)).collect();
        // comrak's positions for a few extensions, such as inline footnotes over several lines, can run past the
        // next inline; everything after the tree relies on inlines following each other.
        let mut end = 0;
        for i in &mut raw {
            if i.range.start < end {
                i.range.start = end.min(i.range.end.max(end));
            }
            i.range.end = i.range.end.max(i.range.start);
            end = i.range.end;
            keep_inside(&mut i.children, &i.range);
        }
        pair_html(raw)
    }

    /// The end of an inline footnote `^[…]`, found by its brackets: comrak's end position is unreliable.
    fn inline_note_end(&self, start: usize) -> Option<usize> {
        let rest = self.text.get(start..)?.strip_prefix("^[")?;
        let mut depth = 1;
        let mut chars = rest.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => {
                    chars.next();
                }
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(start + 2 + i + 1);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn inline<'a>(&self, node: comrak::nodes::Node<'a>) -> Option<Inline> {
        let data = node.data();
        let mut range = self.range(data.sourcepos);
        if let NodeValue::FootnoteReference(f) = &data.value
            && f.name.starts_with(INLINE_NOTE)
            && let Some(end) = self.inline_note_end(range.start)
        {
            range.end = end;
        }
        let Some(_level) = self.enter() else {
            return Some(self.raw_text(range));
        };
        let kind = match &data.value {
            NodeValue::Text(t) => InlineKind::Text(t.as_ref().into()),
            NodeValue::SoftBreak => InlineKind::SoftBreak,
            NodeValue::LineBreak => InlineKind::LineBreak,
            NodeValue::Code(c) => InlineKind::Code(c.literal.as_str().into()),
            NodeValue::HtmlInline(h) => InlineKind::Html(h.as_str().into()),
            NodeValue::Emph => InlineKind::Emph,
            NodeValue::Strong => InlineKind::Strong,
            NodeValue::Strikethrough => InlineKind::Strikethrough,
            NodeValue::Link(l) => InlineKind::Link {
                url: l.url.as_str().into(),
                title: l.title.as_str().into(),
            },
            NodeValue::Image(l) => InlineKind::Image {
                url: l.url.as_str().into(),
                title: l.title.as_str().into(),
            },
            NodeValue::FootnoteReference(f) => InlineKind::FootnoteRef {
                name: f.name.as_str().into(),
                ix: f.ix,
            },
            NodeValue::Math(m) => InlineKind::Math {
                display: m.display_math,
                literal: m.literal.as_str().into(),
            },
            NodeValue::Escaped => InlineKind::Text(self.text[range.clone()].trim_start_matches('\\').into()),
            NodeValue::Highlight => InlineKind::Highlight,
            NodeValue::Superscript => InlineKind::Superscript,
            NodeValue::Subscript => InlineKind::Subscript,
            NodeValue::SpoileredText => InlineKind::Spoiler,
            NodeValue::ShortCode(c) => InlineKind::Emoji(c.emoji.as_str().into()),
            NodeValue::WikiLink(w) => InlineKind::WikiLink {
                target: w.url.as_str().into(),
                embed: w.embed,
            },
            NodeValue::CommentInline(_) => InlineKind::Comment,
            // A single `|` pair in a table cell, which comrak reads after taking the backslashes out of `\|`.
            _ => InlineKind::Text(self.text[range.clone()].replace("\\|", "|").into()),
        };
        drop(data);
        let children = if kind.holds_inlines() {
            self.inlines(node).into()
        } else {
            Box::default()
        };
        Some(Inline { kind, range, children })
    }
}

/// Clamps the ranges of inlines into their container's.
fn keep_inside(children: &mut [Inline], outer: &Range<usize>) {
    for c in children {
        c.range.start = c.range.start.clamp(outer.start, outer.end);
        c.range.end = c.range.end.clamp(c.range.start, outer.end);
        let range = c.range.clone();
        keep_inside(&mut c.children, &range);
    }
}

/// What a single inline HTML tag stands for: whether it closes, the container it opens or closes, or the inline it
/// is on its own.
enum HtmlTag {
    Container { closing: bool, kind: InlineKind },
    Alone(InlineKind),
}

fn html_tag(html: &str) -> Option<HtmlTag> {
    let trimmed = html.trim();
    if trimmed.starts_with("<!--") {
        return Some(HtmlTag::Alone(InlineKind::Comment));
    }
    let (tag, end) = super::markup::read_tag(trimmed, 0)?;
    if end != trimmed.len() {
        return None;
    }
    let (closing, name, attrs, self_closing) = match tag {
        super::markup::Tag::Open {
            name,
            attrs,
            self_closing,
        } => (false, name, attrs, self_closing),
        super::markup::Tag::Close { name } => (true, name, Vec::new(), false),
    };
    let attr = |key: &str| attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    let pixels = |key: &str| attr(key).and_then(super::markup::pixels);
    let kind = match name.as_str() {
        "br" if !closing => return Some(HtmlTag::Alone(InlineKind::LineBreak)),
        "img" if !closing => {
            let url = attr("src").filter(|u| super::url::is_harmless(u))?;
            return Some(HtmlTag::Alone(InlineKind::HtmlImage {
                url: url.into(),
                alt: attr("alt").unwrap_or("").into(),
                width: pixels("width"),
                height: pixels("height"),
            }));
        }
        _ if self_closing => return None,
        "a" => InlineKind::Link {
            url: attr("href").filter(|u| super::url::is_harmless(u)).unwrap_or("").into(),
            title: attr("title").unwrap_or("").into(),
        },
        name => super::style::styled_element(name)?,
    };
    Some(HtmlTag::Container { closing, kind })
}

/// Whether a closing tag ends the container an opening tag started: links match whatever their address.
fn same_container(open: &InlineKind, close: &InlineKind) -> bool {
    std::mem::discriminant(open) == std::mem::discriminant(close)
}

/// Inline HTML nested deeper stays raw: the containers `pair_html` makes add to `MAX_DEPTH`, which comrak's own nesting
/// uses up on its own.
const MAX_PAIRED: usize = 16;

/// Groups inline HTML such as `<b>…</b>`, `<a href>…</a>` or `<mark>…</mark>` into container inlines and turns
/// `<br>`, `<img>` and comments into their own inlines; unmatched tags stay raw HTML.
fn pair_html(raw: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    let mut open: Vec<(usize, InlineKind)> = Vec::new();
    for mut inline in raw {
        let tag = match &inline.kind {
            InlineKind::Html(h) => html_tag(h),
            _ => None,
        };
        if let Some(HtmlTag::Alone(kind)) = tag {
            inline.kind = kind;
            out.push(inline);
            continue;
        }
        if let Some(HtmlTag::Container { closing, kind }) = tag {
            if !closing {
                if open.len() < MAX_PAIRED {
                    open.push((out.len(), kind));
                }
                out.push(inline);
                continue;
            }
            if let Some(pos) = open.iter().rposition(|(_, k)| same_container(k, &kind)) {
                let (idx, kind) = open.remove(pos);
                open.truncate(pos);
                // `<a>` without an address that may be followed is no link to anyone.
                let kind = match kind {
                    InlineKind::Link { url, .. } if url.is_empty() => InlineKind::Span,
                    kind => kind,
                };
                let mut children: Vec<Inline> = out.drain(idx..).collect();
                let opener = children.remove(0);
                if children.is_empty() {
                    children.push(Inline {
                        kind: InlineKind::Text("".into()),
                        range: opener.range.end..opener.range.end,
                        children: Box::default(),
                    });
                }
                out.push(Inline {
                    kind,
                    range: opener.range.start..inline.range.end,
                    children: children.into(),
                });
                continue;
            }
        }
        out.push(inline);
    }
    out
}

/// comrak removes the backslash of `\|` inside table cells before parsing inlines, so its positions behind each escape
/// are one byte short.
fn behind_escaped_pipes(escapes: &[usize], offset: usize) -> usize {
    let (mut low, mut high) = (0, escapes.len());
    while low < high {
        let mid = (low + high) / 2;
        if escapes[mid] - mid < offset {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    offset + low
}

fn floor_char(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

impl Inline {
    /// The source range inside the backticks of a code span.
    pub fn code_content(&self, text: &str) -> Range<usize> {
        let src = &text[self.range.clone()];
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
    pub fn content_range(&self) -> Range<usize> {
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
    pub fn leaf(kind: BlockKind, range: Range<usize>, inlines: impl Into<Box<[Inline]>>) -> Block {
        Block {
            kind,
            range,
            content: Content::Inlines(inlines.into()),
        }
    }

    pub fn container(kind: BlockKind, range: Range<usize>, children: impl Into<Box<[Block]>>) -> Block {
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
    pub fn inline_range(&self) -> Option<Range<usize>> {
        Some(self.inlines().first()?.range.start..self.inlines().last()?.range.end)
    }
}

/// Path from the document root to the innermost block containing `offset`.
pub fn block_path(blocks: &[Block], offset: usize) -> Vec<&Block> {
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
pub fn inline_path(inlines: &[Inline], offset: usize) -> Vec<&Inline> {
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

/// A formula or diagram, which the preview shows as a rendered image.
pub enum Figure<'a> {
    Math { tex: &'a str, display: bool },
    Diagram { source: &'a str, kind: DiagramKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagramKind {
    Mermaid,
    Graphviz,
    PlantUml,
}

/// The diagram language a code block's info string names.
pub fn diagram_kind(info: &str) -> Option<DiagramKind> {
    match info.split_whitespace().next()? {
        "mermaid" => Some(DiagramKind::Mermaid),
        "dot" | "graphviz" => Some(DiagramKind::Graphviz),
        "plantuml" | "puml" => Some(DiagramKind::PlantUml),
        _ => None,
    }
}

/// A ` ```math ` block, as GitHub and GitLab write display formulas.
pub fn is_math_fence(info: &str) -> bool {
    info.split_whitespace().next() == Some("math")
}

/// Lengths of the opening and closing delimiter of a formula's source: `$`, `$$`, `` $` ``, `\(` or `\[`.
pub fn math_delimiters(src: &str) -> (usize, usize) {
    for (open, close) in [("$$", "$$"), ("$`", "`$"), ("\\(", "\\)"), ("\\[", "\\]"), ("$", "$")] {
        if src.len() >= open.len() + close.len() && src.starts_with(open) && src.ends_with(close) {
            return (open.len(), close.len());
        }
    }
    (0, 0)
}

/// Every formula and diagram with its source range, outer ones before those inside them.
pub fn figures(doc: &Doc) -> Vec<(Range<usize>, Figure<'_>)> {
    let mut out = Vec::new();
    walk_blocks(&doc.blocks, &mut |b| {
        match &b.kind {
            BlockKind::MathBlock { literal } => out.push((
                b.range.clone(),
                Figure::Math {
                    tex: literal,
                    display: true,
                },
            )),
            BlockKind::CodeBlock { info, literal, .. } => {
                if let Some(kind) = diagram_kind(info) {
                    out.push((b.range.clone(), Figure::Diagram { source: literal, kind }));
                } else if is_math_fence(info) {
                    out.push((
                        b.range.clone(),
                        Figure::Math {
                            tex: literal.strip_suffix('\n').unwrap_or(literal),
                            display: true,
                        },
                    ));
                }
            }
            _ => {}
        }
        walk_inlines(b.inlines(), &mut |i| {
            if let InlineKind::Math { display, literal } = &i.kind {
                out.push((
                    i.range.clone(),
                    Figure::Math {
                        tex: literal,
                        display: *display,
                    },
                ));
            }
        });
    });
    out
}

/// Whether the document shows images from the web, which the preview loads only once allowed.
pub fn has_remote_images(doc: &Doc) -> bool {
    fn in_html(nodes: &[super::markup::Node]) -> bool {
        nodes.iter().any(|n| match n {
            super::markup::Node::Element(e) => {
                (e.name == "img" && e.attr("src").is_some_and(super::url::is_https)) || in_html(&e.children)
            }
            super::markup::Node::Text { .. } => false,
        })
    }
    let mut found = false;
    walk_blocks(&doc.blocks, &mut |b| {
        if let BlockKind::HtmlBlock { literal } = &b.kind {
            found |= literal.contains("://") && in_html(&super::markup::parse(literal));
        }
        walk_inlines(b.inlines(), &mut |i| {
            found |= matches!(
                &i.kind,
                InlineKind::Image { url, .. } | InlineKind::HtmlImage { url, .. } if super::url::is_https(url)
            );
        });
    });
    found
}

/// The innermost formula or diagram whose source contains byte offset `at`, ends included.
pub fn figure_at(doc: &Doc, at: usize) -> Option<(Range<usize>, Figure<'_>)> {
    figures(doc)
        .into_iter()
        .rev()
        .find(|(r, _)| r.start <= at && at <= r.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(inlines: &[Inline]) -> Vec<String> {
        inlines.iter().map(|i| format!("{:?}", i.kind)).collect()
    }

    #[test]
    fn inline_html_nested_beyond_the_limit_stays_raw() {
        let text = "<b>".repeat(MAX_PAIRED + 3) + "x" + &"</b>".repeat(MAX_PAIRED + 3);
        fn depth(inlines: &[Inline]) -> usize {
            inlines.iter().map(|i| 1 + depth(&i.children)).max().unwrap_or(0)
        }
        let doc = parse(&text);
        assert!(depth(doc.blocks[0].inlines()) <= MAX_PAIRED + 1);
    }

    #[test]
    fn inline_notes_stand_where_the_order_of_references_puts_them() {
        let text: String = (0..50).map(|n| format!("Absatz {n}^[Notiz {n}]\n\n")).collect();
        let doc = parse(&text);
        let notes: Vec<&Block> = doc
            .blocks
            .iter()
            .filter(|b| matches!(b.kind, BlockKind::FootnoteDefinition { .. }))
            .collect();
        assert_eq!(notes.len(), 50);
        for (n, note) in notes.iter().enumerate() {
            assert_eq!(&text[note.range.clone()], format!("^[Notiz {n}]"));
        }
    }

    #[test]
    fn thousands_of_unclosed_inline_notes_are_plain_text() {
        let text = "^[a ".repeat(1500);
        let doc = parse(&text);
        assert_eq!(doc.blocks.len(), 1);
        assert_eq!(parse("Erst^[inline]").blocks.len(), 2);
    }

    #[test]
    fn a_picture_on_the_web_is_one_whatever_the_case_of_its_address() {
        assert!(has_remote_images(&parse("![x](HTTPS://a.de/b.png)\n")));
        assert!(has_remote_images(&parse("<img src=\"HTTPS://a.de/b.png\">\n")));
        assert!(
            !has_remote_images(&parse("![x](http://a.de/b.png)\n")),
            "nothing Splat would load"
        );
    }

    #[test]
    fn an_html_link_without_an_address_is_no_link() {
        let doc = parse("<a>x</a> <a href=\"javascript:y\">z</a>\n");
        assert!(matches!(doc.blocks[0].inlines()[0].kind, InlineKind::Span));
    }

    #[test]
    fn only_formulas_without_an_end_are_given_up() {
        let math = |d: &Doc| {
            let mut found = 0;
            walk_blocks(&d.blocks, &mut |b| {
                walk_inlines(b.inlines(), &mut |i| {
                    found += usize::from(matches!(i.kind, InlineKind::Math { .. }))
                })
            });
            found
        };
        let closed = "\\(x\\) ".repeat(1500);
        assert_eq!(math(&parse(&closed)), 1500);
        let open = format!("\\(x\\) {}", "\\( ".repeat(1500));
        assert_eq!(math(&parse(&open)), 0);
        assert!(!formulas_are_limited(&"\\* ".repeat(1500)));
        assert!(!formulas_are_limited(&"\\(x\\) ".repeat(1500)), "all of them closed");
        assert!(formulas_are_limited(&"\\( ".repeat(1500)));
        assert!(formulas_are_limited(&"$` ".repeat(1500)));
        assert!(!formulas_are_limited(&"$`x`$ ".repeat(1500)));
    }

    #[test]
    fn extended_syntax_becomes_its_own_nodes() {
        let text = "# Titel {#eigene-id .k}\n\n\
                    ==hell== x^2^ H~2~O ~~weg~~ :rocket: ||geheim|| %%still%% [[Seite|Titel]] ![[bild.png]] $`a+b`$ \\(x\\) ^[Notiz *fett*]\n\n\
                    ```math\nx^2\n```\n\nBegriff\n\n: Erklärung\n\n> [!bug]- Absturz\n> Inhalt\n\n> [!NOTE]\n> GitHub\n\n%%\nBlock\n%%\n";
        let doc = parse(text);
        let BlockKind::Heading { id, attrs, .. } = &doc.blocks[0].kind else {
            panic!()
        };
        assert_eq!(id.as_deref(), Some("eigene-id"));
        assert_eq!(&text[attrs.clone().unwrap()], "{#eigene-id .k}");
        assert_eq!(kinds(doc.blocks[0].inlines()), ["Text(\"Titel\")"]);
        let p = doc.blocks[1].inlines();
        let find = |k: &str| p.iter().find(|i| format!("{:?}", i.kind).starts_with(k)).unwrap();
        assert_eq!(&text[find("Highlight").content_range()], "hell");
        assert_eq!(&text[find("Superscript").content_range()], "2");
        assert_eq!(&text[find("Subscript").range.clone()], "~2~");
        assert_eq!(&text[find("Strikethrough").range.clone()], "~~weg~~");
        assert_eq!(find("Emoji").kind, InlineKind::Emoji("🚀".into()));
        assert_eq!(&text[find("Spoiler").content_range()], "geheim");
        assert_eq!(&text[find("Comment").range.clone()], "%%still%%");
        let InlineKind::WikiLink { target, embed: false } = &find("WikiLink").kind else {
            panic!()
        };
        assert_eq!(&**target, "Seite");
        assert!(p.iter().any(|i| i.kind
            == InlineKind::WikiLink {
                target: "bild.png".into(),
                embed: true
            }));
        let maths: Vec<&str> = p
            .iter()
            .filter(|i| matches!(i.kind, InlineKind::Math { .. }))
            .map(|i| &text[i.range.clone()])
            .collect();
        assert_eq!(maths, ["$`a+b`$", "\\(x\\)"]);
        assert_eq!(math_delimiters("$`a+b`$"), (2, 2));
        assert_eq!(math_delimiters("\\(x\\)"), (2, 2));
        assert!(figures(&doc).iter().any(|(_, f)| matches!(
            f,
            Figure::Math {
                tex: "x^2",
                display: true
            }
        )));
        let list = doc.blocks.iter().find(|b| b.kind == BlockKind::DefinitionList).unwrap();
        let parts: Vec<&BlockKind> = list.children()[0].children().iter().map(|b| &b.kind).collect();
        assert_eq!(parts, [&BlockKind::DefinitionTerm, &BlockKind::DefinitionDetails]);
        let alerts: Vec<&BlockKind> = doc
            .blocks
            .iter()
            .map(|b| &b.kind)
            .filter(|k| matches!(k, BlockKind::Alert { .. }))
            .collect();
        let BlockKind::Alert {
            kind,
            title,
            fold: Some(fold),
            ..
        } = alerts[0]
        else {
            panic!("{alerts:?}")
        };
        assert_eq!(
            (*kind, title.as_deref(), fold.open, &text[fold.sign..fold.sign + 1]),
            (AlertKind::Caution, Some("Absturz"), false, "-")
        );
        assert!(matches!(
            alerts[1],
            BlockKind::Alert {
                kind: AlertKind::Note,
                title: None,
                fold: None,
                ..
            }
        ));
        assert!(doc.blocks.iter().any(|b| b.kind == BlockKind::Comment));
        let note = doc.blocks.last().unwrap();
        assert!(matches!(&note.kind, BlockKind::FootnoteDefinition { name, .. } if name.starts_with(INLINE_NOTE)));
        let inner = note.children()[0].inlines();
        assert_eq!(&text[inner[0].range.clone()], "Notiz ");
        assert_eq!(&text[inner[1].range.clone()], "*fett*");
        assert_eq!(&text[note.range.clone()], "^[Notiz *fett*]");
        // Footnotes follow their references, so a definition at the end of the text stays last.
        let ordered = parse("Erst^[inline] dann[^1].\n\n[^1]: Letzte.\n");
        let names: Vec<&str> = ordered
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::FootnoteDefinition { name, .. } => Some(&**name),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["__inline_1", "1"]);
    }

    #[test]
    fn front_matter_is_one_block_before_the_text() {
        let text = "---\ntitle: Splat\ntags: [a, b]\n---\n\n# Kopf\n";
        let doc = parse(text);
        let BlockKind::FrontMatter { content } = &doc.blocks[0].kind else {
            panic!("{:?}", doc.blocks[0].kind);
        };
        assert_eq!(&text[content.clone()], "title: Splat\ntags: [a, b]\n");
        assert_eq!(
            &text[doc.blocks[0].range.clone()],
            "---\ntitle: Splat\ntags: [a, b]\n---"
        );
        assert!(matches!(doc.blocks[1].kind, BlockKind::Heading { level: 1, .. }));
        // Further down, `---` stays a rule.
        assert!(matches!(
            parse("Text\n\n---\n").blocks[1].kind,
            BlockKind::ThematicBreak
        ));
    }

    fn slice<'a>(t: &'a str, r: &Range<usize>) -> &'a str {
        &t[r.clone()]
    }

    #[test]
    fn an_empty_html_pair_has_its_content_between_the_tags() {
        let t = "<b title=\"äää\"></b> x\n";
        let d = parse(t);
        let b = &d.blocks[0].inlines()[0];
        assert_eq!(b.kind, InlineKind::Strong);
        assert_eq!(slice(t, &b.content_range()), "");
        assert_eq!(b.content_range().start, t.find("</b>").unwrap());
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
        assert_eq!(ranges, [1..1, t.find("![").unwrap() + 2..t.find("![").unwrap() + 2]);
    }

    #[test]
    fn inlines_of_a_table_cell_find_their_place_behind_escaped_pipes() {
        for t in [
            "a|b\n-|-\n\\|ä",
            "a|b\n-|-\n\\|ä\n",
            "a|b\n-|-\n\\|ä**b**|x\n",
            "a|b\n-|-\nä\\|ö\\|ü `c`|x\n",
        ] {
            let d = parse(t);
            let mut seen = 0;
            walk_blocks(&d.blocks, &mut |b| {
                walk_inlines(b.inlines(), &mut |i| {
                    assert!(
                        t.is_char_boundary(i.range.start) && t.is_char_boundary(i.range.end),
                        "{t:?} {i:?}"
                    );
                    if let InlineKind::Strong = i.kind {
                        assert_eq!(slice(t, &i.range), "**b**");
                        seen += 1;
                    }
                    if let InlineKind::Code(_) = i.kind {
                        assert_eq!(slice(t, &i.range), "`c`");
                        seen += 1;
                    }
                    if let InlineKind::Text(text) = &i.kind {
                        assert_eq!(slice(t, &i.range).replace("\\|", "|"), &**text, "{t:?}");
                    }
                });
            });
            assert_eq!(
                seen,
                usize::from(t.contains("**")) + usize::from(t.contains('`')),
                "{t:?}"
            );
        }
    }

    #[test]
    fn inline_ranges_are_exact_with_multibyte_text() {
        let t = "Text **fett ü** und `co` <u>un</u> 😀 [li](http://x)\n";
        let d = parse(t);
        let p = &d.blocks[0];
        let strong = p.inlines().iter().find(|i| i.kind == InlineKind::Strong).unwrap();
        assert_eq!(slice(t, &strong.range), "**fett ü**");
        assert_eq!(slice(t, &strong.content_range()), "fett ü");
        let u = p.inlines().iter().find(|i| i.kind == InlineKind::Underline).unwrap();
        assert_eq!(slice(t, &u.range), "<u>un</u>");
        assert_eq!(slice(t, &u.content_range()), "un");
        let link = p
            .inlines()
            .iter()
            .find(|i| matches!(i.kind, InlineKind::Link { .. }))
            .unwrap();
        assert_eq!(slice(t, &link.range), "[li](http://x)");
    }

    #[test]
    fn blocks_and_markers() {
        let t =
            "# Ü\n\n- [x] done\n10. ten\n\n> q\n\n| a | b |\n|---|:-:|\n| 1 | 2 |\n\n```rs\nfn x\n```\n\n$$\ny\n$$\n";
        let d = parse(t);
        assert!(matches!(d.blocks[0].kind, BlockKind::Heading { level: 1, .. }));
        let item = &d.blocks[1].children()[0];
        match &item.kind {
            BlockKind::Item {
                task: Some(task),
                marker,
            } => {
                assert!(task.checked);
                assert_eq!(slice(t, marker), "-");
                assert_eq!(&t[task.symbol..task.symbol + 1], "x");
            }
            k => panic!("{k:?}"),
        }
        match &d.blocks[2].children()[0].kind {
            BlockKind::Item { marker, .. } => assert_eq!(slice(t, marker), "10."),
            k => panic!("{k:?}"),
        }
        assert!(matches!(d.blocks[3].kind, BlockKind::BlockQuote));
        match &d.blocks[4].kind {
            BlockKind::Table { alignments } => assert_eq!(&**alignments, &[Align::None, Align::Center]),
            k => panic!("{k:?}"),
        }
        match &d.blocks[5].kind {
            BlockKind::CodeBlock { content, info, .. } => {
                assert_eq!(&**info, "rs");
                assert_eq!(slice(t, content), "fn x\n");
            }
            k => panic!("{k:?}"),
        }
        assert!(matches!(d.blocks[6].kind, BlockKind::MathBlock { .. }));
    }

    #[test]
    fn alerts_and_highlights() {
        let t = "> [!WARNING] Achtung\n> Text <mark>wichtig</mark>\n\n> normal\n";
        let d = parse(t);
        match &d.blocks[0].kind {
            BlockKind::Alert {
                kind, title, marker, ..
            } => {
                assert_eq!(*kind, AlertKind::Warning);
                assert_eq!(title.as_deref(), Some("Achtung"));
                assert_eq!(slice(t, marker), "[!WARNING] Achtung");
            }
            k => panic!("{k:?}"),
        }
        let para = &d.blocks[0].children()[0];
        let mark = para.inlines().iter().find(|i| i.kind == InlineKind::Highlight).unwrap();
        assert_eq!(slice(t, &mark.content_range()), "wichtig");
        assert!(matches!(d.blocks[1].kind, BlockKind::BlockQuote));
    }

    #[test]
    fn deep_nesting_stays_bounded() {
        let t = format!("{}x\n", "> ".repeat(500));
        let d = parse(&t);
        let mut depth = 0;
        let mut level: &[Block] = &d.blocks;
        while let Some(b) = level.first() {
            depth += 1;
            level = b.children();
        }
        assert!(depth <= MAX_DEPTH + 1, "{depth}");
        let t = format!("{}x{}\n", "*".repeat(400), "*".repeat(400));
        parse(&t);
    }

    #[test]
    fn unmatched_html_stays_raw() {
        let d = parse("a <u>b\n");
        assert!(
            d.blocks[0]
                .inlines()
                .iter()
                .any(|i| matches!(i.kind, InlineKind::Html(_)))
        );
    }

    #[test]
    fn escaped_text_keeps_literal() {
        let t = "a \\* b &amp; c\n";
        let d = parse(t);
        let text: String = d.blocks[0]
            .inlines()
            .iter()
            .filter_map(|i| match &i.kind {
                InlineKind::Text(s) => Some(&**s),
                _ => None,
            })
            .collect();
        assert_eq!(text, "a * b & c");
    }

    #[test]
    fn web_images_in_markdown_and_html_need_permission() {
        let remote = |t: &str| has_remote_images(&parse(t));
        assert!(remote("![Logo](https://x.de/a.png)\n"));
        assert!(remote(
            "<p align=\"center\">\n  <a href=\"https://x.de\"><img src=\"https://x.de/b.svg\"></a>\n</p>\n"
        ));
        assert!(remote("Text <img src=\"https://x.de/c.png\" width=\"20\"> Text\n"));
        assert!(!remote(
            "<p align=\"center\">\n  <a href=\"https://x.de\"><img src=\"logo.png\"></a>\n</p>\n"
        ));
        assert!(!remote("![Logo](logo.png) [Web](https://x.de)\n"));
    }

    #[test]
    fn inline_notes_are_only_switched_off_when_far_more_open_than_close() {
        let on = |text: String| {
            let mut o = options();
            limit_inline_notes(&mut o, &text);
            o.extension.inline_footnotes
        };
        assert!(on("^[".repeat(1000)));
        assert!(!on("^[".repeat(1001)));
        assert!(!on("^[".repeat(1002)));
        assert!(on("^[".repeat(1001) + "]"));
        assert!(!on("^[".repeat(1002) + "]"));
    }

    #[test]
    fn inline_notes_keep_their_place_among_other_notes() {
        let t = "a^[x] b[^r] c^[y]\n\n[^r]: z\n";
        let d = parse(t);
        let notes: Vec<&str> = d
            .blocks
            .iter()
            .filter(|b| matches!(b.kind, BlockKind::FootnoteDefinition { .. }))
            .map(|b| slice(t, &b.range))
            .collect();
        assert_eq!(notes, ["^[x]", "[^r]: z", "^[y]"]);
    }

    #[test]
    fn a_note_with_brackets_ends_at_its_own_bracket() {
        let reference = |t: &str| {
            let d = parse(t);
            let i = d.blocks[0]
                .inlines()
                .iter()
                .find(|i| matches!(i.kind, InlineKind::FootnoteRef { .. }))
                .unwrap()
                .clone();
            t[i.range].to_string()
        };
        assert_eq!(reference("a^[b [c] d] e"), "^[b [c] d]");
        assert_eq!(reference("a^[b [c [d]] e] f"), "^[b [c [d]] e]");
        assert_eq!(reference(r"a^[b \] c] d"), r"^[b \] c]");
    }

    #[test]
    fn every_callout_type_finds_the_alert_kind_closest_to_it() {
        for (name, kind, github) in [
            ("note", AlertKind::Note, true),
            ("TIP", AlertKind::Tip, true),
            ("important", AlertKind::Important, true),
            ("warning", AlertKind::Warning, true),
            ("caution", AlertKind::Caution, true),
            ("hint", AlertKind::Tip, false),
            ("done", AlertKind::Tip, false),
            ("faq", AlertKind::Important, false),
            ("attention", AlertKind::Warning, false),
            ("bug", AlertKind::Caution, false),
            ("abstract", AlertKind::Note, false),
        ] {
            assert_eq!(callout_kind(name), (kind, github), "{name}");
        }
    }

    #[test]
    fn a_callout_without_a_title_shows_its_type_and_its_title_is_no_block() {
        let t = "> [!danger]\n> Text\n";
        let BlockKind::Alert { kind, title, .. } = &parse(t).blocks[0].kind.clone() else {
            panic!("no alert");
        };
        assert_eq!(*kind, AlertKind::Caution);
        assert_eq!(title.as_deref(), Some("Danger"));
        let t = "> [!NOTE] Eigener `Titel`\n> Text\n";
        let d = parse(t);
        let BlockKind::Alert { title, .. } = &d.blocks[0].kind else {
            panic!("no alert");
        };
        assert_eq!(title.as_deref(), Some("Eigener Titel"));
        assert_eq!(d.blocks[0].children().len(), 1);
        assert_eq!(slice(t, &d.blocks[0].children()[0].range), "Text");
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
    fn the_label_of_a_footnote_definition_ends_behind_its_colon() {
        let t = "x[^a]\n\n[^a]: b\n";
        let d = parse(t);
        let BlockKind::FootnoteDefinition { label, .. } = &d.blocks[1].kind else {
            panic!("no definition");
        };
        assert_eq!(slice(t, label), "[^a]:");
    }

    #[test]
    fn a_fence_with_text_behind_it_does_not_close_the_code() {
        let content = |t: &str| {
            let d = parse(t);
            let b = d
                .blocks
                .iter()
                .find(|b| matches!(b.kind, BlockKind::CodeBlock { .. }))
                .unwrap();
            let BlockKind::CodeBlock { content, .. } = &b.kind else {
                unreachable!();
            };
            t[content.clone()].to_string()
        };
        assert_eq!(content("```\na\n```\n"), "a\n");
        assert_eq!(content("```\na\n```x\n"), "a\n```x");
        assert_eq!(content("~~~\na\n~~~\n"), "a\n");
        assert_eq!(content("```\na\nb\n"), "a\nb");
        assert_eq!(content("x\n\n```"), "");
        let t = "x\n\n```";
        let d = parse(t);
        let BlockKind::CodeBlock { content, .. } = &d.blocks[1].kind else {
            panic!("no code");
        };
        assert_eq!(*content, t.len()..t.len());
    }

    #[test]
    fn a_hard_line_break_stays_a_break() {
        let d = parse("a  \nb\n");
        assert!(d.blocks[0].inlines().iter().any(|i| i.kind == InlineKind::LineBreak));
        let d = parse("a\nb\n");
        assert!(d.blocks[0].inlines().iter().any(|i| i.kind == InlineKind::SoftBreak));
    }

    #[test]
    fn inline_ranges_are_clamped_into_their_container() {
        let inline = |range: Range<usize>| Inline {
            kind: InlineKind::Emph,
            range,
            children: Box::default(),
        };
        let mut children = [Inline {
            children: Box::new([inline(0..50)]),
            ..inline(5..30)
        }];
        keep_inside(&mut children, &(10..20));
        assert_eq!(children[0].range, 10..20);
        assert_eq!(children[0].children[0].range, 10..20);
    }

    #[test]
    fn single_html_tags_become_what_they_stand_for() {
        let kinds_of = |t: &str| {
            parse(t).blocks[0]
                .inlines()
                .iter()
                .map(|i| std::mem::discriminant(&i.kind))
                .collect::<Vec<_>>()
        };
        let text = std::mem::discriminant(&InlineKind::Text("".into()));
        let raw = std::mem::discriminant(&InlineKind::Html("".into()));
        let line = std::mem::discriminant(&InlineKind::LineBreak);
        assert_eq!(kinds_of("a<br>b"), [text, line, text]);
        assert_eq!(kinds_of("a</br>b"), [text, raw, text], "a closing tag breaks nothing");
        assert_eq!(kinds_of("a<b/>b"), [text, raw, text], "nothing to enclose");
        let t = "<a href=\"u\" title=\"t\">x</a>\n";
        let d = parse(t);
        assert_eq!(
            d.blocks[0].inlines()[0].kind,
            InlineKind::Link {
                url: "u".into(),
                title: "t".into()
            }
        );
    }

    #[test]
    fn a_closing_tag_of_another_kind_does_not_close() {
        let d = parse("<b>a</i> c\n");
        assert!(
            d.blocks[0].inlines().iter().all(|i| i.kind != InlineKind::Strong),
            "{d:?}"
        );
    }

    #[test]
    fn positions_stay_right_behind_many_escaped_pipes() {
        let t = "a|b\n-|-\n\\|\\|\\|\\|\\|\\|`c`|x\n";
        let d = parse(t);
        let mut code = None;
        walk_blocks(&d.blocks, &mut |b| {
            walk_inlines(b.inlines(), &mut |i| {
                if let InlineKind::Code(_) = i.kind {
                    code = Some(i.range.clone());
                }
            })
        });
        assert_eq!(slice(t, &code.unwrap()), "`c`");
    }

    #[test]
    fn a_position_inside_a_letter_moves_to_its_edge() {
        assert_eq!(floor_char("aéb", 2), 1);
        assert_eq!(floor_char("aéb", 3), 3);
        assert_eq!(floor_char("aéb", 0), 0);
        assert_eq!(ceil_char("aéb", 2), 3);
        assert_eq!(ceil_char("aéb", 1), 1);
        assert_eq!(ceil_char("aéb", 4), 4);
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
        let inner = |at: usize| slice(t, &block_path(&d.blocks, at).last().unwrap().range).to_string();
        assert_eq!(inner(4), "- b");
        assert_eq!(inner(t.len() - 1), "b");
    }

    #[test]
    fn every_diagram_language_is_known() {
        for (info, kind) in [
            ("mermaid", DiagramKind::Mermaid),
            ("dot", DiagramKind::Graphviz),
            ("graphviz title", DiagramKind::Graphviz),
            ("plantuml", DiagramKind::PlantUml),
            ("puml", DiagramKind::PlantUml),
        ] {
            assert_eq!(diagram_kind(info), Some(kind), "{info}");
        }
        assert_eq!(diagram_kind("rust"), None);
        assert_eq!(diagram_kind(""), None);
    }

    #[test]
    fn the_delimiters_of_a_formula_must_stand_on_both_sides() {
        assert_eq!(math_delimiters("$a$"), (1, 1));
        assert_eq!(math_delimiters("$$a$$"), (2, 2));
        assert_eq!(math_delimiters("$`a`$"), (2, 2));
        assert_eq!(math_delimiters("\\(a\\)"), (2, 2));
        assert_eq!(math_delimiters("\\[a\\]"), (2, 2));
        assert_eq!(math_delimiters("$"), (0, 0), "one mark does not close itself");
        assert_eq!(math_delimiters("$a"), (0, 0));
        assert_eq!(math_delimiters("a$$"), (0, 0));
    }

    #[test]
    fn the_figure_at_a_position_includes_its_ends() {
        let t = "$$\na\n$$\n\ny $b$ z\n";
        let d = parse(t);
        let block_end = d.blocks[0].range.end;
        let inline = t.find("$b$").unwrap();
        assert_eq!(figure_at(&d, 0).unwrap().0, d.blocks[0].range);
        assert_eq!(figure_at(&d, block_end).unwrap().0, d.blocks[0].range);
        assert!(figure_at(&d, block_end + 1).is_none());
        assert!(figure_at(&d, inline - 1).is_none());
        assert_eq!(figure_at(&d, inline).unwrap().0, inline..inline + 3);
        assert_eq!(figure_at(&d, inline + 3).unwrap().0, inline..inline + 3);
        assert!(figure_at(&d, inline + 4).is_none());
    }

    #[test]
    fn a_note_over_lines_of_a_quote_or_list_ends_at_its_bracket_and_what_follows_stays_behind_it() {
        for t in ["> a^[b [c]\n> d] e f", "- a^[b [c]\n  d] e f"] {
            let d = parse(t);
            let mut seen = Vec::new();
            walk_blocks(&d.blocks, &mut |b| {
                if !matches!(b.kind, BlockKind::Paragraph) || b.inlines().len() < 2 {
                    return;
                }
                seen = b.inlines().iter().map(|i| i.range.clone()).collect();
                let note = b
                    .inlines()
                    .iter()
                    .find(|i| matches!(i.kind, InlineKind::FootnoteRef { .. }))
                    .unwrap();
                assert!(
                    slice(t, &note.range).starts_with("^[b [c]") && slice(t, &note.range).ends_with("d]"),
                    "{t}"
                );
            });
            assert!(seen.len() >= 3, "{t}: {seen:?}");
            assert!(seen.windows(2).all(|w| w[0].end <= w[1].start), "{t}: {seen:?}");
        }
    }

    #[test]
    fn the_text_of_a_note_far_into_a_paragraph_stops_before_its_bracket() {
        let t = "abcdefgh^[b] c";
        let d = parse(t);
        let note = d
            .blocks
            .iter()
            .find(|b| matches!(b.kind, BlockKind::FootnoteDefinition { .. }))
            .unwrap();
        assert_eq!(slice(t, &note.children()[0].inlines()[0].range), "b");
    }

    #[test]
    fn a_self_closing_tag_opens_nothing_a_later_one_could_close() {
        let d = parse("<b/>x</b> y\n");
        assert!(
            d.blocks[0].inlines().iter().all(|i| i.kind != InlineKind::Strong),
            "{d:?}"
        );
    }
}
