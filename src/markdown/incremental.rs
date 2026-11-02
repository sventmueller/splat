//! Re-parsing only the top-level blocks around an edit. Markdown is mostly local: a blank line between top-level
//! blocks ends everything except front matter, footnotes and link references, which are checked for.

use std::ops::Range;

use super::doc::{Block, BlockKind, Doc, Inline, formulas_are_limited, inline_notes_may_be_limited, parse, parse_part};
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
}

/// The parsed document and its source colors.
pub struct Parsed {
    pub doc: Doc,
    pub spans: Vec<(Range<usize>, Style)>,
    /// Which top-level blocks changed, when only those were parsed again.
    pub splice: Option<Splice>,
}

/// From this size on a document stays plain text in the editor: its tree and layout would take gigabytes.
pub const PLAIN_FROM: usize = 64 << 20;

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
    if text.len() >= PLAIN_FROM {
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
    if new_text.len() >= PLAIN_FROM {
        return None;
    }
    let mut taken = std::mem::take(doc);
    match reparse(&mut taken, old_text, new_text) {
        Some(splice) => Some(Parsed {
            spans: rehighlight(spans, new_text, &taken, &splice),
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

/// comrak moves footnote definitions to the end of the tree, out of text order, and numbers references across the
/// whole document; such documents are parsed whole.
fn has_footnotes(doc: &Doc) -> bool {
    doc.blocks
        .iter()
        .any(|b| matches!(b.kind, BlockKind::FootnoteDefinition { .. }))
}

/// Whether a footnote could begin in `text`; without a definition, `[^a-z]` is plain text.
fn may_begin_footnote(text: &str) -> bool {
    text.contains("[^") || text.contains("^[")
}

/// Updates `doc`, parsed from `old_text`, to `new_text` by parsing only what the change can affect. `None` when
/// the whole document has to be parsed again; `doc` is unchanged then.
pub fn reparse(doc: &mut Doc, old_text: &str, new_text: &str) -> Option<Splice> {
    // A `^[` that the change adds is found below, in the new part.
    if has_footnotes(doc) || inline_notes_may_be_limited(old_text) {
        return None;
    }
    if formulas_are_limited(old_text) || formulas_are_limited(new_text) {
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
    let delta = growth(new_text.len(), old_text.len());
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
        let new_end = shifted(old_end, delta);
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
        if may_begin_footnote(&new_text[start..new_end]) {
            return None;
        }
        let mut part = parse_part(&new_text[start..new_end], start == 0);
        for b in &mut part {
            shift_block(b, growth(start, 0));
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
    fn a_document_from_the_size_limit_on_stays_plain_text() {
        let huge = "# Titel\n".repeat(PLAIN_FROM / 8 + 1);
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
    fn inline_footnotes_always_take_a_full_parse() {
        // Their numbers run through the whole document, and no `]:` gives them away.
        let (old, new) = ("Eins^[a]\n\nZwei\n", "Eins^[a]\n\nZwei^[b]\n");
        let (_, parsed) = reparsed(old, new);
        assert!(parsed.splice.is_none());
        assert_eq!(parsed.doc.blocks, parse(new).blocks);
    }

    #[test]
    fn a_full_parse_leaves_the_old_document_alone() {
        let (old, new) = ("Text[^1]\n\n[^1]: Fuß\n", "Texte[^1]\n\n[^1]: Fuß\n");
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
        let gone = "a\n\nb [^a-z]\n\nc\n\nd\n";
        assert!(partly(gone, "a\n\nb\n\nc\n\nd\n").is_some(), "it goes as plain text");
    }

    #[test]
    fn a_footnote_takes_a_full_parse_however_far_from_the_change() {
        let old = "a[^1]\n\nb\n\nc\n\nd\n\ne\n\nf\n\n[^1]: g\n";
        assert_eq!(partly(old, &old.replace("d\n", "dd\n")), None);
        let inline = "a^[g]\n\nb\n\nc\n\nd\n\ne\n\nf\n";
        assert_eq!(partly(inline, &inline.replace("d\n", "dd\n")), None);
    }

    #[test]
    fn a_definition_gives_a_stray_reference_far_away_its_meaning() {
        let stray = "[^1]\n\na\n\nb\n\nc\n\nd\n\ne\n";
        assert_eq!(partly(stray, &stray.replace("e\n", "[^1]: f\n")), None);
        let unused = "[^1]: f\n\na\n\nb\n\nc\n\nd\n\ne\n";
        assert_eq!(partly(unused, &unused.replace("e\n", "[^1]\n")), None);
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
