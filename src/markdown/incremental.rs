//! Re-parsing only the top-level blocks around an edit. Markdown is mostly local: a blank line between top-level
//! blocks ends everything except front matter, footnotes and link references, which are checked for.

use std::ops::Range;

use super::doc::{Block, BlockKind, Doc, Inline, parse, parse_part};
use super::highlight::{Style, highlight, rehighlight};
use super::text::{common_prefix, common_suffix};

/// What an edit replaced: the top-level block indexes before and after, and the shift of everything behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    pub old: Range<usize>,
    pub new: Range<usize>,
    pub delta: isize,
    /// The re-parsed part of the new text; offsets from its end on are shifted by `delta`.
    pub window: Range<usize>,
}

/// The parsed document and its source colors.
pub struct Parsed {
    pub doc: Doc,
    pub spans: Vec<(Range<usize>, Style)>,
    /// Which top-level blocks changed, when only those were parsed again.
    pub splice: Option<Splice>,
}

pub fn parse_and_color(text: &str) -> Parsed {
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
    let mut taken = std::mem::take(doc);
    match reparse(&mut taken, old_text, new_text) {
        Some(splice) => Parsed {
            spans: rehighlight(spans, new_text, &taken, &splice),
            doc: taken,
            splice: Some(splice),
        },
        None => {
            *doc = taken;
            parse_and_color(new_text)
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

/// comrak moves footnote definitions to the end of the tree, out of text order, and numbers references across the
/// whole document; such documents are parsed whole.
fn has_footnotes(text: &str) -> bool {
    text.contains("[^") || text.contains("^[")
}

/// Updates `doc`, parsed from `old_text`, to `new_text` by parsing only what the change can affect. `None` when
/// the whole document has to be parsed again; `doc` is unchanged then.
pub fn reparse(doc: &mut Doc, old_text: &str, new_text: &str) -> Option<Splice> {
    if has_footnotes(old_text) || has_footnotes(new_text) {
        return None;
    }
    // A line starting with `%%` opens a comment that the next `%%` anywhere closes, across blank lines.
    let comment = |t: &str| t.lines().any(|l| l.trim_start().starts_with("%%"));
    if comment(old_text) || comment(new_text) {
        return None;
    }
    // A link reference definition anywhere can turn brackets anywhere into a link.
    let has_definitions = old_text.contains("]:") || new_text.contains("]:");
    let (old_change, _) = changed(old_text, new_text);
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
    let delta = new_text.len() as isize - old_text.len() as isize;
    let blocks = &doc.blocks;
    let n = blocks.len();
    // Unchanged blocks on either side, parsed again to show the change does not reach into them.
    let first = blocks.partition_point(|b| b.range.end < old_change.start);
    let after_change = blocks.partition_point(|b| b.range.start <= old_change.end);
    let mut before = first.checked_sub(1);
    let mut after = (after_change < n).then_some(after_change);
    let mut step = 1;
    loop {
        let start = before.map_or(0, |b| line_start(old_text, blocks[b].range.start));
        let old_end = match after {
            Some(a) if a + 1 < n => line_start(old_text, blocks[a + 1].range.start),
            _ => old_text.len(),
        };
        let new_end = (old_end as isize + delta) as usize;
        if new_end < start || new_end > new_text.len() || !new_text.is_char_boundary(new_end) {
            return None;
        }
        // Past half the document, parsing all of it is no slower.
        if new_end - start > new_text.len() / 2 && new_text.len() > 4096 {
            return None;
        }
        if has_definitions && (old_text[start..old_end].contains('[') || new_text[start..new_end].contains('[')) {
            return None;
        }
        let mut part = parse_part(&new_text[start..new_end], start == 0);
        for b in &mut part {
            shift_block(b, start as isize);
        }
        let head_holds = before.is_none_or(|b| part.first() == Some(&blocks[b]));
        let tail_holds = after.is_none_or(|a| {
            let mut moved = blocks[a].clone();
            shift_block(&mut moved, delta);
            part.last() == Some(&moved)
        });
        if head_holds && tail_holds {
            let old = before.unwrap_or(0)..after.map_or(n, |a| a + 1);
            let new = old.start..old.start + part.len();
            let tail = doc.blocks.split_off(old.end);
            doc.blocks.truncate(old.start);
            doc.blocks.extend(part);
            let resume = doc.blocks.len();
            doc.blocks.extend(tail);
            for b in &mut doc.blocks[resume..] {
                shift_block(b, delta);
            }
            return Some(Splice {
                old,
                new,
                delta,
                window: start..new_end,
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

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

fn moved(r: &mut Range<usize>, delta: isize) {
    *r = (r.start as isize + delta) as usize..(r.end as isize + delta) as usize;
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
                f.sign = (f.sign as isize + delta) as usize;
            }
        }
        BlockKind::Heading { attrs: Some(attrs), .. } => moved(attrs, delta),
        BlockKind::Item { task, marker } => {
            moved(marker, delta);
            if let Some(t) = task {
                t.symbol = (t.symbol as isize + delta) as usize;
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
    for i in b.inlines.iter_mut() {
        shift_inline(i, delta);
    }
    for c in b.children.iter_mut() {
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
    fn a_full_parse_leaves_the_old_document_alone() {
        let (old, new) = ("Text[^1]\n\n[^1]: Fuß\n", "Texte[^1]\n\n[^1]: Fuß\n");
        let (doc, parsed) = reparsed(old, new);
        assert!(parsed.splice.is_none());
        assert_eq!(parsed.doc.blocks, parse(new).blocks);
        assert_eq!(doc.blocks, parse(old).blocks);
    }
}
