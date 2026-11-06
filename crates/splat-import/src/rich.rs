//! What every importer reads a foreign format into, and the one writer that makes Markdown of it, so that all of them
//! keep their text literal in Splat's dialect in the same way.

mod escape;
mod inline;
mod tidy;
mod write;

#[cfg(test)]
mod tests;

use std::sync::Arc;

pub(crate) use splat_markdown::doc::Align;

use crate::ImageSource;

/// The widest a table cell can be, as `splat_markdown::markup` reads `colspan`.
pub(crate) const MOST_COLUMNS: u32 = 64;

/// A look a piece of text can have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    Bold,
    Italic,
    Strike,
    Underline,
    Highlight,
    Superscript,
    Subscript,
}

impl Mark {
    /// All marks, in the order they nest when they start and end together, as Splat's own commands write them.
    pub(crate) const ALL: [Mark; 7] = [
        Mark::Bold,
        Mark::Italic,
        Mark::Strike,
        Mark::Underline,
        Mark::Highlight,
        Mark::Superscript,
        Mark::Subscript,
    ];

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// A set of marks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Marks(u8);

impl Marks {
    /// Whether `mark` is in the set.
    pub(crate) fn has(self, mark: Mark) -> bool {
        self.0 & mark.bit() != 0
    }

    /// The set with `mark` in it or, if `on` is false, without it.
    #[must_use]
    pub(crate) fn with(self, mark: Mark, on: bool) -> Marks {
        if on {
            Marks(self.0 | mark.bit())
        } else {
            Marks(self.0 & !mark.bit())
        }
    }

    fn common(self, other: Marks) -> Marks {
        Marks(self.0 & other.0)
    }
}

impl From<&[Mark]> for Marks {
    fn from(marks: &[Mark]) -> Marks {
        marks.iter().fold(Marks::default(), |set, mark| set.with(*mark, true))
    }
}

/// Where a picture comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// An address, or a destination in the Markdown once it has been placed.
    Address(String),
    /// A picture carried along in the foreign format.
    Data { mime: &'static str, bytes: Vec<u8> },
}

/// The most a picture carried in a foreign format may hold.
pub(crate) const MOST_PICTURE_BYTES: usize = 32 << 20;

/// Whether `bytes` are what a picture of the kind `mime` starts with, and not more than the limit.
pub(crate) fn is_picture(mime: &str, bytes: &[u8]) -> bool {
    let magic = match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        _ => false,
    };
    magic && bytes.len() <= MOST_PICTURE_BYTES
}

/// A picture with what stands for it where it is left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Image {
    pub(crate) source: Source,
    pub(crate) alt: String,
}

/// The content of a span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Piece {
    Text(String),
    Code(String),
    Image(Image),
    /// A hard line break.
    Break,
    /// A footnote reference, by the name of its definition.
    Note(String),
    /// A formula in LaTeX, within the line.
    Math(String),
}

/// A piece of a line with its look and the address it leads to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) piece: Piece,
    pub(crate) marks: Marks,
    /// Shared, as all the runs of one link are.
    pub(crate) link: Option<Arc<str>>,
}

impl Span {
    /// Text without a look.
    pub(crate) fn text(text: impl Into<String>) -> Span {
        Span::of(Piece::Text(text.into()))
    }

    /// `piece` without a look.
    pub(crate) fn of(piece: Piece) -> Span {
        Span {
            piece,
            marks: Marks::default(),
            link: None,
        }
    }
}

#[cfg(test)]
impl Span {
    /// The span with these marks.
    #[must_use]
    pub(crate) fn marked(self, marks: Marks) -> Span {
        Span { marks, ..self }
    }

    /// The span as a link to `url`.
    #[must_use]
    pub(crate) fn linked(self, url: &str) -> Span {
        Span {
            link: Some(url.into()),
            ..self
        }
    }
}

/// What kind of list a list is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListKind {
    Bullet,
    Numbered { start: u64 },
}

/// A list item; `task` says whether its checkbox is checked, if it has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub(crate) task: Option<bool>,
    pub(crate) blocks: Vec<Block>,
}

/// A list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct List {
    pub(crate) kind: ListKind,
    pub(crate) items: Vec<Item>,
}

/// A table cell, with the columns it spans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cell {
    pub(crate) spans: Vec<Span>,
    pub(crate) columns: u32,
}

impl Cell {
    /// A cell of one column.
    pub(crate) fn new(spans: Vec<Span>) -> Cell {
        Cell { spans, columns: 1 }
    }
}

/// A table; its first row is the header. A cell of several columns makes the writer use an HTML table, which is what
/// Splat renders for what GitHub's tables cannot express.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Table {
    pub(crate) alignments: Vec<Align>,
    pub(crate) rows: Vec<Vec<Cell>>,
}

/// A block of the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Block {
    Paragraph(Vec<Span>),
    Heading {
        level: u8,
        spans: Vec<Span>,
        /// The name links to it use, written `{#name}`.
        anchor: Option<String>,
    },
    List(List),
    Quote(Vec<Block>),
    Code {
        language: String,
        text: String,
    },
    Rule,
    Table(Table),
    /// A formula in LaTeX, on lines of its own.
    Math(String),
    /// A footnote definition; the writer puts them at the end.
    Note {
        name: String,
        blocks: Vec<Block>,
    },
}

/// A foreign document in terms of what Markdown can say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rich {
    pub(crate) blocks: Vec<Block>,
}

impl Rich {
    /// The document as Markdown, ending with a line break unless it is empty.
    pub fn into_markdown(mut self) -> String {
        self.settle_images();
        self.tidied().tidy_markdown()
    }

    /// The document as Markdown, which `tidied` has to have made it ready for.
    pub(crate) fn tidy_markdown(&self) -> String {
        write::markdown(self)
    }

    /// Asks `place` where each picture goes; one it leaves out becomes its alternative text.
    pub(crate) fn place_images(&mut self, place: &mut dyn FnMut(&ImageSource) -> Option<String>) {
        visit_spans(&mut self.blocks, &mut |span| {
            if let Piece::Image(image) = &mut span.piece {
                let asked = match &image.source {
                    Source::Address(address) => place(&ImageSource::Address(address)),
                    Source::Data { mime, bytes } => place(&ImageSource::Data { mime, bytes }),
                };
                match asked {
                    Some(destination) => image.source = Source::Address(destination),
                    None => span.piece = Piece::Text(tidy::one_line(&image.alt)),
                }
            }
        });
    }

    /// Pictures Markdown cannot point to become their alternative text.
    pub(crate) fn settle_images(&mut self) {
        visit_spans(&mut self.blocks, &mut |span| {
            if let Piece::Image(image) = &mut span.piece {
                let usable = match &image.source {
                    Source::Address(address) => !address.trim().is_empty() && splat_markdown::url::is_harmless(address),
                    Source::Data { .. } => false,
                };
                if !usable {
                    span.piece = Piece::Text(tidy::one_line(&image.alt));
                }
            }
        });
    }
}

/// Calls `visit` on every span of `blocks` and what they hold.
pub(crate) fn visit_spans(blocks: &mut [Block], visit: &mut dyn FnMut(&mut Span)) {
    for block in blocks {
        match block {
            Block::Paragraph(spans) | Block::Heading { spans, .. } => spans.iter_mut().for_each(&mut *visit),
            Block::List(list) => {
                for item in &mut list.items {
                    visit_spans(&mut item.blocks, visit);
                }
            }
            Block::Quote(blocks) | Block::Note { blocks, .. } => visit_spans(blocks, visit),
            Block::Table(table) => {
                for cell in table.rows.iter_mut().flatten() {
                    cell.spans.iter_mut().for_each(&mut *visit);
                }
            }
            Block::Code { .. } | Block::Rule | Block::Math(_) => {}
        }
    }
}

/// A list paragraph as flat formats hold it, with its level and the list it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FlatItem {
    pub(crate) level: usize,
    /// Items with another number at the same level start a new list.
    pub(crate) list: u64,
    pub(crate) kind: ListKind,
    pub(crate) item: Item,
}

/// A list being built, with the level and number of the items that continue it.
struct OpenList {
    level: usize,
    id: u64,
    list: List,
}

/// The lists that flat items make, nested by their levels.
pub(crate) fn nest_list(items: Vec<FlatItem>) -> Vec<Block> {
    let mut done = Vec::new();
    let mut open: Vec<OpenList> = Vec::new();
    for flat in items {
        while open.last().is_some_and(|top| top.level > flat.level) {
            close_list(&mut open, &mut done);
        }
        if open
            .last()
            .is_some_and(|top| top.level == flat.level && (top.id != flat.list || !same_kind(top.list.kind, flat.kind)))
        {
            close_list(&mut open, &mut done);
        }
        if open.last().is_none_or(|top| top.level < flat.level) {
            open.push(OpenList {
                level: flat.level,
                id: flat.list,
                list: List {
                    kind: flat.kind,
                    items: Vec::new(),
                },
            });
        }
        if let Some(top) = open.last_mut() {
            top.list.items.push(flat.item);
        }
    }
    while !open.is_empty() {
        close_list(&mut open, &mut done);
    }
    done
}

fn same_kind(a: ListKind, b: ListKind) -> bool {
    matches!(
        (a, b),
        (ListKind::Bullet, ListKind::Bullet) | (ListKind::Numbered { .. }, ListKind::Numbered { .. })
    )
}

/// Ends the innermost list: inside the last item of the one around it or, without one, as a block of its own.
fn close_list(open: &mut Vec<OpenList>, done: &mut Vec<Block>) {
    let Some(ended) = open.pop() else { return };
    let inner = Block::List(ended.list);
    match open.last_mut().and_then(|outer| outer.list.items.last_mut()) {
        Some(item) => item.blocks.push(inner),
        None => done.push(inner),
    }
}

/// Levels for the items whose file gives only their left margins in points: one for each margin, from the left.
pub(crate) fn levels_by_indent(items: &mut [FlatItem], indents: &[Option<f32>]) {
    let mut margins: Vec<f32> = indents.iter().flatten().copied().collect();
    margins.sort_by(f32::total_cmp);
    margins.dedup_by(|a, b| (*a - *b).abs() < 1.0);
    for (item, indent) in items.iter_mut().zip(indents) {
        if let Some(indent) = indent {
            item.level = margins
                .iter()
                .take_while(|margin| **margin < indent - 1.0)
                .count()
                .min(8);
        }
    }
}

/// What kind of list a list's marker typed as text makes: a number or letter with `.` or `)` around it counts, all else is a bullet.
pub(crate) fn marker_kind(marker: &str) -> ListKind {
    let body = marker.trim_start_matches('(').trim_end_matches(['.', ')']);
    let last = body.rsplit('.').next().unwrap_or(body);
    if let Ok(start) = last.parse::<u64>() {
        return ListKind::Numbered { start };
    }
    let delimited = body.len() < marker.trim_start_matches('(').len();
    if !delimited || last.is_empty() {
        return ListKind::Bullet;
    }
    let lower = last.to_ascii_lowercase();
    // A lone `i` opens a list of Roman numbers far more often than one that started at the ninth letter.
    if lower == "i" || (lower.len() > 1 && lower.bytes().all(|b| b"ivxlcdm".contains(&b))) {
        return ListKind::Numbered { start: 1 };
    }
    match lower.as_bytes() {
        [letter @ b'a'..=b'z'] => ListKind::Numbered {
            start: u64::from(*letter - b'a') + 1,
        },
        _ => ListKind::Bullet,
    }
}

/// The checkbox a list's marker typed as text shows, if it is one.
pub(crate) fn marker_task(marker: &str) -> Option<bool> {
    match marker {
        "☐" => Some(false),
        "☑" | "☒" => Some(true),
        _ => None,
    }
}

/// Whether a font sets every letter at the same width, by its name.
pub(crate) fn is_monospace_font(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    !name.contains("monotype")
        && ["menlo", "courier", "consolas", "monaco", "mono", "lucida console"]
            .iter()
            .any(|font| name.contains(font))
}
