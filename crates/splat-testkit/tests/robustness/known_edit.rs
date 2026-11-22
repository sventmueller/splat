//! The partial parse told where an edit lies gives what the one that compares the whole old and new text gives.

use std::ops::Range;

use splat_markdown::incremental::{Parsed, parse_and_color, reparse_edit_and_color, reparse_part_and_color};
use splat_markdown::text::{bytes, offsets};
use splat_testkit::{Reporting, Rng, rounds};

use super::common::{PIECES, document, limited, ordinary_document};

/// `range` of `text` widened by up to `most` bytes on either side, on character boundaries.
fn widened(rng: &mut Rng, text: &str, range: &Range<usize>, most: usize) -> Range<usize> {
    let mut start = range.start.saturating_sub(rng.below(most + 1));
    while !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (range.end + rng.below(most + 1)).min(text.len());
    while !text.is_char_boundary(end) {
        end += 1;
    }
    start..end
}

/// Both partial parses of `parsed` after replacing `replaced` with `with`; the next parse and whether it was partial.
fn both(parsed: Parsed, replaced: Range<usize>, with: &str, what: &str) -> (Parsed, bool) {
    let old = parsed.doc.text().to_owned();
    let new = format!("{}{with}{}", &old[..replaced.start], &old[replaced.end..]);
    let version = parsed.doc.version();
    let by_text = reparse_part_and_color(parsed.doc.copied(), &parsed.spans, &new);
    let by_edit = reparse_edit_and_color(parsed.doc, &parsed.spans, bytes(replaced), with);
    match (by_text, by_edit) {
        (Ok(by_text), Ok(by_edit)) => {
            assert_eq!(by_edit.doc.text(), new, "{what}");
            assert_eq!(by_edit.doc.blocks(), by_text.doc.blocks(), "{what}");
            assert_eq!(by_edit.spans, by_text.spans, "colors: {what}");
            assert_eq!(by_edit.splice, by_text.splice, "splice: {what}");
            assert_eq!(by_edit.kept_colors, by_text.kept_colors, "kept colors: {what}");
            assert_eq!(by_edit.doc.facts(), by_text.doc.facts(), "facts: {what}");
            (by_edit, true)
        }
        (Err(_), Err(doc)) => {
            assert_eq!((doc.version(), doc.text()), (version, old.as_str()), "{what}");
            (parse_and_color(&new), false)
        }
        (by_text, by_edit) => panic!(
            "partial by text: {}, by edit: {}; {what}",
            by_text.is_ok(),
            by_edit.is_ok()
        ),
    }
}

/// Edit after edit, some told as a wider range than what changed, so the edit's ends are trimmed as the comparison
/// would find them.
#[test]
fn reparsing_a_known_edit_matches_reparsing_the_whole_text() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_ed17_0001);
    let (mut steps, mut partial) = (0, 0);
    for round in 0..rounds(1000) {
        let mut text: String = (0..rng.below(6) + 2)
            .map(|_| {
                if round % 2 == 0 {
                    document(&mut rng)
                } else {
                    ordinary_document(&mut rng)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        if round % 7 == 0 {
            text = text.replace('\n', "\r\n");
        }
        let _input = Reporting(&text);
        let mut parsed = parse_and_color(&text);
        for _ in 0..rng.below(12) + 1 {
            let text = parsed.doc.text().to_owned();
            let range = offsets(&rng.range(&text));
            let piece = if rng.below(4) == 0 { "" } else { *rng.pick(PIECES) };
            let (replaced, with) = if rng.below(3) == 0 {
                let wide = widened(&mut rng, &text, &range, 8);
                let with = format!(
                    "{}{piece}{}",
                    &text[wide.start..range.start],
                    &text[range.end..wide.end]
                );
                (wide, with)
            } else {
                (range, piece.to_owned())
            };
            let what = format!("{replaced:?} -> {with:?} of {text:?}");
            let (next, in_part) = both(parsed, replaced, &with, &what);
            steps += 1;
            partial += usize::from(in_part);
            parsed = next;
        }
    }
    assert!(
        partial > steps / 4,
        "only {partial} of {steps} edits were parsed in part"
    );
}
