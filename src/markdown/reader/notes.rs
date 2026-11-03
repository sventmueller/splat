use std::collections::HashMap;
use std::ops::Range;

use super::Reader;
use crate::markdown::doc::{Block, BlockKind, Inline, InlineKind, walk_blocks, walk_inlines};
use crate::markdown::incremental::shift_block;
use crate::markdown::text::{Byte, LineStarts, growth, offsets, shifted};

/// Puts the inline notes among `blocks`: comrak lists footnotes in the order of their references, and an inline one keeps
/// its place among them.
pub(super) fn place_inline_notes<'a>(
    text: &str,
    blocks: &mut Vec<Block>,
    inline_notes: Vec<(usize, String, comrak::nodes::Node<'a>)>,
) {
    if inline_notes.is_empty() {
        return;
    }
    let mut references = HashMap::new();
    walk_blocks(blocks, &mut |b| {
        walk_inlines(b.inlines(), &mut |i| {
            if let InlineKind::FootnoteRef { name, ix } = &i.kind {
                references.insert(&**name, (i.range.clone(), *ix));
            }
        })
    });
    let notes: Vec<(usize, Option<Block>)> = inline_notes
        .into_iter()
        .map(|(at, name, node)| {
            (
                at,
                inline_note(text, references.get(name.as_str()).cloned(), &name, node),
            )
        })
        .collect();
    let mut rest = std::mem::take(blocks).into_iter();
    for (at, note) in notes {
        if let Some(note) = note {
            blocks.extend(rest.by_ref().take(at.saturating_sub(blocks.len())));
            blocks.push(note);
        }
    }
    blocks.extend(rest);
}

/// comrak's name for the footnote an inline `^[note]` stands for, followed by a number.
pub(super) const INLINE_NOTE: &str = "__inline_";

pub(super) fn is_inline_note(name: &str) -> bool {
    name.strip_prefix(INLINE_NOTE)
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// The labels of written footnote definitions that read like comrak's names; those are ordinary footnotes.
pub(super) fn written_like_inline_notes(text: &str) -> std::collections::HashSet<&str> {
    let opening = format!("[^{INLINE_NOTE}");
    text.match_indices(&opening)
        .filter_map(|(at, _)| {
            let label = &text[at + 2..];
            let end = label.find(']')?;
            label[end..].starts_with("]:").then(|| &label[..end])
        })
        .collect()
}

/// The footnote behind an inline `^[note]`. comrak gives its content positions relative to the note text, so it is
/// converted from that text and moved to where it stands; the definition covers the reference.
fn inline_note<'a>(
    text: &str,
    reference: Option<(Range<Byte>, u32)>,
    name: &str,
    node: comrak::nodes::Node<'a>,
) -> Option<Block> {
    let (at, number) = reference?;
    let content = (at.start + 2).min(at.end)..shifted(at.end, -1).max((at.start + 2).min(at.end));
    let note = text.get(offsets(&content))?;
    // Over several lines comrak counts positions from quote and list prefixes it removed, so such a note is kept
    // as plain text.
    if note.contains('\n') {
        let text = Inline {
            kind: InlineKind::Text(note.into()),
            range: content.clone(),
            children: Box::default(),
        };
        let paragraph = Block::leaf(BlockKind::Paragraph, content.clone(), [text]);
        return Some(Block::container(
            BlockKind::FootnoteDefinition {
                name: name.into(),
                label: at.start..at.start,
                number: Some(number),
            },
            at,
            [paragraph],
        ));
    }
    let lines = LineStarts::new(note);
    let cx = Reader {
        text: note,
        lines: &lines,
        depth: std::cell::Cell::new(0),
        escapes: Default::default(),
    };
    let children: Vec<Block> = node
        .children()
        .filter_map(|c| cx.block(c))
        .map(|mut b| {
            shift_block(&mut b, growth(content.start, Byte::default()));
            b
        })
        .collect();
    Some(Block::container(
        BlockKind::FootnoteDefinition {
            name: name.into(),
            label: at.start..at.start,
            number: Some(number),
        },
        at,
        children,
    ))
}

#[cfg(test)]
mod tests {
    use super::super::parse;
    use super::super::tests::slice;
    use crate::markdown::doc::{Block, BlockKind, InlineKind, plain_text, walk_blocks};
    use crate::markdown::text::TextAt;

    #[test]
    fn inline_notes_stand_where_the_order_of_references_puts_them() {
        let text: String = (0..50).map(|n| format!("Absatz {n}^[Notiz {n}]\n\n")).collect();
        let doc = parse(&text);
        let notes: Vec<&Block> = doc
            .blocks
            .iter()
            .filter(|b| matches!(b.kind, BlockKind::FootnoteDefinition { .. }))
            .collect();
        assert_eq!(notes.len(), 50);
        for (n, note) in notes.iter().enumerate() {
            assert_eq!(text.at(note.range.clone()), format!("^[Notiz {n}]"));
        }
    }

    #[test]
    fn a_written_footnote_named_like_an_inline_note_keeps_its_text() {
        for name in ["__inline_x", "__inline_1"] {
            let doc = parse(&format!("Text[^{name}]\n\n[^{name}]: Definition\n"));
            let definition = doc.blocks.last().expect("a definition");
            assert!(
                matches!(definition.kind, BlockKind::FootnoteDefinition { .. }),
                "{:?}",
                definition.kind
            );
            let paragraph = definition.children().first().expect("its text");
            assert_eq!(plain_text(paragraph.inlines()), "Definition", "{name}");
        }
    }

    #[test]
    fn inline_notes_keep_their_place_among_other_notes() {
        let t = "a^[x] b[^r] c^[y]\n\n[^r]: z\n";
        let d = parse(t);
        let notes: Vec<&str> = d
            .blocks
            .iter()
            .filter(|b| matches!(b.kind, BlockKind::FootnoteDefinition { .. }))
            .map(|b| slice(t, &b.range))
            .collect();
        assert_eq!(notes, ["^[x]", "[^r]: z", "^[y]"]);
    }

    #[test]
    fn a_note_over_lines_of_a_quote_or_list_ends_at_its_bracket_and_what_follows_stays_behind_it() {
        for t in ["> a^[b [c]\n> d] e f", "- a^[b [c]\n  d] e f"] {
            let d = parse(t);
            let mut seen = Vec::new();
            walk_blocks(&d.blocks, &mut |b| {
                if !matches!(b.kind, BlockKind::Paragraph) || b.inlines().len() < 2 {
                    return;
                }
                seen = b.inlines().iter().map(|i| i.range.clone()).collect();
                let note = b
                    .inlines()
                    .iter()
                    .find(|i| matches!(i.kind, InlineKind::FootnoteRef { .. }))
                    .unwrap();
                assert!(
                    slice(t, &note.range).starts_with("^[b [c]") && slice(t, &note.range).ends_with("d]"),
                    "{t}"
                );
            });
            assert!(seen.len() >= 3, "{t}: {seen:?}");
            assert!(seen.windows(2).all(|w| w[0].end <= w[1].start), "{t}: {seen:?}");
        }
    }

    #[test]
    fn the_text_of_a_note_far_into_a_paragraph_stops_before_its_bracket() {
        let t = "abcdefgh^[b] c";
        let d = parse(t);
        let note = d
            .blocks
            .iter()
            .find(|b| matches!(b.kind, BlockKind::FootnoteDefinition { .. }))
            .unwrap();
        assert_eq!(slice(t, &note.children()[0].inlines()[0].range), "b");
    }
}
