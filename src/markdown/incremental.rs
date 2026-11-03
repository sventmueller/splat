//! Re-parsing only the top-level blocks around an edit. Markdown is mostly local: a blank line between top-level
//! blocks ends everything except front matter, footnotes and link references, which are checked for.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use super::doc::{
    Block, BlockKind, Doc, Inline, InlineKind, formulas_are_limited, inline_notes_may_be_limited, parse, parse_part,
    walk_blocks, walk_inlines,
};
use super::highlight::{Style, highlight, rehighlight};
use super::text::{common_prefix, common_suffix, growth, line_start, shifted};

/// What an edit replaced: the top-level block indexes before and after, and the shift of everything behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    pub old: Range<usize>,
    pub new: Range<usize>,
    pub delta: isize,
    /// The re-parsed part of the new text; offsets from its end on are shifted by `delta`.
    pub window: Range<usize>,
    /// Where the change ends in the new text: blocks outside `new` from here on moved by `delta`, those before it not.
    pub behind: usize,
}

/// The parsed document and its source colors.
pub struct Parsed {
    pub doc: Doc,
    pub spans: Vec<(Range<usize>, Style)>,
    /// Which top-level blocks changed, when only those were parsed again.
    pub splice: Option<Splice>,
}

/// The estimated memory from which a document stays plain text in the editor.
const MOST_MEMORY: u64 = 4 << 30;

const HEAVIEST_BYTE: u64 = 800;

/// Roughly what a byte adds to the peak memory of tree, colors and layout, measured per syntax; every byte that can
/// start a node counts, as telling code from markup here would let a crafted document pass for code.
const fn memory_of(byte: u8) -> u16 {
    match byte {
        b'#' | b'|' | b'^' | b'@' => 800,
        b'\n' => 750,
        b'*' | b'_' | b'[' => 600,
        b'`' | b'<' | b'>' | b'$' | b'&' | b'"' | b'~' | b'=' => 400,
        b']' | b'(' | b')' | b'\\' | b'-' | b'+' | b':' | b'.' => 250,
        _ => 4,
    }
}

const MEMORY_OF: [u16; 256] = {
    let mut table = [memory_of(0); 256];
    let mut byte = u8::MAX;
    while byte > 0 {
        table[byte as usize] = memory_of(byte);
        byte -= 1;
    }
    table
};

/// The memory `text` is estimated to take once it is parsed and laid out.
pub fn memory_estimate(text: &str) -> u64 {
    text.bytes().map(|b| u64::from(MEMORY_OF[usize::from(b)])).sum()
}

/// Whether `text` would take too much memory to parse and lay out; shorter texts need no look at their bytes.
pub fn stays_plain(text: &str) -> bool {
    text.len() as u64 * HEAVIEST_BYTE >= MOST_MEMORY && memory_estimate(text) >= MOST_MEMORY
}

const LONGEST_LABEL: usize = 1000;
const MOST_STAND_INS: usize = 1000;

impl Parsed {
    /// No blocks and no colors: commands treat the text as plain, and the preview shows nothing.
    pub fn plain() -> Parsed {
        Parsed {
            doc: Doc::default(),
            spans: Vec::new(),
            splice: None,
        }
    }
}

pub fn parse_and_color(text: &str) -> Parsed {
    if stays_plain(text) {
        return Parsed::plain();
    }
    let doc = parse(text);
    let spans = highlight(text, &doc);
    Parsed {
        doc,
        spans,
        splice: None,
    }
}

/// Parses `new_text` only around its change from `old_text` where that is safe, otherwise all of it. `doc` and
/// `spans` belong to `old_text`; a partial parse takes `doc` over, a full one leaves it as it was.
pub fn reparse_and_color(doc: &mut Doc, old_text: &str, spans: &[(Range<usize>, Style)], new_text: &str) -> Parsed {
    reparse_part_and_color(doc, old_text, spans, new_text).unwrap_or_else(|| parse_and_color(new_text))
}

/// The partial parse of `reparse_and_color`; `None` when all of `new_text` has to be parsed, with `doc` as it was.
pub fn reparse_part_and_color(
    doc: &mut Doc,
    old_text: &str,
    spans: &[(Range<usize>, Style)],
    new_text: &str,
) -> Option<Parsed> {
    if stays_plain(new_text) {
        return None;
    }
    let mut taken = std::mem::take(doc);
    match reparse(&mut taken, old_text, new_text) {
        Some(splice) => Some(Parsed {
            spans: with_definitions_in_window(rehighlight(spans, new_text, &taken, &splice), new_text, &taken, &splice),
            doc: taken,
            splice: Some(splice),
        }),
        None => {
            *doc = taken;
            None
        }
    }
}

/// The changed part of a text, as byte ranges in the old and the new text.
pub fn changed(old: &str, new: &str) -> (Range<usize>, Range<usize>) {
    let mut prefix = common_prefix(old.as_bytes(), new.as_bytes());
    while !(old.is_char_boundary(prefix) && new.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    let max_suffix = old.len().min(new.len()) - prefix;
    let mut suffix = common_suffix(old.as_bytes(), new.as_bytes(), max_suffix);
    while !(old.is_char_boundary(old.len() - suffix) && new.is_char_boundary(new.len() - suffix)) {
        suffix -= 1;
    }
    (prefix..old.len() - suffix, prefix..new.len() - suffix)
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
    mut spans: Vec<(Range<usize>, Style)>,
    text: &str,
    doc: &Doc,
    splice: &Splice,
) -> Vec<(Range<usize>, Style)> {
    let blocks: Vec<Block> = doc
        .blocks
        .iter()
        .rev()
        .take_while(|b| is_definition(b))
        .filter(|b| is_written_definition(b) && splice.window.contains(&b.range.start))
        .cloned()
        .collect();
    if !blocks.is_empty() {
        spans.extend(highlight(text, &Doc { blocks }));
        spans.sort_by_key(|(r, _)| r.start);
        // `rehighlight` may have colored the whole text.
        spans.dedup();
    }
    spans
}

/// The label of the `[^label]` at the start of `text`, if it is one that a definition can have.
fn footnote_label(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("[^")?;
    let end = rest.bytes().take(LONGEST_LABEL + 1).position(|c| c == b']')?;
    let label = &rest[..end];
    let allowed = !label.is_empty() && !label.bytes().any(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n' | 0));
    allowed.then_some(label)
}

/// Whether the `]` at `at` closes the label of a `[^label]:`.
fn ends_footnote_label(text: &str, at: usize) -> bool {
    let from = at.saturating_sub(LONGEST_LABEL + 2);
    let open = text.as_bytes()[from..at].iter().rposition(|&c| c == b'[');
    open.is_some_and(|open| footnote_label(&text[from + open..=at]).is_some())
}

/// Whether `text` may hold a link reference definition; `[^label]:` defines a footnote instead.
fn may_define_links(text: &str) -> bool {
    text.match_indices("]:").any(|(i, _)| !ends_footnote_label(text, i))
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

/// The blocks of `text`, a part of the document, with its footnotes recognized and its definitions where they stand.
/// comrak takes `[^label]` for a reference only with a definition in the same parse, and moves only the last definition
/// of a label behind the text, which is the stand-in here. A block left open swallows the stand-ins, which its neighbor
/// shows, so without a neighbor to compare with the result is `None`: the end of the document also ends blocks differently.
fn parse_window(text: &str, at_start: bool, at_end: bool) -> Option<Vec<Block>> {
    let definitions = stand_ins(text)?;
    if definitions.is_empty() {
        return Some(parse_part(text, at_start));
    }
    if at_end {
        return None;
    }
    let mut part = parse_part(&format!("{text}{definitions}"), at_start);
    part.retain(|b| b.range.start < text.len());
    Some(part)
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

/// Whether a reference may hide in plain text, as comrak still counts it where the reader gave up its positions.
fn may_hide_reference<'a>(blocks: impl IntoIterator<Item = &'a Block>) -> bool {
    let mut found = false;
    for block in blocks {
        walk_blocks(std::slice::from_ref(block), &mut |b| {
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
        BlockKind::FootnoteDefinition { name, .. } => !labels.insert(name.to_lowercase()),
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
    old_change: &Range<usize>,
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
fn replace_blocks(doc: &mut Doc, old: Range<usize>, part: Vec<Block>, old_change: &Range<usize>, delta: isize) {
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

/// Whether something anywhere in the text decides how the part around a change is read.
fn is_read_as_a_whole(old_text: &str, new_text: &str) -> bool {
    // A line starting with `%%` opens a comment that the next `%%` anywhere closes, across blank lines.
    let comment = |t: &str| t.lines().any(|l| l.trim_start().starts_with("%%"));
    // A `^[` that the change adds is found in the new part.
    inline_notes_may_be_limited(old_text)
        || formulas_are_limited(old_text)
        || formulas_are_limited(new_text)
        || comment(old_text)
        || comment(new_text)
}

/// Updates `doc`, parsed from `old_text`, to `new_text` by parsing only what the change can affect. `None` when
/// the whole document has to be parsed again; `doc` is unchanged then.
pub fn reparse(doc: &mut Doc, old_text: &str, new_text: &str) -> Option<Splice> {
    if is_read_as_a_whole(old_text, new_text) {
        return None;
    }
    // A link reference definition anywhere can turn brackets anywhere into a link.
    let has_definitions = may_define_links(old_text) || may_define_links(new_text);
    let (old_change, new_change) = changed(old_text, new_text);
    // A `---` line at the very start opens front matter that the next `---` line anywhere would close.
    if old_text.starts_with("---") || new_text.starts_with("---") {
        let closed_before = doc
            .blocks
            .first()
            .is_some_and(|b| matches!(b.kind, BlockKind::FrontMatter { .. }) && b.range.end < old_change.start);
        if !closed_before {
            return None;
        }
    }
    let delta = growth(new_text.len(), old_text.len());
    let blocks = &doc.blocks;
    let text_blocks = blocks.iter().rposition(|b| !is_definition(b)).map_or(0, |i| i + 1);
    let order = source_order(blocks, text_blocks)?;
    let n = order.len();
    // Unchanged blocks on either side, parsed again to show the change does not reach into them.
    let first = order.partition_point(|&i| blocks[i].range.end < old_change.start);
    let after_change = order.partition_point(|&i| blocks[i].range.start <= old_change.end);
    let mut before = first.checked_sub(1);
    let mut after = (after_change < n).then_some(after_change);
    let mut step = 1;
    loop {
        let start = before.map_or(0, |b| line_start(old_text, blocks[order[b]].range.start));
        let old_end = match after {
            Some(a) if a + 1 < n => line_start(old_text, blocks[order[a + 1]].range.start),
            _ => old_text.len(),
        };
        let new_end = shifted(old_end, delta);
        if new_end < start || new_end > new_text.len() || !new_text.is_char_boundary(new_end) {
            return None;
        }
        // Past half the document, parsing all of it is no slower.
        if new_end - start > new_text.len() / 2 && new_text.len() > 4096 {
            return None;
        }
        let (old_part, new_part) = (&old_text[start..old_end], &new_text[start..new_end]);
        if has_definitions && (holds_link_bracket(old_part) || holds_link_bracket(new_part)) {
            return None;
        }
        // The names comrak gives inline notes run through the whole document.
        if new_part.contains("^[") {
            return None;
        }
        let in_window = &order[before.unwrap_or(0)..after.map_or(n, |a| a + 1)];
        // A definition without a reference is not in the tree, but the references in it gave other notes their numbers.
        if looks_like_definitions(old_part) != in_window.iter().filter(|&&i| i >= text_blocks).count() {
            return None;
        }
        if may_hide_reference(in_window.iter().map(|&i| &blocks[i])) {
            return None;
        }
        let old_references = references(in_window.iter().map(|&i| &blocks[i]));
        let known: HashMap<&str, u32> = old_references.iter().copied().collect();
        let mut part = parse_window(new_part, start == 0, after.is_none())?;
        // A reference comrak would not know in the whole document, or that the window did not hold, changes numbers.
        if !number_references(&mut part, &known) || may_hide_reference(&part) {
            return None;
        }
        for b in &mut part {
            shift_block(b, growth(start, 0));
        }
        let head_holds = before.is_none_or(|b| part.first() == Some(&blocks[order[b]]));
        let tail_holds = after.is_none_or(|a| {
            let mut moved = blocks[order[a]].clone();
            shift_block(&mut moved, delta);
            part.last() == Some(&moved)
        });
        if head_holds && tail_holds {
            // Numbers and the order of the definitions stay as long as references and definitions do.
            if references(&part) != old_references || holds_nested_definition(&part) {
                return None;
            }
            let written = in_window.iter().map(|&i| &blocks[i]).filter(|b| is_definition(b));
            if !definitions_stay(written, &part, &old_change, delta) {
                return None;
            }
            part.retain(|b| !is_definition(b));
            let text = &blocks[..text_blocks];
            let old =
                text.partition_point(|b| b.range.start < start)..text.partition_point(|b| b.range.start < old_end);
            let new = old.start..old.start + part.len();
            replace_blocks(doc, old.clone(), part, &old_change, delta);
            return Some(Splice {
                old,
                new,
                delta,
                window: start..new_end,
                behind: new_change.end,
            });
        }
        if before.is_none() && after.is_none() {
            return None;
        }
        before = before.and_then(|b| b.checked_sub(step));
        after = after.map(|a| a + step).filter(|&a| a < n);
        step *= 2;
    }
}

fn moved(r: &mut Range<usize>, delta: isize) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn reparsed(old: &str, new: &str) -> (Doc, Parsed) {
        let mut doc = parse(old);
        let spans = highlight(old, &doc);
        let parsed = reparse_and_color(&mut doc, old, &spans, new);
        (doc, parsed)
    }

    #[test]
    fn a_partial_parse_takes_the_document_over() {
        let old = "Eins\n\nZwei\n\nDrei\n\nVier\n\nFünf\n\nSechs\n\nSieben\n";
        let new = old.replace("Vier", "Viertel");
        let new = new.as_str();
        let (doc, parsed) = reparsed(old, new);
        assert_eq!(parsed.splice.map(|s| (s.new, s.delta)), Some((2..5, 3)));
        assert_eq!(parsed.doc.blocks, parse(new).blocks);
        assert_eq!(parsed.spans, highlight(new, &parse(new)));
        assert!(doc.blocks.is_empty());
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
    fn a_document_over_the_limit_stays_plain_text() {
        let huge = "# Titel\n".repeat(3 << 20);
        let parsed = parse_and_color(&huge);
        assert!(parsed.doc.blocks.is_empty() && parsed.spans.is_empty());
        let mut doc = parse("# Titel\n");
        let spans = highlight("# Titel\n", &doc);
        assert!(reparse_part_and_color(&mut doc, "# Titel\n", &spans, &huge).is_none());
        assert!(
            reparse_and_color(&mut doc, "# Titel\n", &spans, &huge)
                .doc
                .blocks
                .is_empty()
        );
        let shrunk = reparse_and_color(&mut Doc::default(), &huge, &[], "# Titel\n");
        assert_eq!(shrunk.doc.blocks, parse("# Titel\n").blocks);
    }

    #[test]
    fn an_inline_footnote_near_the_change_takes_a_full_parse() {
        // Their numbers run through the whole document, and no `]:` gives them away.
        let (old, new) = ("Eins^[a]\n\nZwei\n", "Eins^[a]\n\nZwei^[b]\n");
        let (_, parsed) = reparsed(old, new);
        assert!(parsed.splice.is_none());
        assert_eq!(parsed.doc.blocks, parse(new).blocks);
    }

    #[test]
    fn a_full_parse_leaves_the_old_document_alone() {
        let (old, new) = ("Text[^1]\n\n[^1]: Fuß\n", "Text[^1]\n\n[^1]: Füße\n");
        let (doc, parsed) = reparsed(old, new);
        assert!(parsed.splice.is_none());
        assert_eq!(parsed.doc.blocks, parse(new).blocks);
        assert_eq!(doc.blocks, parse(old).blocks);
    }

    #[test]
    fn a_change_starts_and_ends_at_whole_letters() {
        assert_eq!(changed("x é y", "x è y"), (2..4, 2..4), "the letters begin alike");
        assert_eq!(changed("x éy", "x ĩy"), (2..4, 2..4), "the letters end alike");
        assert_eq!(changed("abXcd", "abYcd"), (2..3, 2..3));
        assert_eq!(changed("aa", "aaa"), (2..2, 2..3), "the common parts do not overlap");
    }

    fn partly(old: &str, new: &str) -> Option<Splice> {
        let (_, parsed) = reparsed(old, new);
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
        let splice = reparse(&mut parse(old), old, new).unwrap();
        let once = highlight(new, &doc);
        assert_eq!(with_definitions_in_window(once.clone(), new, &doc, &splice), once);
    }

    #[test]
    fn definitions_between_the_changed_blocks_keep_their_colors() {
        let old = "a[^1]\n\n[^1]: e\n\nb\n\nc\n\nd\n";
        let new = old.replace("b\n", "bb\n");
        let (_, parsed) = reparsed(old, &new);
        assert!(parsed.splice.is_some());
        assert_eq!(parsed.spans, highlight(&new, &parse(&new)));
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
    fn a_link_definition_needs_a_full_parse_where_brackets_are_near_the_change() {
        let old = "[r]: u\n\na\n\nb\n\nc\n\nd\n\ne\n\nf\n";
        assert!(partly(old, &old.replace("e\n", "ee\n")).is_some(), "no brackets near");
        let brackets = old.replace("e\n", "[r]\n");
        assert_eq!(partly(old, &brackets), None, "brackets come");
        assert_eq!(partly(&brackets, old), None, "brackets go");
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
        let (_, parsed) = reparsed(old, &new);
        assert!(matches!(parsed.doc.blocks[0].kind, BlockKind::FrontMatter { .. }));
    }

    #[test]
    fn a_link_definition_that_comes_or_goes_can_change_brackets_far_away() {
        let without = "[r]\n\na\n\nb\n\nc\n\nd\n\ne\n";
        let with = format!("{without}\n[r]: u\n");
        assert_eq!(partly(without, &with), None);
        assert_eq!(partly(&with, without), None);
    }
}
