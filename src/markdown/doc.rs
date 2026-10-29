use std::ops::Range;

use crate::i18n::n;
use comrak::nodes::{AlertType, ListDelimType, ListType, NodeValue, Sourcepos, TableAlignment};
use comrak::{Arena, Options, parse_document};

use super::text::LineStarts;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

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

/// Strings and lists are boxed slices without spare capacity, since a large document has hundreds of thousands of
/// blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub range: Range<usize>,
    pub children: Box<[Block]>,
    pub inlines: Box<[Inline]>,
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
    let root = parse_document(&arena, text, &opts);
    let lines = LineStarts::new(text);
    let cx = Cx {
        text,
        lines: &lines,
        depth: std::cell::Cell::new(0),
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
    let notes: Vec<(usize, Option<Block>)> = inline_notes
        .into_iter()
        .map(|(at, name, node)| (at, inline_note(text, &blocks, &name, node)))
        .collect();
    for (at, note) in notes {
        if let Some(note) = note {
            blocks.insert(at.min(blocks.len()), note);
        }
    }
    blocks.shrink_to_fit();
    blocks
}

/// comrak's name for the footnote an inline `^[note]` stands for.
const INLINE_NOTE: &str = "__inline_";

/// The footnote behind an inline `^[note]`. comrak gives its content positions relative to the note text, so it is
/// converted from that text and moved to where it stands; the definition covers the reference.
fn inline_note<'a>(text: &str, blocks: &[Block], name: &str, node: comrak::nodes::Node<'a>) -> Option<Block> {
    let mut reference = None;
    walk_blocks(blocks, &mut |b| {
        walk_inlines(&b.inlines, &mut |i| {
            if let InlineKind::FootnoteRef { name: n, ix } = &i.kind
                && &**n == name
            {
                reference = Some((i.range.clone(), *ix));
            }
        })
    });
    let (at, number) = reference?;
    let content = (at.start + 2).min(at.end)..at.end.saturating_sub(1).max((at.start + 2).min(at.end));
    let note = &text[content.clone()];
    // Over several lines comrak counts positions from quote and list prefixes it removed, so such a note is kept
    // as plain text.
    if note.contains('\n') {
        let paragraph = Block {
            kind: BlockKind::Paragraph,
            range: content.clone(),
            children: Box::default(),
            inlines: Box::new([Inline {
                kind: InlineKind::Text(note.into()),
                range: content.clone(),
                children: Box::default(),
            }]),
        };
        return Some(Block {
            kind: BlockKind::FootnoteDefinition {
                name: name.into(),
                label: at.start..at.start,
                number: Some(number),
            },
            range: at,
            children: Box::new([paragraph]),
            inlines: Box::default(),
        });
    }
    let lines = LineStarts::new(note);
    let cx = Cx {
        text: note,
        lines: &lines,
        depth: std::cell::Cell::new(0),
    };
    let children: Vec<Block> = node
        .children()
        .filter_map(|c| cx.block(c))
        .map(|mut b| {
            super::incremental::shift_block(&mut b, content.start as isize);
            b
        })
        .collect();
    Some(Block {
        kind: BlockKind::FootnoteDefinition {
            name: name.into(),
            label: at.start..at.start,
            number: Some(number),
        },
        range: at,
        children: children.into(),
        inlines: Box::default(),
    })
}

/// A callout type as the alert kind closest to it, and whether it is one of GitHub's.
pub fn callout_kind(name: &str) -> (AlertKind, bool) {
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

pub fn capitalized(name: &str) -> String {
    let lower = name.to_lowercase();
    let mut chars = lower.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

pub fn node_text<'a>(node: comrak::nodes::Node<'a>) -> String {
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

struct Cx<'t> {
    text: &'t str,
    lines: &'t LineStarts,
    depth: std::cell::Cell<usize>,
}

struct Level<'c>(&'c std::cell::Cell<usize>);

impl Drop for Level<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

impl Cx<'_> {
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
        let start = self.lines.start(sp.start.line, sp.start.column).min(self.text.len());
        let end = self.lines.end(sp.end.line, sp.end.column).min(self.text.len());
        let start = floor_char(self.text, start);
        let end = ceil_char(self.text, end.max(start));
        start..end
    }

    fn block<'a>(&self, node: comrak::nodes::Node<'a>) -> Option<Block> {
        let data = node.data();
        let range = self.range(data.sourcepos);
        let Some(_level) = self.enter() else {
            return Some(Block {
                kind: BlockKind::Paragraph,
                inlines: Box::new([self.raw_text(range.clone())]),
                range,
                children: Box::default(),
            });
        };
        let kind = match &data.value {
            NodeValue::Paragraph => {
                let inlines = self.inlines(node);
                if let [only] = inlines.as_slice()
                    && let InlineKind::Math { display: true, literal } = &only.kind
                {
                    return Some(Block {
                        kind: BlockKind::MathBlock {
                            literal: literal.clone(),
                        },
                        range,
                        children: Box::default(),
                        inlines: Box::default(),
                    });
                }
                return Some(Block {
                    kind: BlockKind::Paragraph,
                    range,
                    children: Box::default(),
                    inlines: inlines.into(),
                });
            }
            NodeValue::Heading(h) => {
                let inlines = self.inlines(node);
                let id = data.attrs.as_ref().and_then(|a| a.id.as_deref()).map(Into::into);
                let attrs = data.attrs.as_ref().and_then(|_| self.heading_attrs(&range));
                return Some(Block {
                    kind: BlockKind::Heading {
                        level: h.level,
                        setext: h.setext,
                        id,
                        attrs,
                    },
                    range,
                    children: Box::default(),
                    inlines: inlines.into(),
                });
            }
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => BlockKind::BlockQuote,
            NodeValue::Alert(a) => BlockKind::Alert {
                kind: match a.alert_type {
                    AlertType::Note => AlertKind::Note,
                    AlertType::Tip => AlertKind::Tip,
                    AlertType::Important => AlertKind::Important,
                    AlertType::Warning => AlertKind::Warning,
                    AlertType::Caution => AlertKind::Caution,
                },
                title: a.title.as_deref().map(Into::into),
                marker: self.alert_marker(&range),
                fold: None,
            },
            NodeValue::Callout(c) => {
                let (kind, github) = callout_kind(&c.callout_type);
                let custom = node
                    .children()
                    .find(|n| matches!(n.data().value, NodeValue::CalloutTitle))
                    .map(node_text)
                    .filter(|t| !t.trim().is_empty());
                let title = custom.or_else(|| (!github).then(|| capitalized(&c.callout_type)));
                let marker = self.alert_marker(&range);
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
            NodeValue::CalloutTitle => return None,
            NodeValue::DescriptionList => BlockKind::DefinitionList,
            NodeValue::DescriptionItem(_) => BlockKind::DefinitionItem,
            NodeValue::DescriptionTerm => BlockKind::DefinitionTerm,
            NodeValue::DescriptionDetails => BlockKind::DefinitionDetails,
            NodeValue::MultilineComment(_) => BlockKind::Comment,
            NodeValue::List(l) => BlockKind::List {
                kind: match l.list_type {
                    ListType::Bullet => ListKind::Bullet(self.text[range.start..].chars().next().unwrap_or('-')),
                    ListType::Ordered => ListKind::Ordered {
                        start: l.start,
                        delim: match l.delimiter {
                            ListDelimType::Period => '.',
                            ListDelimType::Paren => ')',
                        },
                    },
                },
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
            NodeValue::FrontMatter(_) => {
                let slice = &self.text[range.clone()];
                let start = range.start + slice.find('\n').map_or(slice.len(), |i| i + 1);
                let end = range.start + slice.trim_end().rfind('\n').map_or(0, |i| i + 1);
                BlockKind::FrontMatter {
                    content: start..end.max(start),
                }
            }
            NodeValue::Table(t) => BlockKind::Table {
                alignments: t
                    .alignments
                    .iter()
                    .map(|a| match a {
                        TableAlignment::None => Align::None,
                        TableAlignment::Left => Align::Left,
                        TableAlignment::Center => Align::Center,
                        TableAlignment::Right => Align::Right,
                    })
                    .collect(),
            },
            NodeValue::TableRow(header) => BlockKind::TableRow { header: *header },
            NodeValue::TableCell => {
                let mut inlines = self.inlines(node);
                let escapes: Vec<usize> = self.text[range.clone()]
                    .match_indices("\\|")
                    .map(|(i, _)| range.start + i)
                    .collect();
                if !escapes.is_empty() {
                    shift_escaped_pipes(&mut inlines, &escapes);
                }
                return Some(Block {
                    kind: BlockKind::TableCell,
                    range,
                    children: Box::default(),
                    inlines: inlines.into(),
                });
            }
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
        Some(Block {
            kind,
            range,
            children: children.into(),
            inlines: Box::default(),
        })
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
        let last_line = self.text[last_line_start..range.start + trimmed.len()].trim();
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
            _ => InlineKind::Text(self.text[range.clone()].into()),
        };
        drop(data);
        let children = match kind {
            InlineKind::Emph
            | InlineKind::Strong
            | InlineKind::Strikethrough
            | InlineKind::Highlight
            | InlineKind::Superscript
            | InlineKind::Subscript
            | InlineKind::Spoiler
            | InlineKind::WikiLink { .. }
            | InlineKind::Link { .. }
            | InlineKind::Image { .. } => self.inlines(node).into(),
            _ => Box::default(),
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
    let pixels = |key: &str| {
        attr(key)
            .and_then(|v| v.trim().trim_end_matches("px").parse::<f64>().ok())
            .filter(|n| *n > 0.0 && *n < 100_000.0)
            .map(|n| n as u32)
    };
    let kind = match name.as_str() {
        "br" if !closing => return Some(HtmlTag::Alone(InlineKind::LineBreak)),
        "img" if !closing => {
            let url = attr("src").filter(|u| super::markup::safe_url(u))?;
            return Some(HtmlTag::Alone(InlineKind::HtmlImage {
                url: url.into(),
                alt: attr("alt").unwrap_or("").into(),
                width: pixels("width"),
                height: pixels("height"),
            }));
        }
        _ if self_closing => return None,
        "u" | "ins" => InlineKind::Underline,
        "mark" => InlineKind::Highlight,
        "sup" => InlineKind::Superscript,
        "sub" => InlineKind::Subscript,
        "b" | "strong" => InlineKind::Strong,
        "i" | "em" | "cite" | "dfn" | "var" => InlineKind::Emph,
        "s" | "del" | "strike" => InlineKind::Strikethrough,
        "kbd" | "code" | "samp" | "tt" => InlineKind::Kbd,
        "span" | "small" | "big" | "abbr" | "font" | "q" | "time" => InlineKind::Span,
        "a" => InlineKind::Link {
            url: attr("href").filter(|u| super::markup::safe_url(u)).unwrap_or("").into(),
            title: attr("title").unwrap_or("").into(),
        },
        _ => return None,
    };
    Some(HtmlTag::Container { closing, kind })
}

/// Whether a closing tag ends the container an opening tag started: links match whatever their address.
fn same_container(open: &InlineKind, close: &InlineKind) -> bool {
    std::mem::discriminant(open) == std::mem::discriminant(close)
}

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
                open.push((out.len(), kind));
                out.push(inline);
                continue;
            }
            if let Some(pos) = open.iter().rposition(|(_, k)| same_container(k, &kind)) {
                let (idx, kind) = open.remove(pos);
                open.truncate(pos);
                let mut children: Vec<Inline> = out.drain(idx..).collect();
                let opener = children.remove(0);
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

/// comrak removes the backslash of `\|` inside table cells before parsing inlines, so inline positions
/// after each escape are one byte short.
fn shift_escaped_pipes(inlines: &mut [Inline], escapes: &[usize]) {
    let map = |o: usize| {
        let mut out = o;
        for (n, &e) in escapes.iter().enumerate() {
            if e - n < o {
                out = o + n + 1;
            }
        }
        out
    };
    fn apply(inlines: &mut [Inline], map: &impl Fn(usize) -> usize) {
        for i in inlines {
            i.range = map(i.range.start)..map(i.range.end);
            apply(&mut i.children, map);
        }
    }
    apply(inlines, &map);
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
    pub fn is_container(&self) -> bool {
        matches!(
            self.kind,
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

    /// Source range between the opening and closing delimiters of a container inline.
    pub fn content_range(&self) -> Range<usize> {
        match (self.children.first(), self.children.last()) {
            (Some(first), Some(last)) => first.range.start..last.range.end,
            _ => {
                let mid = self.range.start + (self.range.end - self.range.start) / 2;
                mid..mid
            }
        }
    }
}

impl Block {
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
        Some(self.inlines.first()?.range.start..self.inlines.last()?.range.end)
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
                level = &b.children;
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
        walk_blocks(&b.children, f);
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

pub fn is_diagram(info: &str) -> bool {
    diagram_kind(info).is_some()
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
        walk_inlines(&b.inlines, &mut |i| {
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
                (e.name == "img" && e.attr("src").is_some_and(|u| u.starts_with("https://"))) || in_html(&e.children)
            }
            super::markup::Node::Text { .. } => false,
        })
    }
    let mut found = false;
    walk_blocks(&doc.blocks, &mut |b| {
        if let BlockKind::HtmlBlock { literal } = &b.kind {
            found |= literal.contains("https://") && in_html(&super::markup::parse(literal));
        }
        walk_inlines(&b.inlines, &mut |i| {
            found |= matches!(
                &i.kind,
                InlineKind::Image { url, .. } | InlineKind::HtmlImage { url, .. } if url.starts_with("https://")
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
        assert_eq!(kinds(&doc.blocks[0].inlines), ["Text(\"Titel\")"]);
        let p = &doc.blocks[1].inlines;
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
        let parts: Vec<&BlockKind> = list.children[0].children.iter().map(|b| &b.kind).collect();
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
        let inner = &note.children[0].inlines;
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
    fn inline_ranges_are_exact_with_multibyte_text() {
        let t = "Text **fett ü** und `co` <u>un</u> 😀 [li](http://x)\n";
        let d = parse(t);
        let p = &d.blocks[0];
        let strong = p.inlines.iter().find(|i| i.kind == InlineKind::Strong).unwrap();
        assert_eq!(slice(t, &strong.range), "**fett ü**");
        assert_eq!(slice(t, &strong.content_range()), "fett ü");
        let u = p.inlines.iter().find(|i| i.kind == InlineKind::Underline).unwrap();
        assert_eq!(slice(t, &u.range), "<u>un</u>");
        assert_eq!(slice(t, &u.content_range()), "un");
        let link = p
            .inlines
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
        let item = &d.blocks[1].children[0];
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
        match &d.blocks[2].children[0].kind {
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
        let para = &d.blocks[0].children[0];
        let mark = para.inlines.iter().find(|i| i.kind == InlineKind::Highlight).unwrap();
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
            level = &b.children;
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
                .inlines
                .iter()
                .any(|i| matches!(i.kind, InlineKind::Html(_)))
        );
    }

    #[test]
    fn escaped_text_keeps_literal() {
        let t = "a \\* b &amp; c\n";
        let d = parse(t);
        let text: String = d.blocks[0]
            .inlines
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
}
