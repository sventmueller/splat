//! Moving a tree along with an edit in front of it.

use std::ops::Range;

use super::{Block, BlockKind, Fold, Inline, InlineKind, Task};
use crate::text::{Byte, shifted};

/// What an edit replaced: the top-level block indexes before and after, and the shift of everything behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    /// The indexes of the replaced top-level blocks in the old tree.
    pub old: Range<usize>,
    /// The indexes of the top-level blocks that replace them in the new tree.
    pub new: Range<usize>,
    /// How many bytes longer the new text is than the old; negative when it is shorter.
    pub delta: isize,
    /// The re-parsed part of the new text; positions from its end on are shifted by `delta`.
    pub window: Range<Byte>,
    /// Where the change ends in the new text: blocks outside `new` from here on moved by `delta`, those before it not.
    pub behind: Byte,
}

fn moved(r: &mut Range<Byte>, delta: isize) {
    *r = shifted(r.start, delta)..shifted(r.end, delta);
}

/// Moves every source offset of a block by `delta`. The patterns name every field, so a new offset cannot be missed.
pub fn shift_block(b: &mut Block, delta: isize) {
    if delta == 0 {
        return;
    }
    let Block {
        kind,
        range,
        content: _,
    } = b;
    moved(range, delta);
    match kind {
        BlockKind::Alert {
            kind: _,
            title: _,
            marker,
            fold,
        } => {
            moved(marker, delta);
            if let Some(Fold { open: _, sign }) = fold {
                *sign = shifted(*sign, delta);
            }
        }
        BlockKind::Heading {
            level: _,
            setext: _,
            id: _,
            attrs,
        } => {
            if let Some(attrs) = attrs {
                moved(attrs, delta);
            }
        }
        BlockKind::Item { task, marker } => {
            moved(marker, delta);
            if let Some(Task { checked: _, symbol }) = task {
                *symbol = shifted(*symbol, delta);
            }
        }
        BlockKind::CodeBlock {
            info: _,
            literal: _,
            fenced: _,
            content,
        } => moved(content, delta),
        BlockKind::FootnoteDefinition {
            name: _,
            label,
            number: _,
        } => moved(label, delta),
        BlockKind::FrontMatter { content } => moved(content, delta),
        BlockKind::List { kind: _, tight: _ }
        | BlockKind::HtmlBlock { literal: _ }
        | BlockKind::Table { alignments: _ }
        | BlockKind::TableRow { header: _ }
        | BlockKind::MathBlock { literal: _ }
        | BlockKind::Paragraph
        | BlockKind::BlockQuote
        | BlockKind::ThematicBreak
        | BlockKind::TableCell
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
    let Inline { kind, range, children } = i;
    moved(range, delta);
    match kind {
        InlineKind::Text(_)
        | InlineKind::Code(_)
        | InlineKind::Html(_)
        | InlineKind::Emoji(_)
        | InlineKind::Link { url: _, title: _ }
        | InlineKind::Image { url: _, title: _ }
        | InlineKind::FootnoteRef { name: _, ix: _ }
        | InlineKind::Math { display: _, literal: _ }
        | InlineKind::WikiLink { target: _, embed: _ }
        | InlineKind::HtmlImage {
            url: _,
            alt: _,
            width: _,
            height: _,
        }
        | InlineKind::SoftBreak
        | InlineKind::LineBreak
        | InlineKind::Emph
        | InlineKind::Strong
        | InlineKind::Strikethrough
        | InlineKind::Underline
        | InlineKind::Highlight
        | InlineKind::Superscript
        | InlineKind::Subscript
        | InlineKind::Spoiler
        | InlineKind::Comment
        | InlineKind::Kbd
        | InlineKind::Span => {}
    }
    for c in children.iter_mut() {
        shift_inline(c, delta);
    }
}
