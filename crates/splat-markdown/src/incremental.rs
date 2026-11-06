//! Re-parsing only the top-level blocks around an edit, as long as each rule in docs/partial-parse.md holds, which also says
//! why that gives what a full parse gives and what to look at when comrak changes.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use super::doc::facts::{
    BlocksByPosition, MOST_MEMORY, TextFacts, Unclosed, ends_footnote_label, expansions_fit, footnote_label,
    holds_loose_comment, link_definitions, may_define_links, reads_alone, reference_expansions, unclosed_after_change,
};
pub use super::doc::facts::{memory_estimate, stays_plain};
use super::doc::{Block, BlockKind, Doc, Inline, InlineKind, Splice, shift_block, walk_blocks, walk_inlines};
use super::highlight::{Style, highlight, highlight_blocks, rehighlight};
use super::reader::{UNCLOSED_MARKS, parse_part, parse_shared};
use super::text::{
    Byte, TextAt, TextPart, changed, growth, label_key, line_start, next_line_start, previous_line_start, shifted,
};

/// The parsed document and its source colors.
pub struct Parsed {
    /// The tree of the text.
    pub doc: Doc,
    /// The styled byte ranges of the source view.
    pub spans: Vec<(Range<Byte>, Style)>,
    /// Which top-level blocks changed, when only those were parsed again.
    pub splice: Option<Splice>,
}

const MOST_STAND_INS: usize = 1000;

impl Parsed {
    /// No blocks and no colors: commands treat the text as plain, and the preview shows nothing.
    pub fn plain(text: Arc<str>) -> Parsed {
        Parsed {
            doc: Doc::too_large(text),
            spans: Vec::new(),
            splice: None,
        }
    }

    /// The same for a text a parse failed on.
    pub fn failed(text: Arc<str>) -> Parsed {
        Parsed {
            doc: Doc::new(text, Vec::new()),
            spans: Vec::new(),
            splice: None,
        }
    }
}

/// `text` parsed in full, with its source colors; plain when it would take too much memory.
pub fn parse_and_color(text: &str) -> Parsed {
    parse_shared_and_color(text.into())
}

/// [`parse_and_color`] for a text that is shared already, which the tree keeps without a copy.
pub fn parse_shared_and_color(text: Arc<str>) -> Parsed {
    if stays_plain(&text) {
        return Parsed::plain(text);
    }
    let doc = parse_shared(text);
    let spans = highlight(&doc);
    // Here it may take the time; the next keystroke would feel it.
    doc.facts();
    Parsed {
        doc,
        spans,
        splice: None,
    }
}

/// Parses `new_text` only around its change from the text of `doc` where that is safe, otherwise all of it. `spans` belong
/// to `doc`.
pub fn reparse_and_color(doc: Doc, spans: &[(Range<Byte>, Style)], new_text: &str) -> Parsed {
    reparse_part_and_color(doc, spans, new_text).unwrap_or_else(|_| parse_and_color(new_text))
}

/// The partial parse of `reparse_and_color`; `doc` as it was when all of `new_text` has to be parsed.
pub fn reparse_part_and_color(
    mut doc: Doc,
    spans: &[(Range<Byte>, Style)],
    new_text: &str,
) -> Result<Parsed, Box<Doc>> {
    match reparse(&mut doc, new_text) {
        Some(splice) => Ok(Parsed {
            spans: with_definitions_in_window(rehighlight(spans, &doc, &splice), &doc, &splice),
            doc,
            splice: Some(splice),
        }),
        None => Err(Box::new(doc)),
    }
}

fn is_definition(b: &Block) -> bool {
    matches!(b.kind, BlockKind::FootnoteDefinition { .. })
}

/// A definition with a place of its own in the text, unlike the one of an inline note, which lies inside a block.
fn is_written_definition(b: &Block) -> bool {
    matches!(b.kind, BlockKind::FootnoteDefinition { number: None, .. })
}

/// `rehighlight` colors the blocks of the splice, and definitions between them lost theirs with the window.
fn with_definitions_in_window(
    mut spans: Vec<(Range<Byte>, Style)>,
    doc: &Doc,
    splice: &Splice,
) -> Vec<(Range<Byte>, Style)> {
    let blocks: Vec<Block> = doc
        .blocks
        .iter()
        .rev()
        .take_while(|b| is_definition(b))
        .filter(|b| is_written_definition(b) && splice.window.contains(&b.range.start))
        .cloned()
        .collect();
    if !blocks.is_empty() {
        spans.extend(highlight_blocks(doc.text(), &blocks));
        spans.sort_by_key(|(r, _)| r.start);
        // `rehighlight` may have colored the whole text.
        spans.dedup();
    }
    spans
}

fn looks_like_definitions(text: &str) -> usize {
    text.match_indices("]:")
        .filter(|&(i, _)| ends_footnote_label(text, i))
        .count()
}

/// Whether `text` holds a bracket that a link definition elsewhere can turn into a link.
fn holds_link_bracket(text: &str) -> bool {
    text.match_indices('[')
        .any(|(i, _)| footnote_label(&text[i..]).is_none())
}

/// Definitions for the labels in `text`; `None` when there are too many to be worth parsing.
fn stand_ins(text: &str) -> Option<String> {
    let mut labels = HashSet::new();
    let mut definitions = String::new();
    for (i, _) in text.match_indices("[^") {
        if let Some(label) = footnote_label(&text[i..])
            && labels.insert(label)
        {
            if labels.len() > MOST_STAND_INS {
                return None;
            }
            definitions.push_str("\n\n[^");
            definitions.push_str(label);
            definitions.push_str("]: x\n");
        }
    }
    Some(definitions)
}

/// The blocks of `text`, a part of the document, with its footnotes recognized and its definitions where they stand, and
/// its brackets read with the link definitions of the whole text. comrak takes `[^label]` for a reference only with a
/// definition in the same parse, and moves only the last definition of a label behind the text, which is the stand-in
/// here. A block left open swallows what stands behind it, which its neighbor shows, so without a neighbor to compare with
/// that takes a look of its own: the end of the document also ends blocks differently.
fn parse_window(window: &Window) -> Result<PartBlocks, WholeParse> {
    let &Window {
        text,
        at_start,
        at_end,
        links,
    } = window;
    let notes = stand_ins(text).ok_or(WholeParse::Footnotes)?;
    let links = if holds_link_bracket(text) { links } else { "" };
    if notes.is_empty() && links.is_empty() {
        return Ok(PartBlocks(parse_part(text, at_start)));
    }
    if at_end && !notes.is_empty() {
        return Err(WholeParse::Footnotes);
    }
    let mut part = parse_part(&format!("{text}{notes}\n\n{links}"), at_start);
    // Text that reads as anything but a definition there is no definition, whatever it looked like in its place.
    let definitions_start = text.end() + notes.len();
    if part
        .iter()
        .any(|b| b.range.start >= definitions_start && !is_definition(b))
    {
        return Err(WholeParse::LinkDefinitions);
    }
    part.retain(|b| b.range.start < text.end());
    if at_end {
        let alone = parse_part(text, at_start);
        if part.len() != alone.len() || !part.iter().zip(&alone).all(|(a, b)| same_shape(a, b)) {
            return Err(WholeParse::OpenBlock);
        }
    }
    Ok(PartBlocks(part))
}

/// The blocks of a part parsed on its own, whose positions count from the start of that part until they are placed.
struct PartBlocks(Vec<Block>);

impl PartBlocks {
    fn placed_at(mut self, start: Byte) -> Vec<Block> {
        for b in &mut self.0 {
            shift_block(b, growth(start, Byte::default()));
        }
        self.0
    }
}

/// A part of a text to parse on its own.
struct Window<'a> {
    text: &'a str,
    at_start: bool,
    at_end: bool,
    /// The link definitions of all of the text.
    links: &'a str,
}

/// Whether two blocks are the same but for what their text reads as.
fn same_shape(a: &Block, b: &Block) -> bool {
    a.kind == b.kind
        && a.range == b.range
        && a.children().len() == b.children().len()
        && a.children().iter().zip(b.children()).all(|(a, b)| same_shape(a, b))
}

fn references<'a>(blocks: impl IntoIterator<Item = &'a Block>) -> Vec<(&'a str, u32)> {
    let mut found = Vec::new();
    for block in blocks {
        walk_blocks(std::slice::from_ref(block), &mut |b| {
            walk_inlines(b.inlines(), &mut |i| {
                if let InlineKind::FootnoteRef { name, ix } = &i.kind {
                    found.push((&**name, *ix));
                }
            })
        });
    }
    found
}

/// Gives the references in `blocks` the numbers of `known`; false when one of them is not there.
fn number_references(blocks: &mut [Block], known: &HashMap<&str, u32>) -> bool {
    fn in_inlines(inlines: &mut [Inline], known: &HashMap<&str, u32>) -> bool {
        inlines.iter_mut().all(|i| {
            if let InlineKind::FootnoteRef { name, ix } = &mut i.kind {
                match known.get(&**name) {
                    Some(number) => *ix = *number,
                    None => return false,
                }
            }
            in_inlines(&mut i.children, known)
        })
    }
    blocks.iter_mut().all(|b| {
        let (inlines, children) = b.content_mut();
        in_inlines(inlines, known) && number_references(children, known)
    })
}

/// Whether a reference may hide in plain text, as comrak still counts it where the reader gave up its positions, or in the
/// title of a callout, which is plain text to the tree. `text` is what the positions of `blocks` point into, as a stand-in
/// takes the reference out of the title.
fn may_hide_reference<'a>(blocks: impl IntoIterator<Item = &'a Block>, part: TextPart) -> bool {
    let mut found = false;
    for block in blocks {
        walk_blocks(std::slice::from_ref(block), &mut |b| {
            if let BlockKind::Alert {
                title: Some(title),
                marker,
                ..
            } = &b.kind
            {
                let line = part
                    .inside(marker.start)
                    .zip(part.inside(marker.end))
                    .and_then(|(start, end)| part.text.get(start.get()..end.get()));
                found |= title.contains("[^")
                    || title.contains("^[")
                    || line.is_none_or(|l| l.contains("[^") || l.contains("^["));
            }
            walk_inlines(b.inlines(), &mut |i| {
                if let InlineKind::Text(text) = &i.kind {
                    found |= text.contains("[^") || text.contains("^[");
                }
            })
        });
    }
    found
}

fn holds_nested_definition(blocks: &[Block]) -> bool {
    let mut found = false;
    for b in blocks {
        walk_blocks(b.children(), &mut |c| found |= is_definition(c));
    }
    found
}

/// The indexes of the text blocks and written definitions in source order, as comrak moved the definitions behind the
/// text. `None` where a definition was moved out of a quote or list, or a second one of a label stayed behind.
fn source_order(blocks: &[Block], text_blocks: usize) -> Option<Vec<usize>> {
    if blocks[..text_blocks].iter().any(is_definition) {
        return None;
    }
    let mut labels = HashSet::new();
    let second_of_a_label = blocks[text_blocks..].iter().any(|b| match &b.kind {
        BlockKind::FootnoteDefinition { name, .. } => !labels.insert(label_key(name)),
        _ => false,
    });
    if second_of_a_label {
        return None;
    }
    let mut order: Vec<usize> = (0..text_blocks).collect();
    order.extend((text_blocks..blocks.len()).filter(|&i| is_written_definition(&blocks[i])));
    if order.len() > text_blocks {
        order.sort_by_key(|&i| blocks[i].range.start);
        if order
            .windows(2)
            .any(|w| blocks[w[0]].range.end > blocks[w[1]].range.start)
        {
            return None;
        }
    }
    Some(order)
}

/// Whether the definitions parsed in `part` are those written there before, with the change moving what lies behind it.
fn definitions_stay<'a>(
    written: impl Iterator<Item = &'a Block>,
    part: &[Block],
    old_change: &Range<Byte>,
    delta: isize,
) -> bool {
    let expected = written.map(|definition| {
        let mut definition = definition.clone();
        if definition.range.start >= old_change.end {
            shift_block(&mut definition, delta);
        }
        definition
    });
    expected.eq(part.iter().filter(|b| is_definition(b)).cloned())
}

/// Puts `part` in the place of the text blocks `old` and moves the blocks behind the change along with it.
fn replace_blocks(doc: &mut Doc, old: Range<usize>, part: Vec<Block>, old_change: &Range<Byte>, delta: isize) {
    let tail = doc.blocks.split_off(old.end);
    doc.blocks.truncate(old.start);
    doc.blocks.extend(part);
    let resume = doc.blocks.len();
    doc.blocks.extend(tail);
    for b in &mut doc.blocks[resume..] {
        if b.range.start >= old_change.end {
            shift_block(b, delta);
        }
    }
}

/// Why a change takes parsing all of the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WholeParse {
    /// So many unclosed inline notes or formulas that the reader switches them off, which only the whole text can tell.
    SwitchedOff,
    /// A `%%` that nothing closes yet, which another one anywhere would.
    LooseComment,
    /// Front matter that is open, or comes or goes.
    FrontMatter,
    /// Numbers or definitions of footnotes change, or cannot be told from a part of the text.
    Footnotes,
    /// A link's definition changes, or there are too many to read along.
    LinkDefinitions,
    /// A block stays open at the end of the text.
    OpenBlock,
    /// The part to parse reached half of the text, the change does not fit the blocks around it, or the new text takes too
    /// much memory to parse at all.
    TooMuch,
}

fn opens_front_matter(text: &str) -> bool {
    text.trim_start_matches('\u{FEFF}').starts_with("---")
}

/// Whether something anywhere in the text decides how the part around a change is read. A `^[` that the change adds is
/// found in the new part.
fn read_as_a_whole(facts: &TextFacts, new_unclosed: Unclosed) -> Option<WholeParse> {
    let limited = |unclosed: Unclosed| unclosed.latex > UNCLOSED_MARKS || unclosed.code > UNCLOSED_MARKS;
    if facts.marks.inline_notes > UNCLOSED_MARKS || limited(facts.unclosed_formulas) || limited(new_unclosed) {
        return Some(WholeParse::SwitchedOff);
    }
    facts.loose_comment.then_some(WholeParse::LooseComment)
}

/// What stays the same for every part of the text tried around one change.
struct Change<'a> {
    blocks: &'a [Block],
    old_text: &'a str,
    new_text: &'a str,
    /// The text blocks and written definitions in source order.
    order: &'a [usize],
    text_blocks: usize,
    old_change: Range<Byte>,
    delta: isize,
    /// A link's definition may stand anywhere in the old or the new text.
    may_define_links: bool,
    links: &'a str,
    reference_expansions: usize,
}

/// The blocks that take the place of `old` in the tree.
struct Replacement {
    old: Range<usize>,
    part: Vec<Block>,
    window: Range<Byte>,
    old_window: Range<Byte>,
    reference_expansions: usize,
}

/// Where the text of a block starts that begins at `at`, with the lines right above it that belong to no block, which go no
/// further up than `limit`: definitions of links, read out of the paragraph they head, which a line below can undo.
fn block_start_with_definitions(text: &str, at: Byte, limit: Byte) -> Byte {
    let mut start = line_start(text, at);
    while let Some(above) = previous_line_start(text, start) {
        if above < limit || text.at(above..start).trim().is_empty() {
            break;
        }
        start = above;
    }
    start
}

/// Where the blocks from `before` up to `after` of the order lie in both texts.
struct Part<'a> {
    before: Option<usize>,
    after: Option<usize>,
    start: Byte,
    old_end: Byte,
    new_end: Byte,
    old: &'a str,
    new: &'a str,
    /// The blocks of the old text it holds, as indexes into them.
    in_window: &'a [usize],
}

impl Part<'_> {
    fn old_stretch(&self) -> TextPart<'_> {
        TextPart {
            text: self.old,
            start: self.start,
        }
    }
}

impl<'c> Change<'c> {
    fn part_between(&self, before: Option<usize>, after: Option<usize>) -> Result<Part<'c>, WholeParse> {
        let (blocks, order) = (self.blocks, self.order);
        let n = order.len();
        let start = before.map_or(Byte::default(), |b| {
            let block_above = b
                .checked_sub(1)
                .map_or(Byte::default(), |above| blocks[order[above]].range.end);
            block_start_with_definitions(self.old_text, blocks[order[b]].range.start, block_above)
        });
        let old_end = match after {
            Some(a) if a + 1 < n => line_start(self.old_text, blocks[order[a + 1]].range.start),
            _ => self.old_text.end(),
        };
        let new_end = shifted(old_end, self.delta);
        if new_end < start || new_end > self.new_text.end() || !self.new_text.boundary(new_end) {
            return Err(WholeParse::TooMuch);
        }
        // Past half the document, parsing all of it is no slower.
        if new_end - start > self.new_text.len() / 2 && self.new_text.len() > 4096 {
            return Err(WholeParse::TooMuch);
        }
        Ok(Part {
            before,
            after,
            start,
            old_end,
            new_end,
            old: self.old_text.at(start..old_end),
            new: self.new_text.at(start..new_end),
            in_window: &order[before.unwrap_or(0)..after.map_or(n, |a| a + 1)],
        })
    }

    fn old_blocks<'p>(&self, part: &'p Part<'c>) -> impl Iterator<Item = &'c Block> + use<'c, 'p> {
        let blocks = self.blocks;
        part.in_window.iter().map(move |&i| &blocks[i])
    }

    /// Rule 2 of docs/partial-parse.md: what the footnotes of the part are can be told from the tree.
    fn footnotes_can_be_told(&self, part: &Part) -> Result<(), WholeParse> {
        // The names comrak gives inline notes run through the whole document.
        if part.new.contains("^[") {
            return Err(WholeParse::Footnotes);
        }
        // A definition without a reference is not in the tree, but the references in it gave other notes their numbers.
        let written = part.in_window.iter().filter(|&&i| i >= self.text_blocks).count();
        if looks_like_definitions(part.old) != written {
            return Err(WholeParse::Footnotes);
        }
        if may_hide_reference(self.old_blocks(part), part.old_stretch()) {
            return Err(WholeParse::Footnotes);
        }
        Ok(())
    }

    /// Rule 3: a reference comrak would not know in the whole document, or that the window did not hold, changes numbers.
    fn references_keep_their_numbers(
        part: &Part,
        read: &mut PartBlocks,
        known: &HashMap<&str, u32>,
    ) -> Result<(), WholeParse> {
        if !number_references(&mut read.0, known) || may_hide_reference(&read.0, TextPart::whole(part.new)) {
            return Err(WholeParse::Footnotes);
        }
        Ok(())
    }

    /// Rule 4: a link's definition anywhere can turn brackets anywhere into a link.
    fn link_definitions_stay(&self, part: &Part, read: &PartBlocks) -> Result<(), WholeParse> {
        let brackets = holds_link_bracket(part.old) || holds_link_bracket(part.new);
        let near_definitions = self.may_define_links || may_define_links(part.new);
        let old_alone = part
            .in_window
            .iter()
            .all(|&i| reads_alone(std::slice::from_ref(&self.blocks[i])));
        if near_definitions && brackets && !(old_alone && reads_alone(&read.0)) {
            return Err(WholeParse::LinkDefinitions);
        }
        let new_blocks = BlocksByPosition::new(&read.0);
        let old_blocks = BlocksByPosition::new(self.old_blocks(part));
        let old_definitions = link_definitions(&old_blocks, part.old_stretch()).ok_or(WholeParse::LinkDefinitions)?;
        let new_definitions =
            link_definitions(&new_blocks, TextPart::whole(part.new)).ok_or(WholeParse::LinkDefinitions)?;
        if old_definitions != new_definitions {
            return Err(WholeParse::LinkDefinitions);
        }
        // Rule 5: a `%%` that nothing closes yet.
        if holds_loose_comment(&new_blocks, part.new) {
            return Err(WholeParse::LooseComment);
        }
        Ok(())
    }

    /// Rule 6: what the links of the whole text expand to after the change, as long as comrak's budget allows it.
    fn expansions_after(&self, part: &Part, read: &PartBlocks) -> Result<usize, WholeParse> {
        let gone: usize = part
            .in_window
            .iter()
            .map(|&i| reference_expansions(std::slice::from_ref(&self.blocks[i]), part.old_stretch()))
            .sum();
        let come = reference_expansions(&read.0, TextPart::whole(part.new));
        let after = self.reference_expansions - gone + come;
        // A part read alone may expand to less than all of the text it belongs to.
        if !expansions_fit(after, self.links, self.new_text.len()) || !expansions_fit(come, self.links, part.new.len())
        {
            return Err(WholeParse::LinkDefinitions);
        }
        Ok(after)
    }

    /// Rule 1: the blocks on either side of the change, parsed again, came out as they were, so it did not reach into them.
    fn neighbours_hold(&self, part: &Part, placed: &[Block]) -> bool {
        let (blocks, order) = (self.blocks, self.order);
        let head_holds = part.before.is_none_or(|b| placed.first() == Some(&blocks[order[b]]));
        let tail_holds = part.after.is_none_or(|a| {
            let mut moved = blocks[order[a]].clone();
            shift_block(&mut moved, self.delta);
            placed.last() == Some(&moved)
        });
        head_holds && tail_holds
    }

    /// Rule 3 again: numbers and the order of the definitions stay as long as references and definitions do.
    fn footnotes_stay(&self, part: &Part, placed: &[Block], old_references: &[(&str, u32)]) -> Result<(), WholeParse> {
        if references(placed) != old_references || holds_nested_definition(placed) {
            return Err(WholeParse::Footnotes);
        }
        let written = self.old_blocks(part).filter(|b| is_definition(b));
        if !definitions_stay(written, placed, &self.old_change, self.delta) {
            return Err(WholeParse::Footnotes);
        }
        Ok(())
    }

    /// What the part from block `before` up to block `after` of `order` comes to; `None` if the change reaches beyond them.
    fn try_part(&self, before: Option<usize>, after: Option<usize>) -> Result<Option<Replacement>, WholeParse> {
        let part = self.part_between(before, after)?;
        self.footnotes_can_be_told(&part)?;
        let old_references = references(self.old_blocks(&part));
        let known: HashMap<&str, u32> = old_references.iter().copied().collect();
        let mut read = parse_window(&Window {
            text: part.new,
            at_start: part.start == Byte::default(),
            at_end: after.is_none(),
            links: self.links,
        })?;
        Self::references_keep_their_numbers(&part, &mut read, &known)?;
        self.link_definitions_stay(&part, &read)?;
        let reference_expansions = self.expansions_after(&part, &read)?;
        let mut placed = read.placed_at(part.start);
        if !self.neighbours_hold(&part, &placed) {
            return Ok(None);
        }
        self.footnotes_stay(&part, &placed, &old_references)?;
        placed.retain(|b| !is_definition(b));
        let text = &self.blocks[..self.text_blocks];
        let old = text.partition_point(|b| b.range.start < part.start)
            ..text.partition_point(|b| b.range.start < part.old_end);
        Ok(Some(Replacement {
            old,
            part: placed,
            window: part.start..part.new_end,
            old_window: part.start..part.old_end,
            reference_expansions,
        }))
    }
}

/// A debug build looks at all of a text of up to this size after each partial parse.
const MOST_CHECKED_BYTES: usize = 1 << 20;

/// Whether the blocks lie in their text in order, with what comrak moved behind them coming last.
fn blocks_fit(doc: &Doc) -> bool {
    let text = doc.text();
    let fits = |b: &Block| b.range.end <= text.end() && text.boundary(b.range.start) && text.boundary(b.range.end);
    let in_text = doc.blocks.iter().take_while(|b| !is_definition(b));
    doc.blocks.iter().all(fits) && in_text.is_sorted_by_key(|b| b.range.start)
}

/// Updates `doc` to `new_text` by parsing only what the change can affect. `None` when the whole document has to be
/// parsed again; `doc` is unchanged then.
pub fn reparse(doc: &mut Doc, new_text: &str) -> Option<Splice> {
    reparse_or_why_not(doc, new_text).ok()
}

/// `reparse`, with the reason if all of `new_text` has to be parsed.
pub(crate) fn reparse_or_why_not(doc: &mut Doc, new_text: &str) -> Result<Splice, WholeParse> {
    let shared = doc.shared_text();
    let old_text: &str = &shared;
    let facts = doc.facts().clone();
    let (old_change, new_change) = changed(old_text, new_text);
    let marks = facts.marks.after_change(old_text, new_text, &old_change, &new_change);
    if marks.memory >= MOST_MEMORY {
        return Err(WholeParse::TooMuch);
    }
    let unclosed = unclosed_after_change(facts.unclosed_formulas, old_text, new_text, &old_change, &new_change);
    if let Some(why) = read_as_a_whole(&facts, unclosed) {
        return Err(why);
    }
    let links = facts.link_definitions.clone().ok_or(WholeParse::LinkDefinitions)?;
    // A `---` line at the very start opens front matter that the next `---` line anywhere would close. comrak also goes by
    // the line ends of the text to find that line, and skips a byte order mark.
    if opens_front_matter(old_text) || opens_front_matter(new_text) {
        let closed_before = doc.blocks.first().is_some_and(|b| {
            matches!(b.kind, BlockKind::FrontMatter { .. })
                && next_line_start(old_text, b.range.end).is_some_and(|behind| behind <= old_change.start)
        });
        let line_ends_change =
            old_text.at(old_change.clone()).contains('\r') || new_text.at(new_change.clone()).contains('\r');
        if !closed_before || line_ends_change {
            return Err(WholeParse::FrontMatter);
        }
    }
    let delta = growth(new_text.end(), old_text.end());
    let blocks = &doc.blocks;
    let text_blocks = blocks.iter().rposition(|b| !is_definition(b)).map_or(0, |i| i + 1);
    let order = source_order(blocks, text_blocks).ok_or(WholeParse::Footnotes)?;
    let n = order.len();
    // Unchanged blocks on either side, parsed again to show the change does not reach into them.
    let first = order.partition_point(|&i| blocks[i].range.end < old_change.start);
    let after_change = order.partition_point(|&i| blocks[i].range.start <= old_change.end);
    let mut before = first.checked_sub(1);
    let mut after = (after_change < n).then_some(after_change);
    let change = Change {
        blocks,
        old_text,
        new_text,
        order: &order,
        text_blocks,
        old_change: old_change.clone(),
        delta,
        may_define_links: facts.may_define_links,
        links: &links,
        reference_expansions: facts.reference_expansions,
    };
    let mut step = 1;
    let replacement = loop {
        if let Some(replacement) = change.try_part(before, after)? {
            break replacement;
        }
        if before.is_none() && after.is_none() {
            return Err(WholeParse::TooMuch);
        }
        before = before.and_then(|b| b.checked_sub(step));
        after = after.map(|a| a + step).filter(|&a| a < n);
        step *= 2;
    };
    let Replacement {
        old,
        part,
        window,
        old_window,
        reference_expansions,
    } = replacement;
    let new = old.start..old.start + part.len();
    replace_blocks(doc, old.clone(), part, &old_change, delta);
    // A `]:` is told from a footnote's by its line, which lies in the part parsed again.
    let touched_a_mark = old_text.at(old_window).contains("]:") || new_text.at(window.clone()).contains("]:");
    let facts = TextFacts {
        marks,
        unclosed_formulas: unclosed,
        may_define_links: if touched_a_mark {
            may_define_links(new_text)
        } else {
            facts.may_define_links
        },
        reference_expansions,
        ..facts
    };
    doc.renew(new_text, facts);
    debug_assert!(blocks_fit(doc), "the blocks after a partial parse leave their text");
    debug_assert!(
        new_text.len() > MOST_CHECKED_BYTES || *doc.facts() == TextFacts::of(doc),
        "what a partial parse carries on is no longer what the text says"
    );
    Ok(Splice {
        old,
        new,
        delta,
        window,
        behind: new_change.end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::facts::{HEAVIEST_BYTE, LONGEST_LABEL, MEMORY_OF};
    use crate::reader::parse;

    use crate::text::bytes;

    fn reparsed(old: &str, new: &str) -> Parsed {
        let doc = parse(old);
        let spans = highlight(&doc);
        reparse_and_color(doc, &spans, new)
    }

    #[test]
    fn links_to_a_long_address_read_the_same_in_part_as_in_the_whole_text() {
        let address = format!("https://example.com/{}", "x".repeat(200));
        let padding: String = (0..5000).map(|i| format!("Absatz {i} mit etwas Text.\n\n")).collect();
        let references = "[a] ".repeat(100);
        let old = format!("{references}\n\nvorher\n\n{padding}[a]: {address}\n");
        for new in [
            old.replacen("vorher", "vorherx", 1),
            old.replacen("[a] [a]", "[a] x[a]", 1),
        ] {
            let parsed = reparsed(&old, &new);
            assert!(parsed.splice.is_some());
            assert_eq!(parsed.doc.blocks, parse(&new).blocks);
        }
    }

    #[test]
    fn links_beyond_what_they_may_expand_to_take_a_full_parse() {
        let address = format!("https://example.com/{}", "x".repeat(2000));
        let old = format!("{}\n\nvorher\n\nnoch\n\n[a]: {address}\n", "[a] ".repeat(60));
        let new = old.replacen("vorher", "vorherx", 1);
        let parsed = reparsed(&old, &new);
        assert!(parsed.splice.is_none());
        assert_eq!(parsed.doc.blocks, parse(&new).blocks);
    }

    #[test]
    fn links_beyond_what_a_part_may_expand_to_take_a_full_parse() {
        let address = format!("https://example.com/{}", "x".repeat(2000));
        let padding: String = (0..12_000).map(|i| format!("Absatz {i} mit etwas Text.\n\n")).collect();
        let old = format!("{}\n\nvorher\n\n{padding}[a]: {address}\n", "[a] ".repeat(60));
        assert!(old.len() > 200_000, "all of the text can take more than a part");
        let new = old.replacen("[a] [a]", "[a] x[a]", 1);
        let parsed = reparsed(&old, &new);
        assert!(parsed.splice.is_none());
        assert_eq!(parsed.doc.blocks, parse(&new).blocks);
    }

    #[test]
    fn what_links_expand_to_follows_a_change() {
        let address = "https://example.com/abc";
        let old = format!("[a]\n\nvorher\n\nnoch\n\n[a]: {address}\n{}", " ".repeat(500));
        let new = old.replacen("vorher", "[a] [a]", 1);
        let parsed = reparsed(&old, &new);
        assert!(parsed.splice.is_some());
        assert_eq!(parsed.doc.facts().reference_expansions, 3 * address.len());
        assert_eq!(
            parsed.doc.facts().reference_expansions,
            TextFacts::of(&parse(&new)).reference_expansions
        );
    }

    #[test]
    fn every_parse_makes_a_new_version() {
        assert_eq!(Doc::default().version(), 0);
        let old = "Eins\n\nZwei\n\nDrei\n\nVier\n\nFünf\n\nSechs\n\nSieben\n";
        let (first, second) = (parse(old), parse(old));
        assert_ne!(first.version(), second.version(), "the same text, parsed twice");
        let new = old.replace("Vier", "Viertel");
        let doc = parse(old);
        let version = doc.version();
        let spans = highlight(&doc);
        assert!(reparse_part_and_color(doc, &spans, old).is_ok_and(|p| p.doc.version() != version));
        let doc = parse(old);
        let version = doc.version();
        let parsed = reparse_and_color(doc, &highlight(&parse(old)), &new);
        assert!(parsed.splice.is_some());
        assert_ne!(parsed.doc.version(), version, "only a part parsed");
        assert_eq!(parsed.doc.text(), new);
    }

    #[test]
    fn a_failed_partial_parse_leaves_the_document_as_it_was() {
        let (old, new) = ("Text[^1]\n\n[^1]: Fuß\n", "Text[^1]\n\n[^1]: Füße\n");
        let doc = parse(old);
        let version = doc.version();
        let spans = highlight(&doc);
        let doc = reparse_part_and_color(doc, &spans, new).err().expect("a full parse");
        assert_eq!((doc.version(), doc.text()), (version, old));
        assert_eq!(doc.blocks, parse(old).blocks);
    }

    #[test]
    fn a_plain_text_keeps_its_text_without_blocks() {
        let parsed = Parsed::plain("# Titel\n".into());
        assert!(parsed.doc.blocks.is_empty());
        assert_eq!(parsed.doc.text(), "# Titel\n");
        assert_ne!(parsed.doc.version(), 0);
    }

    #[test]
    fn a_partial_parse_takes_the_document_over() {
        let old = "Eins\n\nZwei\n\nDrei\n\nVier\n\nFünf\n\nSechs\n\nSieben\n";
        let new = old.replace("Vier", "Viertel");
        let new = new.as_str();
        let parsed = reparsed(old, new);
        assert_eq!(parsed.splice.map(|s| (s.new, s.delta)), Some((2..5, 3)));
        assert_eq!(parsed.doc.blocks, parse(new).blocks);
        assert_eq!(parsed.spans, highlight(&parse(new)));
    }

    #[test]
    fn the_content_decides_what_stays_plain_text() {
        let size = 64 << 20;
        let prose = "Ein Satz, wie er in einem langen Text ohne jede Auszeichnung stehen kann\n";
        assert!(!stays_plain(&prose.repeat(size / prose.len())));
        assert!(stays_plain(&"| a |\n".repeat(size / 6)));
        assert!(!stays_plain(&"| a |\n".repeat(1000)), "too short to matter");
        assert!(!stays_plain(""));
    }

    #[test]
    fn no_byte_outweighs_the_heaviest() {
        assert_eq!(MEMORY_OF.iter().max().map(|m| u64::from(*m)), Some(HEAVIEST_BYTE));
    }

    #[test]
    fn the_estimate_adds_up_the_bytes() {
        assert_eq!(memory_estimate("a"), 4);
        assert_eq!(memory_estimate("# a\n"), 800 + 4 + 4 + 750);
    }

    #[test]
    fn only_a_text_over_the_limit_counts_as_too_large() {
        let huge = "# Titel\n".repeat(3 << 20);
        let parsed = parse_and_color(&huge);
        assert!(parsed.doc.is_too_large());
        assert!(!Parsed::failed(huge.as_str().into()).doc.is_too_large());
        assert!(!parse_and_color("# Titel\n").doc.is_too_large());
        let shrunk = reparse_and_color(parsed.doc, &[], "# Titel\n");
        assert!(!shrunk.doc.is_too_large());
    }

    #[test]
    fn a_document_over_the_limit_stays_plain_text() {
        let huge = "# Titel\n".repeat(3 << 20);
        let parsed = parse_and_color(&huge);
        assert!(parsed.doc.blocks.is_empty() && parsed.spans.is_empty());
        let doc = parse("# Titel\n");
        let spans = highlight(&doc);
        let doc = reparse_part_and_color(doc, &spans, &huge).err().expect("a full parse");
        assert!(reparse_and_color(*doc, &spans, &huge).doc.blocks.is_empty());
        let shrunk = reparse_and_color(parsed.doc, &[], "# Titel\n");
        assert_eq!(shrunk.doc.blocks, parse("# Titel\n").blocks);
    }

    #[test]
    fn an_inline_footnote_near_the_change_takes_a_full_parse() {
        // Their numbers run through the whole document, and no `]:` gives them away.
        let (old, new) = ("Eins^[a]\n\nZwei\n", "Eins^[a]\n\nZwei^[b]\n");
        let parsed = reparsed(old, new);
        assert!(parsed.splice.is_none());
        assert_eq!(parsed.doc.blocks, parse(new).blocks);
    }

    #[test]
    fn a_change_starts_and_ends_at_whole_letters() {
        assert_eq!(
            changed("x é y", "x è y"),
            (bytes(2..4), bytes(2..4)),
            "the letters begin alike"
        );
        assert_eq!(
            changed("x éy", "x ĩy"),
            (bytes(2..4), bytes(2..4)),
            "the letters end alike"
        );
        assert_eq!(changed("abXcd", "abYcd"), (bytes(2..3), bytes(2..3)));
        assert_eq!(
            changed("aa", "aaa"),
            (bytes(2..2), bytes(2..3)),
            "the common parts do not overlap"
        );
    }

    fn partly(old: &str, new: &str) -> Option<Splice> {
        let parsed = reparsed(old, new);
        assert_eq!(parsed.doc.blocks, parse(new).blocks, "{old:?} to {new:?}");
        parsed.splice
    }

    #[test]
    fn what_reaches_across_blocks_on_either_side_of_the_change_takes_a_full_parse() {
        let plain = "a\n\nb\n\nc\n\nd\n";
        for (special, why) in [
            ("a\n\nb\n\nc\n\nd[^1]\n\n[^1]: e\n", "footnote"),
            ("a\n\nb\n\nc\n\nd^[e]\n", "inline footnote"),
            ("a\n\nb\n\nc\n\n%%d\n", "comment"),
            ("---\na\n\nb\n\nc\n\nd\n", "front matter"),
        ] {
            assert_eq!(partly(plain, special), None, "{why} comes");
            assert_eq!(partly(special, plain), None, "{why} goes");
        }
    }

    #[test]
    fn what_only_looks_like_a_footnote_leaves_the_parse_partial() {
        for stray in ["```\n[^a-z]\n```\n", "```\n^[a-z]\n```\n", "[^a-z]\n"] {
            let old = format!("a\n\nb\n\nc\n\nd\n\ne\n\n{stray}");
            assert!(partly(&old, &old.replacen("b\n", "bb\n", 1)).is_some(), "{stray:?}");
        }
        let code = "a\n\n```\n[^a-z]\n```\n\nb\n\nc\n";
        assert!(
            partly(code, &code.replacen("a\n", "aa\n", 1)).is_some(),
            "next to the change"
        );
    }

    /// Text, references and definitions in the order of the source, with the definitions written before, between and
    /// behind the rest.
    const NOTES: &str = "[^3]: drei\n\na\n\nb[^1]\n\nc\n\n[^2]: zwei\n    und mehr\n\nd[^2]\n\ne[^1]\n\nf[^3]\n\n> j[^1]\n\ng\n\n[^1]: eins\n\nh\n\ni\n";

    #[test]
    fn typing_around_footnotes_leaves_the_parse_partial() {
        for (from, to) in [
            ("a\n", "aa\n"),
            ("b[", "bb["),
            ("c\n", "cc\n"),
            ("d[", "dd["),
            ("e[^1]", "ee[^1] und"),
            ("> j[", "> jj["),
            ("g\n", "gg\n"),
            ("h\n", "hh\n"),
            ("\n\ni\n", "\n"),
        ] {
            assert!(partly(NOTES, &NOTES.replacen(from, to, 1)).is_some(), "{from:?}");
        }
    }

    #[test]
    fn a_change_to_what_the_numbers_depend_on_takes_a_full_parse() {
        for (from, to, why) in [
            ("zwei", "zweite", "a definition"),
            ("a\n", "a[^1]\n", "a reference comes"),
            ("b[^1]", "b", "a reference goes"),
            ("d[^2]", "d[^1]", "another reference"),
            ("c\n", "c[^4]\n", "a reference without a definition"),
            ("c\n", "[^4]: vier\n", "a definition comes"),
            ("[^3]: drei\n\n", "", "a definition goes"),
            ("a\n", "    a\n", "a block joins the definition before it"),
            ("g\n", "```\ng\n", "a block left open reaches the definition behind it"),
        ] {
            let new = NOTES.replacen(from, to, 1);
            assert_eq!(partly(NOTES, &new), None, "{why}");
        }
    }

    #[test]
    fn a_definition_gives_a_stray_reference_far_away_its_meaning() {
        let stray = "[^1]\n\na\n\nb\n\nc\n\nd\n\ne\n\nf\n";
        assert_eq!(partly(stray, &stray.replace("e\n", "[^1]: g\n")), None);
        let unused = "[^1]: g\n\na\n\nb\n\nc\n\nd\n\ne\n\nf\n";
        assert_eq!(partly(unused, &unused.replace("e\n", "[^1]\n")), None);
    }

    #[test]
    fn the_references_in_an_unused_definition_still_count() {
        let old = "[^3]: drei\n\na\n\n[^2]: zwei[^3]\n\nb\n\nc\n\nd\n";
        assert_eq!(partly(old, &old.replace("[^2]: zwei[^3]\n\n", "")), None);
    }

    #[test]
    fn a_reference_the_reader_gave_up_on_still_counts() {
        let old = "a\n\nb\n\n|---|---|\n\nc\n\nd\n\n[^1]: e\n\nf\n";
        assert_eq!(partly(old, &old.replace("|---|---|", "|---[^1]|---|")), None);
    }

    #[test]
    fn a_definition_inside_another_block_takes_a_full_parse() {
        let moved = "> [^1]: e\n\na[^1]\n\nb\n\nc\n\nd\n";
        assert_eq!(partly(moved, &moved.replace("c\n", "cc\n")), None);
        let unused = "a\n\nb\n\nc\n\nd\n\ne\n";
        assert_eq!(partly(unused, &unused.replace("c\n", "> [^1]: f\n")), None);
    }

    #[test]
    fn labels_that_differ_only_by_their_case_folding_count_as_defined_twice() {
        let old = "[SS]: http://a\n\nb\n\nc\n\n[ß]\n\n[ß]: http://b\n\nd\n\ne\n";
        let new = old.replace("[ß]\n\n", "[ß] x\n\n");
        let doc = parse(old);
        let spans = highlight(&doc);
        let parsed = reparse_and_color(doc, &spans, &new);
        assert!(parsed.splice.is_none(), "a part would read the later definition");
        assert_eq!(parsed.doc.blocks, parse(&new).blocks);
    }

    #[test]
    fn what_looks_like_a_definition_in_front_matter_leaves_the_parse_partial() {
        let old = "---\nx: [a]: b\n---\n\nEins\n\nZwei\n\nDrei\n\nVier\n\nFünf\n";
        let new = old.replace("Zwei", "Zwei x");
        let doc = parse(old);
        let spans = highlight(&doc);
        let parsed = reparse_and_color(doc, &spans, &new);
        assert!(parsed.splice.is_some(), "a keystroke need not read all of it");
        assert_eq!(parsed.doc.blocks, parse(&new).blocks);
    }

    #[test]
    fn a_table_that_comes_below_a_paragraph_undoes_the_definition_it_is_headed_by() {
        let old = "Eins\n\nZwei\n\n[a]: e\nDrei\n\nVier\n";
        let new = old.replace("Drei\n", "Drei\nf\n|---|\n9\n");
        let doc = parse(old);
        let spans = highlight(&doc);
        let parsed = reparse_and_color(doc, &spans, &new);
        assert_eq!(parsed.doc.blocks, parse(&new).blocks);
    }

    #[test]
    fn a_change_below_a_definition_above_a_paragraph_reads_as_a_full_parse() {
        let old = "Eins\n\n[a]: e\nZwei [a]\n\nDrei\n\nVier\n";
        let new = old.replace("Zwei", "Zwei x");
        let doc = parse(old);
        let spans = highlight(&doc);
        let parsed = reparse_and_color(doc, &spans, &new);
        assert_eq!(parsed.doc.blocks, parse(&new).blocks);
    }

    #[test]
    fn a_second_definition_of_a_label_takes_a_full_parse() {
        let old = "a[^1]\n\n[^1]: b\n\n[^1]: c\n";
        assert_eq!(partly(old, &old.replace("c\n", "c\n> \n")), None);
        assert_eq!(partly(old, &old.replacen("\n\n[^1]: c", "\n> \n[^1]: c", 1)), None);
        let cased = "a[^x]\n\n[^X]: b\n\n[^x]: c\n\nd\n\ne\n\nf\n";
        assert_eq!(partly(cased, &cased.replace("e\n", "ee\n")), None);
    }

    #[test]
    fn a_footnote_at_the_end_of_the_document_takes_a_full_parse() {
        let old = "[^1]: e\n\na\n\nb\n\nc[^1]\n\n- d[^1]\n- f";
        assert_eq!(partly(old, &format!("{old}\n")), None);
        assert_eq!(partly(old, &format!("{old}x")), None);
        let no_line_break = "a\n\nb[^1]\n\nc\n\n[^1]: e";
        assert_eq!(
            partly(no_line_break, &no_line_break.replace("c\n", "cc\n")).map(|s| s.delta),
            Some(1)
        );
    }

    #[test]
    fn labels_are_what_a_definition_can_have() {
        assert_eq!(footnote_label("[^a] b"), Some("a"));
        for no_label in [
            "[a]", "[^]", "[^a", "[^a b]", "[^a\tb]", "[^a\nb]", "[^a\rb]", "[^a\0b]",
        ] {
            assert_eq!(footnote_label(no_label), None, "{no_label:?}");
        }
        let label = |length: usize| format!("[^{}]", "a".repeat(length));
        assert!(footnote_label(&label(LONGEST_LABEL)).is_some());
        assert_eq!(footnote_label(&label(LONGEST_LABEL + 1)), None);
    }

    #[test]
    fn a_footnote_definition_is_no_link_definition() {
        assert!(!may_define_links("[^1]: a\n\nü[^2]: b\n"));
        for link in ["[r]: u\n", "[^1]: a\n[r]: u\n", "[^a b]: u\n", "[^]: u\n", "a]: u\n"] {
            assert!(may_define_links(link), "{link:?}");
        }
        let label = |text: &str| format!("[^{text}]: u\n");
        assert!(!may_define_links(&label(&"a".repeat(LONGEST_LABEL))));
        assert!(may_define_links(&label(&"a".repeat(LONGEST_LABEL + 1))));
        assert!(may_define_links(&label(&format!("1{}", "ü".repeat(LONGEST_LABEL)))));
        assert!(!holds_link_bracket("[^1] und [^2]"));
        for bracket in ["[r]", "[^1][r]", "[^a b]", "[[^1]]"] {
            assert!(holds_link_bracket(bracket), "{bracket:?}");
        }
    }

    #[test]
    fn stand_ins_cover_each_label_once_up_to_a_limit() {
        assert_eq!(
            stand_ins("[^a] [^b] [^a]").as_deref(),
            Some("\n\n[^a]: x\n\n\n[^b]: x\n")
        );
        assert_eq!(stand_ins("[a] [^] x").as_deref(), Some(""));
        let labels = |n: usize| (0..n).map(|i| format!("[^{i}]")).collect::<String>();
        assert!(stand_ins(&labels(MOST_STAND_INS)).is_some());
        assert_eq!(stand_ins(&labels(MOST_STAND_INS + 1)), None);
    }

    #[test]
    fn what_looks_like_a_definition_is_counted() {
        assert_eq!(looks_like_definitions("[^1]: a\n\n[r]: u\n\n```\n[^2]: b\n```\n"), 2);
    }

    #[test]
    fn definitions_keep_one_color_when_all_of_the_text_is_colored_anew() {
        let (old, new) = (
            "a[^1]\n\n[^1]: e\n\nb\n\nc\n\nd\n",
            "a[^1]\n\n[^1]: e\n\nbb\n\nc\n\nd\n",
        );
        let doc = parse(new);
        let splice = reparse(&mut parse(old), new).unwrap();
        let once = highlight(&doc);
        assert_eq!(with_definitions_in_window(once.clone(), &doc, &splice), once);
    }

    #[test]
    fn definitions_between_the_changed_blocks_keep_their_colors() {
        let old = "a[^1]\n\n[^1]: e\n\nb\n\nc\n\nd\n";
        let new = old.replace("b\n", "bb\n");
        let parsed = reparsed(old, &new);
        assert!(parsed.splice.is_some());
        assert_eq!(parsed.spans, highlight(&parse(&new)));
    }

    #[test]
    fn a_far_footnote_leaves_the_parse_partial_with_its_offsets_moved_along() {
        let old = "[^1]: e\n\na[^1][^2]\n\nb\n\nc\n\nd\n\nf\n\n[^2]: g\n";
        assert!(partly(old, &old.replace("c\n", "cc\n")).is_some());
        let inline = "a^[g]\n\nb\n\nc\n\nd\n\ne\n\nf^[h]\n";
        assert!(partly(inline, &inline.replace("c\n", "cc\n")).is_some());
    }

    #[test]
    fn one_closing_delimiter_far_away_can_switch_formulas_back_on() {
        let old = format!("{}\n\n\\(x\\)\n\na\n\nb\n\nc\n\nd\n\ne\n", "\\( ".repeat(1001));
        let new = old.replace("d\n", "d\\)\n");
        assert_eq!(partly(&old, &new), None);
        assert_eq!(partly(&new, &old), None);
    }

    #[test]
    fn many_unclosed_inline_notes_take_a_full_parse() {
        let far = |n: usize| format!("```\n{}\n```\n\na\n\nb\n\nc\n\nd\n\ne\n", "^[".repeat(n));
        let change = |old: String| partly(&old, &old.replace("d\n", "dd\n"));
        assert!(change(far(1000)).is_some());
        assert_eq!(change(far(1001)), None);
        // One `]` far from the rest switches inline notes back on for the whole document.
        let switched_off = format!("{}\n\n^[g]\n\na\n\nb\n\nc\n\nd\n\ne\n", "^[x ".repeat(1001));
        assert_eq!(partly(&switched_off, &switched_off.replace("d\n", "d]\n")), None);
        // So does one `^[` less, and nothing of it is left in the new part.
        let one_too_many = format!("{}\n\n^[g]\n\na\n\nb\n\nc\n\nd ^[\n\ne\n", "^[x ".repeat(1000));
        assert_eq!(partly(&one_too_many, &one_too_many.replace("d ^[\n", "d\n")), None);
    }

    #[test]
    fn brackets_near_the_change_read_the_definitions_of_the_whole_text() {
        let old = "[r]: u\n\na\n\nb\n\nc\n\nd\n\ne\n\nf\n";
        assert!(partly(old, &old.replace("e\n", "ee\n")).is_some(), "no brackets near");
        let brackets = old.replace("e\n", "[r]\n");
        assert!(partly(old, &brackets).is_some(), "brackets come");
        assert!(partly(&brackets, old).is_some(), "brackets go");
    }

    #[test]
    fn front_matter_is_only_safe_when_the_change_lies_behind_it() {
        let old = "---\nt: 1\n---\n\nb\n\nc\n\nd\n";
        assert!(partly(old, &old.replace("c\n", "cc\n")).is_some());
        assert!(
            partly(old, &old.replace("b\n", "bb\n")).is_some(),
            "it is parsed again with the change"
        );
        assert_eq!(partly(old, &old.replace("t: 1", "t: 2")), None);
        let end = old.find("\n\nb").unwrap();
        let at_its_end = format!("{}x{}", &old[..end], &old[end..]);
        assert_eq!(partly(old, &at_its_end), None, "it ends right before the change");
        assert_eq!(
            partly("---\n\nb\n\nc\n", "---\n\nb\n\ncc\n"),
            None,
            "a rule that a later one could close"
        );
    }

    #[test]
    fn a_rule_in_the_middle_never_opens_front_matter() {
        let old = "a\n\n---\n\nb\n\nc\n\nd\n";
        let splice = partly(old, &old.replace("b\n", "z\n\n---\n\nb\n")).unwrap();
        assert_eq!(splice.old, 1..4);
    }

    #[test]
    fn a_change_that_reaches_on_widens_until_the_blocks_agree_again() {
        let old = "a\n\nb\n\nc -->\n\nd\n\ne\n\nf\n\ng\n";
        let new = old.replacen("a\n", "<!--\n", 1);
        let splice = partly(old, &new).unwrap();
        assert_eq!(splice.old, 0..5);
        assert_eq!(splice.new, 0..3);
    }

    fn paragraphs(n: usize, length: usize) -> String {
        (0..n).map(|_| "x".repeat(length - 2) + "\n\n").collect()
    }

    #[test]
    fn past_half_of_a_large_document_a_full_parse_is_no_slower() {
        let change = |old: &str, block: usize, length: usize| {
            let mut new = old.to_string();
            new.replace_range(block * length..block * length + 1, "y");
            partly(old, &new)
        };
        let exactly_half = paragraphs(6, 1000);
        assert!(change(&exactly_half, 1, 1000).is_some());
        assert_eq!(change(&paragraphs(5, 1000), 1, 1000), None);
        assert!(
            change(&paragraphs(30, 1000), 10, 1000).is_some(),
            "the window lies far behind the start"
        );
        assert!(
            change(&paragraphs(5, 100), 1, 100).is_some(),
            "a small document is cheap either way"
        );
        assert!(change(&paragraphs(4, 1024), 1, 1024).is_some(), "up to the limit");
        assert_eq!(change(&paragraphs(4, 1025), 1, 1025), None, "over the limit");
    }

    #[test]
    fn front_matter_is_parsed_again_as_such_with_the_start_of_the_document() {
        let old = "---\nt: 1\n---\n\nb\n\nc\n";
        let new = old.replace("b\n", "bb\n");
        let parsed = reparsed(old, &new);
        assert!(matches!(parsed.doc.blocks[0].kind, BlockKind::FrontMatter { .. }));
    }

    #[test]
    fn typing_near_links_stays_partial_when_their_definitions_stay() {
        let long = "lang ".repeat(40);
        let old = format!(
            "[r] und [t][r]\n\na\n\nb\n\nc\n\nd\n\ne\n\n[r]: http://x.y \"Titel\"\n\nf\n\ng\n\n{long}\n\nh\n\ni\n"
        );
        let old = old.as_str();
        assert!(
            partly(old, &old.replacen("c\n", "c [r]\n", 1)).is_some(),
            "a bracket typed"
        );
        assert!(partly(old, &old.replacen("d\n", "dd\n", 1)).is_some(), "plain text");
        let at_end = old.replacen("i\n", "i [r]\n", 1);
        let splice = partly(old, &at_end).expect("the last block");
        assert!(splice.window.end == at_end.end(), "{splice:?}");
    }

    #[test]
    fn a_change_of_a_definition_in_front_of_a_heading_takes_a_full_parse() {
        let old = "[a]:ä\nt\n-\nB\n\n[a]\n";
        assert_eq!(partly(old, &old.replace("ä", "ö")), None);
        let old = "[a]:> e\n-\n~~~\n~~~\n[a]\n";
        assert_eq!(partly(old, &old.replace("> e", "> \ne")), None);
    }

    #[test]
    fn a_change_of_a_definition_itself_takes_a_full_parse() {
        let old = "[r]\n\na\n\nb\n\nc\n\nd\n\ne\n\n[r]: http://x.y\n\nf\n\ng\n\nh\n\ni\n";
        assert_eq!(partly(old, &old.replace("http://x.y", "http://z")), None);
        assert_eq!(partly(old, &old.replace("c\n", "[s]: u\n")), None);
        assert_eq!(partly(old, &old.replace("[r]: http://x.y\n\n", "")), None);
    }

    #[test]
    fn a_block_left_open_at_the_end_does_not_swallow_the_definitions_unseen() {
        let old = "[r]\n\n[r]: u\n\na\n\nb\n\nc\n";
        assert!(
            partly(old, &old.replacen("c\n", "```\nc [r]\n", 1)).is_none_or(|s| s.window.end.get() > 0),
            "either way the same tree"
        );
    }

    #[test]
    fn brackets_in_code_are_no_definitions() {
        let old = "a\n\n```\nx[0]: 1\n```\n\nb [c]\n\nd\n\ne\n\nf\n\ng\n\nh\n";
        assert!(partly(old, &old.replacen("d\n", "dd\n", 1)).is_some());
        assert!(
            partly(old, &old.replacen("b [c]", "bb [c]", 1)).is_some(),
            "right behind it"
        );
    }

    #[test]
    fn marks_of_a_comment_in_code_leave_the_parse_partial() {
        for around in ["```mermaid\n%%{init}%%\ngraph\n```\n", "```\n%% a\n%% b\n```\n"] {
            let old = format!("{around}\na\n\nb\n\nc\n\nd\n\ne\n\nf\n\ng\n");
            assert!(partly(&old, &old.replacen("d\n", "dd\n", 1)).is_some(), "{around:?}");
        }
    }

    #[test]
    fn a_full_parse_says_why() {
        let why = |old: &str, new: &str| reparse_or_why_not(&mut parse(old), new).unwrap_err();
        let plain = "a\n\nb\n\nc\n\nd\n";
        assert_eq!(why("%%a\n\nb\n\nc\n", "%%a\n\nbb\n\nc\n"), WholeParse::LooseComment);
        assert_eq!(why(plain, "a\n\nb\n\nc\n\n%%d\n"), WholeParse::LooseComment);
        assert_eq!(why("---\na\n\nb\n", "---\na\n\nbb\n"), WholeParse::FrontMatter);
        assert_eq!(why(plain, "a\n\nb\n\nc\n\nd^[e]\n"), WholeParse::Footnotes);
        assert_eq!(why(plain, "a\n\nb\n\nc\n\n[r]: u\n"), WholeParse::LinkDefinitions);
        let many_notes = "^[".repeat(UNCLOSED_MARKS + 1);
        assert_eq!(why(&many_notes, &format!("{many_notes}x")), WholeParse::SwitchedOff);
    }

    #[test]
    fn a_link_definition_that_comes_or_goes_can_change_brackets_far_away() {
        let without = "[r]\n\na\n\nb\n\nc\n\nd\n\ne\n";
        let with = format!("{without}\n[r]: u\n");
        assert_eq!(partly(without, &with), None);
        assert_eq!(partly(&with, without), None);
    }
}
