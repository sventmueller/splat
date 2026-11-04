//! Moving a tree along with an edit in front of it.

use std::ops::Range;

use super::{Block, BlockKind, Inline};
use crate::text::{Byte, shifted};

/// What an edit replaced: the top-level block indexes before and after, and the shift of everything behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    pub old: Range<usize>,
    pub new: Range<usize>,
    pub delta: isize,
    /// The re-parsed part of the new text; positions from its end on are shifted by `delta`.
    pub window: Range<Byte>,
    /// Where the change ends in the new text: blocks outside `new` from here on moved by `delta`, those before it not.
    pub behind: Byte,
}

fn moved(r: &mut Range<Byte>, delta: isize) {
    *r = shifted(r.start, delta)..shifted(r.end, delta);
}

/// Moves every source offset of a block by `delta`.
pub fn shift_block(b: &mut Block, delta: isize) {
    if delta == 0 {
        return;
    }
    moved(&mut b.range, delta);
    match &mut b.kind {
        BlockKind::Alert { marker, fold, .. } => {
            moved(marker, delta);
            if let Some(f) = fold {
                f.sign = shifted(f.sign, delta);
            }
        }
        BlockKind::Heading { attrs: Some(attrs), .. } => moved(attrs, delta),
        BlockKind::Item { task, marker } => {
            moved(marker, delta);
            if let Some(t) = task {
                t.symbol = shifted(t.symbol, delta);
            }
        }
        BlockKind::CodeBlock { content, .. } => moved(content, delta),
        BlockKind::FootnoteDefinition { label, .. } => moved(label, delta),
        BlockKind::FrontMatter { content } => moved(content, delta),
        BlockKind::Paragraph
        | BlockKind::Heading { .. }
        | BlockKind::BlockQuote
        | BlockKind::List { .. }
        | BlockKind::HtmlBlock { .. }
        | BlockKind::ThematicBreak
        | BlockKind::Table { .. }
        | BlockKind::TableRow { .. }
        | BlockKind::TableCell
        | BlockKind::MathBlock { .. }
        | BlockKind::DefinitionList
        | BlockKind::DefinitionItem
        | BlockKind::DefinitionTerm
        | BlockKind::DefinitionDetails
        | BlockKind::Comment => {}
    }
    let (inlines, children) = b.content_mut();
    for i in inlines.iter_mut() {
        shift_inline(i, delta);
    }
    for c in children.iter_mut() {
        shift_block(c, delta);
    }
}

fn shift_inline(i: &mut Inline, delta: isize) {
    moved(&mut i.range, delta);
    for c in i.children.iter_mut() {
        shift_inline(c, delta);
    }
}
