//! What writes a document out block by block: the preview and the exports. Which kinds of block there are and which of them
//! their containers write is decided once here, and a new kind has to be answered by all of them.

use std::ops::Range;

use super::{AlertKind, Align, Block, BlockKind, Fold, ListKind};
use crate::text::Byte;

/// What a callout says about itself.
#[derive(Clone, Copy)]
pub struct Callout<'a> {
    /// Which of the five alert kinds it shows as.
    pub kind: AlertKind,
    /// The title shown instead of the kind's label, if it has one.
    pub title: Option<&'a str>,
    /// Where its first line stands in the source.
    pub marker: &'a Range<Byte>,
    /// The `-` or `+` behind an Obsidian callout's marker, if it can fold.
    pub fold: Option<&'a Fold>,
}

/// What a code block says about itself.
#[derive(Clone, Copy)]
pub struct Code<'a> {
    /// The info string behind the opening fence, such as `rust`; empty for indented code.
    pub info: &'a str,
    /// The code without its fences and without the indentation of the block.
    pub literal: &'a str,
    /// Where the code stands in the source: between the fence lines, or all of an indented block.
    pub content: &'a Range<Byte>,
}

/// What a footnote's definition says about itself.
#[derive(Clone, Copy)]
pub struct Note<'a> {
    /// The label between `[^` and `]`, or comrak's `__inline_` name for an inline note.
    pub name: &'a str,
    /// Where `[^label]:` stands; empty for an inline note.
    pub label: &'a Range<Byte>,
    /// The number an inline `^[note]` shows, which has no name of its own.
    pub number: Option<u32>,
}

/// What writes a document out block by block, with one method for each kind of block that is written on its own;
/// [`write_block`] picks the method.
pub trait BlockOutput<'a> {
    /// Where a block stands, as its container decides it.
    type Place;

    /// Writes a paragraph of running text.
    fn paragraph(&mut self, block: &'a Block, place: Self::Place);
    /// Writes a heading of `level` 1 to 6; `setext` when it is underlined rather than opened with `#`.
    fn heading(&mut self, block: &'a Block, level: u8, setext: bool, place: Self::Place);
    /// Writes a block quote, `>`, with the blocks it holds.
    fn quote(&mut self, block: &'a Block, place: Self::Place);
    /// Writes a GitHub alert or an Obsidian callout with the blocks it holds.
    fn callout(&mut self, block: &'a Block, callout: Callout<'a>, place: Self::Place);
    /// Writes a list of terms and their details.
    fn definition_list(&mut self, block: &'a Block, place: Self::Place);
    /// Writes a bullet or ordered list with its items; `tight` when no blank line parts them.
    fn list(&mut self, block: &'a Block, kind: &'a ListKind, tight: bool, place: Self::Place);
    /// Writes an indented or fenced code block.
    fn code_block(&mut self, block: &'a Block, code: Code<'a>, place: Self::Place);
    /// Writes a block of raw HTML, `literal` as it stands in the source.
    fn html_block(&mut self, block: &'a Block, literal: &'a str, place: Self::Place);
    /// Writes the front matter, `content` being the lines between its `---` delimiters.
    fn front_matter(&mut self, block: &'a Block, content: &'a Range<Byte>, place: Self::Place);
    /// Writes a horizontal rule, `---`, `***` or `___`.
    fn rule(&mut self, block: &'a Block, place: Self::Place);
    /// Writes a display formula that fills a paragraph on its own, `literal` being its TeX.
    fn math_block(&mut self, block: &'a Block, literal: &'a str, place: Self::Place);
    /// Writes a table, `alignments` holding the alignment of each column.
    fn table(&mut self, block: &'a Block, alignments: &'a [Align], place: Self::Place);
    /// Can turn up inside the definition of another note, so whoever writes them later has to look for more.
    fn footnote_definition(&mut self, block: &'a Block, note: Note<'a>, place: Self::Place);
}

/// Sends `block` to what `output` does with its kind. Items, rows, cells and the parts of a definition list are written by
/// the block around them, and a comment shows nowhere.
pub fn write_block<'a, O: BlockOutput<'a>>(output: &mut O, block: &'a Block, place: O::Place) {
    match &block.kind {
        BlockKind::Paragraph => output.paragraph(block, place),
        BlockKind::Heading { level, setext, .. } => output.heading(block, *level, *setext, place),
        BlockKind::BlockQuote => output.quote(block, place),
        BlockKind::Alert {
            kind,
            title,
            marker,
            fold,
        } => output.callout(
            block,
            Callout {
                kind: *kind,
                title: title.as_deref(),
                marker,
                fold: fold.as_ref(),
            },
            place,
        ),
        BlockKind::DefinitionList => output.definition_list(block, place),
        BlockKind::List { kind, tight } => output.list(block, kind, *tight, place),
        BlockKind::CodeBlock {
            info, literal, content, ..
        } => output.code_block(block, Code { info, literal, content }, place),
        BlockKind::HtmlBlock { literal } => output.html_block(block, literal, place),
        BlockKind::FrontMatter { content } => output.front_matter(block, content, place),
        BlockKind::ThematicBreak => output.rule(block, place),
        BlockKind::MathBlock { literal } => output.math_block(block, literal, place),
        BlockKind::Table { alignments } => output.table(block, alignments, place),
        BlockKind::FootnoteDefinition { name, label, number } => output.footnote_definition(
            block,
            Note {
                name,
                label,
                number: *number,
            },
            place,
        ),
        BlockKind::Comment
        | BlockKind::Item { .. }
        | BlockKind::TableRow { .. }
        | BlockKind::TableCell
        | BlockKind::DefinitionItem
        | BlockKind::DefinitionTerm
        | BlockKind::DefinitionDetails => {}
    }
}
