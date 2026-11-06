//! What decides whether a part of a text can be read on its own. The answers lie all over the text, and asking at every
//! keystroke would take longer than the keystroke, so a document works them out once and a partial parse carries them on.

use std::ops::Range;
use std::sync::Arc;

use super::{Block, BlockKind, Doc, Inline, InlineKind, block_path, walk_blocks, walk_inlines};
use crate::text::{
    Byte, TextAt, TextPart, block_indent, indent_len, is_quote_prefix, label_key, line_end, line_start,
    next_line_start, previous_line_start,
};

/// The longest label a footnote can have, in bytes.
pub const LONGEST_LABEL: usize = 1000;

/// More would be read again at every keystroke, for far less than parsing all of the text saves.
const MOST_LINK_DEFINITIONS: usize = 64 << 10;

/// The estimated memory from which a document stays plain text in the editor.
pub(crate) const MOST_MEMORY: u64 = 4 << 30;

pub(crate) const HEAVIEST_BYTE: u64 = 800;

/// Roughly what a byte adds to the peak memory of tree, colors and layout, measured per syntax; every byte that can
/// start a node counts, as telling code from markup here would let a crafted document pass for code.
const fn memory_of(byte: u8) -> u16 {
    match byte {
        b'#' | b'|' | b'^' | b'@' => 800,
        b'\n' | b'\r' => 750,
        b'*' | b'_' | b'[' => 600,
        b'`' | b'<' | b'>' | b'$' | b'&' | b'"' | b'~' | b'=' => 400,
        b']' | b'(' | b')' | b'\\' | b'-' | b'+' | b':' | b'.' => 250,
        _ => 4,
    }
}

pub(crate) const MEMORY_OF: [u16; 256] = {
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

/// How often a text holds the marks that make the rest of it matter for a part of it, and what it takes in memory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkCounts {
    /// `^[`
    pub inline_notes: usize,
    /// `\(`, `\[` and `` $` ``
    pub formulas: usize,
    /// `%%`
    pub comments: usize,
    /// `]:`
    pub definitions: usize,
    /// What `memory_estimate` says.
    pub memory: u64,
}

impl MarkCounts {
    /// The counts for all of `text`.
    pub(crate) fn of(text: &[u8]) -> MarkCounts {
        let mut marks = MarkCounts {
            memory: text.last().map_or(0, |&b| u64::from(MEMORY_OF[usize::from(b)])),
            ..MarkCounts::default()
        };
        for pair in text.windows(2) {
            marks.memory += u64::from(MEMORY_OF[usize::from(pair[0])]);
            match (pair[0], pair[1]) {
                (b'^', b'[') => marks.inline_notes += 1,
                (b'\\', b'(' | b'[') | (b'$', b'`') => marks.formulas += 1,
                (b'%', b'%') => marks.comments += 1,
                (b']', b':') => marks.definitions += 1,
                _ => {}
            }
        }
        marks
    }

    /// The marks of `new` when they were these in `old` and only the given ranges differ. A mark can reach one byte over
    /// the edge of a change, and those on the edge count in both.
    #[must_use]
    pub fn after_change(self, old: &str, new: &str, old_change: &Range<Byte>, new_change: &Range<Byte>) -> MarkCounts {
        let around = |text: &str, change: &Range<Byte>| {
            let from = change.start.get().saturating_sub(1);
            let to = (change.end.get() + 1).min(text.len());
            MarkCounts::of(&text.as_bytes()[from..to])
        };
        let (gone, come) = (around(old, old_change), around(new, new_change));
        MarkCounts {
            inline_notes: self.inline_notes + come.inline_notes - gone.inline_notes,
            formulas: self.formulas + come.formulas - gone.formulas,
            comments: self.comments + come.comments - gone.comments,
            definitions: self.definitions + come.definitions - gone.definitions,
            memory: self.memory + come.memory - gone.memory,
        }
    }
}

/// Walks `text` from its end, resetting at every blank line: comrak looks for a closing mark within a paragraph, so a closer
/// in the next one closes nothing, however many there are.
pub(crate) fn from_the_end_by_paragraph(text: &str, mut at_byte: impl FnMut(usize), mut paragraph_ends: impl FnMut()) {
    let bytes = text.as_bytes();
    let mut line_is_blank = true;
    for at in (0..bytes.len()).rev() {
        match bytes[at] {
            // The line feed of the pair ended the line already.
            b'\r' if bytes.get(at + 1) == Some(&b'\n') => {}
            b'\n' | b'\r' => {
                if line_is_blank {
                    paragraph_ends();
                }
                line_is_blank = true;
            }
            b' ' | b'\t' => {}
            _ => line_is_blank = false,
        }
        at_byte(at);
    }
}

/// Formulas with no closing delimiter of their own behind them in their paragraph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unclosed {
    /// `\\(` and `\\[`
    pub latex: usize,
    /// `` $` ``
    pub code: usize,
}

pub(crate) fn unclosed_formulas(text: &str) -> Unclosed {
    let bytes = text.as_bytes();
    let closers = std::cell::Cell::new((0usize, 0usize, 0usize));
    let (mut latex, mut code) = (0usize, 0usize);
    from_the_end_by_paragraph(
        text,
        |at| {
            let (mut round, mut square, mut ticks) = closers.get();
            match bytes[at] {
                b'\\' => {
                    let mut count = |closers: &mut usize| match closers.checked_sub(1) {
                        Some(left) => *closers = left,
                        None => latex += 1,
                    };
                    match bytes.get(at + 1) {
                        Some(b')') => round += 1,
                        Some(b']') => square += 1,
                        Some(b'(') => count(&mut round),
                        Some(b'[') => count(&mut square),
                        _ => {}
                    }
                }
                b'$' if at > 0 && bytes[at - 1] == b'`' => ticks += 1,
                b'$' if bytes.get(at + 1) == Some(&b'`') => match ticks.checked_sub(1) {
                    Some(left) => ticks = left,
                    None => code += 1,
                },
                _ => {}
            }
            closers.set((round, square, ticks));
        },
        || closers.set((0, 0, 0)),
    );
    Unclosed { latex, code }
}

/// The stretch of `text` around `change` between two blank lines, where a mark can be closed from.
fn paragraph_around(text: &str, change: &Range<Byte>) -> Range<Byte> {
    let is_blank = |line: Byte| text.at(line..line_end(text, line)).trim_matches([' ', '\t']).is_empty();
    let mut start = line_start(text, change.start);
    while let Some(before) = previous_line_start(text, start) {
        if is_blank(before) {
            break;
        }
        start = before;
    }
    let mut end = line_end(text, change.end);
    while let Some(next) = next_line_start(text, end) {
        end = line_end(text, next);
        if is_blank(next) {
            break;
        }
    }
    start..end
}

/// `unclosed_formulas` of `new` when it said `unclosed` of `old` and only the given ranges differ: what a paragraph holds
/// stays within it.
pub(crate) fn unclosed_after_change(
    unclosed: Unclosed,
    old: &str,
    new: &str,
    old_change: &Range<Byte>,
    new_change: &Range<Byte>,
) -> Unclosed {
    let gone = unclosed_formulas(old.at(paragraph_around(old, old_change)));
    let come = unclosed_formulas(new.at(paragraph_around(new, new_change)));
    Unclosed {
        latex: unclosed.latex + come.latex - gone.latex,
        code: unclosed.code + come.code - gone.code,
    }
}

/// What a document knows about all of its text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextFacts {
    /// How often the marks stand in the text, and what it takes in memory.
    pub marks: MarkCounts,
    /// What `unclosed_formulas` says.
    pub unclosed_formulas: Unclosed,
    /// A line starts with a `%%` that nothing closes yet, which the next one anywhere would.
    pub loose_comment: bool,
    /// A `]:` stands anywhere that does not end the label of a footnote.
    pub may_define_links: bool,
    /// The lines that define links, for reading a part of the text with; `None` if there are too many.
    pub link_definitions: Option<Arc<str>>,
    /// What `reference_expansions` says of the blocks.
    pub reference_expansions: usize,
}

impl TextFacts {
    /// What `doc` knows about its text, read from the text and its blocks.
    pub fn of(doc: &Doc) -> TextFacts {
        let text = doc.text();
        let marks = MarkCounts::of(text.as_bytes());
        let by_position = BlocksByPosition::new(&doc.blocks);
        let reference_expansions = reference_expansions(&doc.blocks, TextPart::whole(text));
        TextFacts {
            marks,
            unclosed_formulas: unclosed_formulas(text),
            loose_comment: marks.comments > 0 && holds_loose_comment(&by_position, text),
            may_define_links: may_define_links(text),
            link_definitions: if marks.definitions > 0 {
                link_definitions_of(&by_position, text)
                    .filter(|definitions| expansions_fit(reference_expansions, definitions, text.len()))
            } else {
                Some("".into())
            },
            reference_expansions,
        }
    }
}

/// Whether `text` may hold a link reference definition; `[^label]:` defines a footnote instead.
pub(crate) fn may_define_links(text: &str) -> bool {
    text.match_indices("]:").any(|(at, _)| !ends_footnote_label(text, at))
}

/// The label of the `[^label]` at the start of `text`, if it is one that a definition can have.
pub(crate) fn footnote_label(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("[^")?;
    let end = rest.bytes().take(LONGEST_LABEL + 1).position(|c| c == b']')?;
    let label = &rest[..end];
    let allowed = !label.is_empty() && !label.bytes().any(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n' | 0));
    allowed.then_some(label)
}

/// Whether the `]` at `at` closes the label of a `[^label]:`.
pub fn ends_footnote_label(text: &str, at: usize) -> bool {
    let from = at.saturating_sub(LONGEST_LABEL + 2);
    let open = text.as_bytes()[from..at].iter().rposition(|&c| c == b'[');
    // A label ends at its first `]`, so an earlier `[^a]` says nothing about this one.
    open.is_some_and(|open| {
        footnote_label(&text[from + open..=at]).is_some_and(|label| from + open + 2 + label.len() == at)
    })
}

/// The top-level blocks of a part of a text in source order, to find the block at a position without going through all of
/// them.
pub(crate) struct BlocksByPosition<'a> {
    blocks: Vec<&'a Block>,
    /// How far the blocks up to each one reach: an inline note lies inside the block with its reference, which comes behind
    /// it here, so their ends do not sort as their starts do.
    reach: Vec<Byte>,
}

impl<'a> BlocksByPosition<'a> {
    pub(crate) fn new(blocks: impl IntoIterator<Item = &'a Block>) -> BlocksByPosition<'a> {
        let mut sorted: Vec<&Block> = blocks.into_iter().collect();
        sorted.sort_by_key(|b| b.range.start);
        let mut furthest = Byte::default();
        let reach = sorted
            .iter()
            .map(|b| {
                furthest = furthest.max(b.range.end);
                furthest
            })
            .collect();
        BlocksByPosition { blocks: sorted, reach }
    }

    /// The blocks around `at`, outermost first.
    fn path(&self, at: Byte) -> Vec<&'a Block> {
        let mut i = self.blocks.partition_point(|b| b.range.start <= at);
        while i > 0 && at <= self.reach[i - 1] {
            i -= 1;
            let top = self.blocks[i];
            if at <= top.range.end {
                let mut path = vec![top];
                path.extend(block_path(top.children(), at));
                return path;
            }
        }
        Vec::new()
    }

    fn at(&self, at: Byte) -> Option<&'a Block> {
        self.path(at).last().copied()
    }

    /// Whether no quote, list or other block with blocks inside stands around `at`.
    fn outside_of_containers(&self, at: Byte) -> bool {
        self.path(at).iter().all(|b| b.is_leaf())
    }

    /// How the text at `at` was read: what defines a link is left out of the tree, so it stands outside of every leaf or
    /// in front of the text of a paragraph, which a line below may have made a heading.
    fn reading(&self, at: Byte) -> Reading {
        let Some(leaf) = self.at(at) else {
            return Reading::LeftOut;
        };
        if !leaf.is_leaf() {
            return Reading::LeftOut;
        }
        if !matches!(leaf.kind, BlockKind::Paragraph | BlockKind::Heading { .. }) {
            return Reading::Text;
        }
        match leaf.inlines() {
            [] => Reading::LeftOut,
            [first, ..] if at < first.range.start => Reading::LeftOut,
            _ => Reading::Paragraph(leaf.range.start),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reading {
    Text,
    LeftOut,
    /// Holds the start of the paragraph. Behind definitions in front of its text, positions in a paragraph may be off.
    Paragraph(Byte),
}

/// How much the links that refer to a definition may expand to in all of a text of `len` bytes, by comrak's guard against
/// a small text growing huge; a link beyond that stays text.
fn reference_budget(len: usize) -> usize {
    len.max(100_000)
}

/// How much of `reference_budget` the blocks of `part` take: a link with its address in place takes none, and all that refer
/// to a definition end in a bracket.
pub(crate) fn reference_expansions(blocks: &[Block], part: TextPart) -> usize {
    let mut taken = 0;
    walk_blocks(blocks, &mut |b| {
        walk_inlines(b.inlines(), &mut |i| {
            if let InlineKind::Link { url, title } | InlineKind::Image { url, title } = &i.kind
                && let Some(end) = part.inside(i.range.end).filter(|end| end.get() > 0)
                && part.text.as_bytes().get(end.get() - 1) == Some(&b']')
            {
                taken += url.len() + title.len();
            }
        });
    });
    taken
}

/// Whether no link has run out of `reference_budget`, which the tree does not tell: comrak also spends it on a link inside a
/// link's text, which is why some is held back.
pub(crate) fn expansions_fit(taken: usize, definitions: &str, len: usize) -> bool {
    let longest = definitions.split("\n\n").map(str::len).max().unwrap_or(0);
    taken + taken / 8 + longest <= reference_budget(len)
}

/// Whether the `]` at `at` closes a label that opens its line, as a definition outside of any block does.
fn opens_its_line(text: &str, at: Byte) -> bool {
    let head = text.at(line_start(text, at)..at);
    block_indent(head).is_some_and(|indent| head[indent..].starts_with('['))
}

/// The lines of `part` that define links, with those behind each that lie outside of the text as well: an address or title
/// may stand on the next. `None` if the tree cannot tell, or a definition is not of the plain kind that means the same copied
/// elsewhere: one inside a list needs its indent to stay, and a label over two lines cannot be told from text by looking at
/// its line.
pub(crate) fn link_definitions(blocks: &BlocksByPosition, part: TextPart) -> Option<String> {
    let TextPart { text, start: offset } = part;
    let holds_definition = |line: Byte| {
        let rest = text.from(line);
        let prefix = rest.len() - rest.trim_start_matches([' ', '\t', '>']).len();
        let content = line_end(text, line);
        let at = offset + (line + prefix).get();
        !is_quote_prefix(text.at(line..content))
            && blocks.reading(at) == Reading::LeftOut
            && blocks.outside_of_containers(at)
    };
    let mut definitions = String::new();
    let mut done = Byte::default();
    for (at, _) in text.match_indices("]:") {
        let at = Byte::new(at);
        if at < done || ends_footnote_label(text, at.get()) {
            continue;
        }
        match blocks.reading(offset + at.get()) {
            Reading::Text => continue,
            Reading::Paragraph(start) => {
                let line = line_start(text, at);
                let head = text.at(line..at);
                if offset + line.get() <= start && head[indent_len(head)..].starts_with('[') {
                    return None;
                }
                continue;
            }
            Reading::LeftOut => {}
        }
        if !blocks.outside_of_containers(offset + at.get()) || !opens_its_line(text, at) {
            return None;
        }
        let start = line_start(text, at);
        let mut end = line_end(text, at);
        while let Some(next) = next_line_start(text, end) {
            if next >= text.end() || !holds_definition(next) {
                break;
            }
            end = line_end(text, next);
        }
        definitions.push_str(text.at(start..end));
        definitions.push_str("\n\n");
        done = end;
    }
    Some(definitions)
}

/// The definitions to read a part of `text` with. comrak goes by the first definition of a label, which reading them behind
/// the part would change if the part holds a later one, so a text with a label defined twice stays out of this.
fn link_definitions_of(blocks: &BlocksByPosition, text: &str) -> Option<Arc<str>> {
    let definitions = link_definitions(blocks, TextPart::whole(text))?;
    if definitions.len() > MOST_LINK_DEFINITIONS {
        return None;
    }
    let mut labels = std::collections::HashSet::new();
    for (at, _) in definitions.match_indices("]:") {
        let label = definitions[..at]
            .rfind('[')
            .map_or("", |open| &definitions[open + 1..at]);
        if !ends_footnote_label(&definitions, at) && !labels.insert(label_key(label)) {
            return None;
        }
    }
    Some(definitions.into())
}

/// Whether a line of `text`, the text of `blocks`, starts with a `%%` outside of code: a comment that runs on to the next
/// one anywhere, or text that the next one would turn into it.
pub(crate) fn holds_loose_comment(blocks: &BlocksByPosition, text: &str) -> bool {
    text.match_indices("%%").any(|(at, _)| {
        let at = Byte::new(at);
        let line = line_start(text, at);
        indent_len(text.at(line..at)) == at - line
            && !blocks
                .at(at)
                .is_some_and(|leaf| matches!(leaf.kind, BlockKind::CodeBlock { .. } | BlockKind::MathBlock { .. }))
    })
}

/// Whether the blocks hold nothing but what reads the same with the definitions of a text behind it as within the text:
/// embedded HTML, comments and notes have rules of their own that reach across blocks.
pub(crate) fn reads_alone(blocks: &[Block]) -> bool {
    fn plain_inlines(inlines: &[Inline]) -> bool {
        inlines.iter().all(|i| {
            !matches!(
                i.kind,
                InlineKind::Html(_) | InlineKind::FootnoteRef { .. } | InlineKind::Comment
            ) && plain_inlines(&i.children)
        })
    }
    blocks.iter().all(|b| {
        let plain_kind = matches!(
            b.kind,
            BlockKind::Paragraph
                | BlockKind::Heading { .. }
                | BlockKind::BlockQuote
                | BlockKind::List { .. }
                | BlockKind::Item { .. }
                | BlockKind::CodeBlock { .. }
                | BlockKind::ThematicBreak
                | BlockKind::Table { .. }
                | BlockKind::TableRow { .. }
                | BlockKind::TableCell
                | BlockKind::MathBlock { .. }
        );
        plain_kind && plain_inlines(b.inlines()) && reads_alone(b.children())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::parse;
    use crate::text::changed;

    fn facts(text: &str) -> TextFacts {
        TextFacts::of(&parse(text))
    }

    #[test]
    fn marks_are_counted_and_follow_a_change() {
        let old = "^[a] \\(b\\) $`c`$ %%d%% [e]: f\n";
        assert_eq!(
            MarkCounts::of(old.as_bytes()),
            MarkCounts {
                inline_notes: 1,
                formulas: 2,
                comments: 2,
                definitions: 1,
                memory: memory_estimate(old),
            }
        );
        for (from, to) in [
            ("^[a]", "a]"),
            ("a]", "^[a]"),
            ("[e]:", "[e] :"),
            ("d%%", "d%"),
            ("d%%", "d%%%"),
            (" ", ""),
            ("\\(b", "\\(\\[b"),
            ("$`c", "$c"),
        ] {
            let new = old.replacen(from, to, 1);
            let (old_change, new_change) = changed(old, &new);
            assert_eq!(
                MarkCounts::of(old.as_bytes()).after_change(old, &new, &old_change, &new_change),
                MarkCounts::of(new.as_bytes()),
                "{from:?} to {to:?}"
            );
        }
    }

    #[test]
    fn unclosed_formulas_follow_a_change_of_their_paragraph() {
        let old = "\\( a\n\nb \\) c \\(\nd\n\n$` e\n\nf\r\n\r\n\\[ g\n";
        for (from, to) in [
            ("b ", "x \\( "),
            ("\\) c", "c"),
            ("\nd\n\n", "\nd\n"),
            ("\n\n$`", "$`"),
            ("e\n\n", "e `$\n\n"),
            ("f\r\n\r\n", "f\r\n"),
            ("\\[ g", "\\[ g \\]"),
            ("\\( a", "a"),
        ] {
            let new = old.replacen(from, to, 1);
            let (old_change, new_change) = changed(old, &new);
            assert_eq!(
                unclosed_after_change(unclosed_formulas(old), old, &new, &old_change, &new_change),
                unclosed_formulas(&new),
                "{from:?} to {to:?}"
            );
        }
    }

    #[test]
    fn a_change_at_the_very_ends_of_a_text_keeps_the_count() {
        for (old, new) in [("", "%%"), ("%%", ""), ("%", "%%"), ("a", "]:a"), ("a", "a]:")] {
            let (old_change, new_change) = changed(old, new);
            assert_eq!(
                MarkCounts::of(old.as_bytes()).after_change(old, new, &old_change, &new_change),
                MarkCounts::of(new.as_bytes()),
                "{old:?} to {new:?}"
            );
        }
    }

    #[test]
    fn a_comment_at_the_start_of_a_line_counts_unless_it_is_code() {
        for (text, loose, why) in [
            ("a\n\nb\n", false, "none"),
            ("a\n\n%%\nb\n%%\n", true, "closed, but what opens it may go"),
            ("a\n\n%%b%% c\n", true, "closed on its line"),
            ("```mermaid\n%%{init}%%\ngraph\n```\n", false, "in code"),
            ("a %% b\n", false, "not at the start of a line"),
            ("a\n\n%%b\n", true, "a comment up to the end of the text"),
        ] {
            assert_eq!(facts(text).loose_comment, loose, "{why}: {text:?}");
        }
    }

    fn definitions(text: &str) -> String {
        facts(text).link_definitions.expect("few definitions").to_string()
    }

    #[test]
    fn what_defines_a_link_is_told_from_what_only_looks_like_it() {
        assert_eq!(definitions("a\n\n[r]: u\n\nb\n"), "[r]: u\n\n");
        assert_eq!(definitions("[r]: u\n[s]: v\n\nb\n"), "[r]: u\n[s]: v\n\n");
        assert_eq!(
            definitions("[r]: u\nTitel\n=====\n\nb\n"),
            "[r]: u\n\n",
            "in front of a heading's text"
        );
        assert_eq!(definitions("# Titel a[0]: x\n"), "");
        assert_eq!(definitions("[r]:\n  u\n  \"t\"\n\nb\n"), "[r]:\n  u\n  \"t\"\n\n");
        assert_eq!(definitions("```\na[0]: x\n```\n"), "");
        assert_eq!(definitions("Text a[0]: x\n"), "");
        assert_eq!(definitions("<!-- [r]: u -->\n"), "");
        assert_eq!(definitions("a[^1]\n\n[^1]: Fuß\n"), "");
        assert_eq!(
            definitions("[r]: u\nText\n"),
            "[r]: u\n\n",
            "in front of a paragraph's text"
        );
    }

    #[test]
    fn too_many_definitions_are_not_worth_reading_along() {
        let many: String = (0..10_000)
            .map(|n| format!("[r{n}]: http://example.com/{n}\n"))
            .collect();
        assert!(facts(&many).link_definitions.is_none());
    }

    #[test]
    fn a_definition_that_may_mean_something_else_elsewhere_cannot_be_read_along() {
        for (text, why) in [
            ("Begriff\n\n: [r]: u\n", "needs its term"),
            ("> - [r]: u\n", "inside a quote and a list"),
            ("> [r]: u\n", "inside a quote"),
            ("1.  x\n\n    [a]: u\n", "reads as code without its list"),
            ("[f\nb]: u\n", "a label over two lines"),
            ("[f\r\nb]: u\r\n", "a label over two lines with Windows line ends"),
        ] {
            assert!(facts(text).link_definitions.is_none(), "{why}");
        }
    }

    #[test]
    fn a_position_between_many_blocks_is_found_without_going_through_them() {
        let text: String = (0..50_000).map(|n| format!("a {n}\n\n[l{n}]: u\n\n")).collect();
        let started = std::time::Instant::now();
        assert!(facts(&text).link_definitions.is_none(), "too many");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_position_inside_an_earlier_block_is_found_behind_later_ones() {
        let doc = parse("a^[b] c\n\nd\n");
        let blocks = BlocksByPosition::new(&doc.blocks);
        assert_eq!(
            blocks.path(Byte::new(6)).len(),
            1,
            "behind the note in the block that holds it"
        );
        assert!(blocks.path(Byte::new(8)).is_empty(), "between the blocks");
    }

    #[test]
    fn a_footnote_label_ends_at_its_first_bracket() {
        assert!(ends_footnote_label("[^a]: x", 3));
        let far = "[^a] und noch etwas]: x";
        assert!(!ends_footnote_label(far, far.find("]:").unwrap()));
        assert!(may_define_links(far));
    }

    #[test]
    fn the_paragraph_around_a_change_reaches_over_windows_line_ends() {
        let text = "a\r\nb\r\n\r\nc\r\nd\r\n";
        let at = |needle: &str| Byte::new(text.find(needle).unwrap());
        let around = |needle: &str| text.at(paragraph_around(text, &(at(needle)..at(needle))));
        assert!(around("d").starts_with("c\r\nd"), "{:?}", around("d"));
        assert!(around("b").starts_with("a\r\nb"), "{:?}", around("b"));
        assert!(!around("b").contains('c'));
    }

    #[test]
    fn windows_line_ends_do_not_end_a_paragraph_for_a_closing_mark() {
        let text = "\\\\(a\r\nb\\\\)\r\n";
        assert_eq!(unclosed_formulas(text), Unclosed::default());
        assert_eq!(
            unclosed_formulas("\\\\(a\r\n\r\nb\\\\)\r\n"),
            Unclosed { latex: 1, code: 0 },
            "a blank line does"
        );
    }

    #[test]
    fn a_label_defined_twice_cannot_be_read_along() {
        assert!(facts("[r]: u\n\na\n\n[R]: v\n").link_definitions.is_none());
        assert!(facts("[r]: u\n\na\n\n[s]: v\n").link_definitions.is_some());
    }
}
