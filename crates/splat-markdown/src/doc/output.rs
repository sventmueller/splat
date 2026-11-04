//! What writes a document out block by block: the preview and the exports. Which kinds of block there are and which of them
//! their containers write is decided once here, and a new kind has to be answered by all of them.

use std::ops::Range;

use super::{AlertKind, Align, Block, BlockKind, Fold, ListKind};
use crate::text::Byte;

/// What a callout says about itself.
#[derive(Clone, Copy)]
pub struct Callout<'a> {
    pub kind: AlertKind,
    pub title: Option<&'a str>,
    /// Where its first line stands in the source.
    pub marker: &'a Range<Byte>,
    pub fold: Option<&'a Fold>,
}

/// What a code block says about itself.
#[derive(Clone, Copy)]
pub struct Code<'a> {
    pub info: &'a str,
    pub literal: &'a str,
    pub content: &'a Range<Byte>,
}

/// What a footnote's definition says about itself.
#[derive(Clone, Copy)]
pub struct Note<'a> {
    pub name: &'a str,
    pub label: &'a Range<Byte>,
    pub number: Option<u32>,
}

pub trait BlockOutput<'a> {
    /// Where a block stands, as its container decides it.
    type Place;

    fn paragraph(&mut self, block: &'a Block, place: Self::Place);
    fn heading(&mut self, block: &'a Block, level: u8, setext: bool, place: Self::Place);
    fn quote(&mut self, block: &'a Block, place: Self::Place);
    fn callout(&mut self, block: &'a Block, callout: Callout<'a>, place: Self::Place);
    fn definition_list(&mut self, block: &'a Block, place: Self::Place);
    fn list(&mut self, block: &'a Block, kind: &'a ListKind, tight: bool, place: Self::Place);
    fn code_block(&mut self, block: &'a Block, code: Code<'a>, place: Self::Place);
    fn html_block(&mut self, block: &'a Block, literal: &'a str, place: Self::Place);
    fn front_matter(&mut self, block: &'a Block, content: &'a Range<Byte>, place: Self::Place);
    fn rule(&mut self, block: &'a Block, place: Self::Place);
    fn math_block(&mut self, block: &'a Block, literal: &'a str, place: Self::Place);
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
