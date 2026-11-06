//! Whether an edit made in the preview does what the person sees: the text they typed appears where they typed it, and
//! nothing else changes. Markdown has too many ways of turning text into syntax for rules alone to prevent that, so the
//! changed part of the document is parsed again and compared.

use std::ops::Range;

use splat_markdown::commands::Edit;
use splat_markdown::doc::Doc;
use splat_markdown::reader::parse_part;
use splat_markdown::text::{Byte, TextAt, line_end, line_start};

use super::layout::{Atom, Layout, NoImages, ParaKind, Run, RunMap, layout};
use super::map::{Index, Shown};

/// A larger stretch is not read twice per keystroke; such an edit goes unchecked.
const MAX_CHECKED: usize = 256 << 10;

/// What an edit should change in what the preview shows.
pub(super) struct Expected<'a> {
    pub typed: &'a str,
    /// Visible characters it removes, where those can be told.
    pub removed: Option<usize>,
    /// The paragraphs stay as they are: no new ones, none gone, none of another kind.
    pub same_paragraphs: bool,
}

/// What a stretch of Markdown shows: its visible characters and its kinds of paragraph.
#[derive(Debug, PartialEq)]
struct Shows {
    chars: Vec<char>,
    paragraphs: Vec<(ParaKind, bool)>,
}

fn shows(markdown: &str, at_start: bool) -> Shows {
    let doc = Doc::new(markdown.into(), parse_part(markdown, at_start));
    let laid_out = layout(&doc, &NoImages, None);
    let mut chars = Vec::new();
    let mut paragraphs = Vec::new();
    for para in laid_out.blocks.iter().flat_map(|b| &b.paras) {
        paragraphs.push((para.style.kind.clone(), para.style.marker.is_some()));
        for run in &para.runs {
            // List numbers follow from the position of an item.
            if !matches!(run.map, RunMap::Atom { atom: Atom::Marker, .. }) {
                chars.extend(run.text.chars().filter(|c| !c.is_whitespace()));
            }
        }
    }
    Shows { chars, paragraphs }
}

/// The top-level blocks an edit touches and one on each side, which it may join or come apart from; line starts and ends
/// included. `None` if that is too much to parse for an edit.
fn stretch(doc: &Doc, edit: &Range<Byte>) -> Option<Range<Byte>> {
    let text = doc.text();
    let (mut start, mut end) = (edit.start, edit.end);
    let (mut before, mut after) = (None::<Byte>, None::<Byte>);
    for block in doc.blocks() {
        let r = &block.range;
        if r.end < edit.start {
            before = Some(before.map_or(r.start, |b| b.max(r.start)));
        } else if r.start > edit.end {
            after = Some(after.map_or(r.end, |a| a.min(r.end)));
        } else {
            start = start.min(r.start);
            end = end.max(r.end);
        }
    }
    let start = line_start(text, before.map_or(start, |b| b.min(start)));
    let end = line_end(text, after.map_or(end, |a| a.max(end)));
    (end - start <= MAX_CHECKED).then_some(start..end)
}

/// Whether `edit` of the text of `doc` shows what `expected` says.
pub(super) fn shows_as_expected(doc: &Doc, edit: &Edit, expected: &Expected) -> bool {
    let Some(part) = stretch(doc, &edit.range) else {
        return true;
    };
    let text = doc.text();
    let at_start = part.start == Byte::default();
    let old = shows(text.at(part.clone()), at_start);
    let new_text = format!(
        "{}{}{}",
        text.at(part.start..edit.range.start),
        edit.replacement,
        text.at(edit.range.end..part.end)
    );
    let new = shows(&new_text, at_start);
    if expected.same_paragraphs && old.paragraphs != new.paragraphs {
        return false;
    }
    let typed: Vec<char> = expected.typed.chars().filter(|c| !c.is_whitespace()).collect();
    replaces_one_stretch(&old.chars, &new.chars, &typed, expected.removed)
}

/// Whether `new` is `old` with one stretch, of `removed` characters if known, replaced by `typed`.
fn replaces_one_stretch(old: &[char], new: &[char], typed: &[char], removed: Option<usize>) -> bool {
    if new.len() < typed.len() || removed.is_some_and(|r| r > old.len() || new.len() + r != old.len() + typed.len()) {
        return false;
    }
    let shorter = old.len().min(new.len());
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(shorter)
        .take_while(|(a, b)| a == b)
        .count();
    let kept_behind = new.len() - typed.len();
    (kept_behind.saturating_sub(suffix)..=prefix.min(kept_behind)).any(|at| new[at..at + typed.len()] == *typed)
}

/// Punctuation the typography settings may have made of something else in the source.
const TYPOGRAPHIC: [char; 13] = ['“', '”', '„', '‚', '‘', '’', '«', '»', '‹', '›', '–', '—', '…'];

/// The visible characters of a stretch of the preview, unless what it shows may differ from what a check sees: rendered
/// formulas and pictures, and punctuation the typography settings changed.
pub(super) fn visible_chars(layout: &Layout, index: &Index, range: &Range<Shown>) -> Option<usize> {
    let (first, last) = (index.para_at(range.start)?, index.para_at(range.end)?);
    let mut count = 0;
    for pi in first..=last {
        for (at, run) in index.para(layout, pi).runs_at(index.paras[pi].start) {
            if range.start.max(at.start) < range.end.min(at.end) {
                count += visible_in_run(run, at.start, range)?;
            }
        }
    }
    Some(count)
}

/// The visible characters of `range` in a run that starts at `at`.
fn visible_in_run(run: &Run, mut at: Shown, range: &Range<Shown>) -> Option<usize> {
    match run.map {
        RunMap::Atom { atom: Atom::Marker, .. } => return Some(0),
        RunMap::Atom { atom: Atom::Text, .. } | RunMap::Text { .. } => {}
        RunMap::Atom { .. } => return None,
    }
    let mut count = 0;
    for c in run.text.chars() {
        if range.contains(&at) && !c.is_whitespace() {
            if TYPOGRAPHIC.contains(&c) {
                return None;
            }
            count += 1;
        }
        at += c.len_utf16();
    }
    Some(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use splat_markdown::text::bytes;

    fn stretch_of(old: &str, new: &str, typed: &str, removed: Option<usize>) -> bool {
        let chars = |s: &str| s.chars().collect::<Vec<_>>();
        replaces_one_stretch(&chars(old), &chars(new), &chars(typed), removed)
    }

    fn laid_out(t: &str) -> (Doc, Layout) {
        let doc = splat_markdown::reader::parse(t);
        let l = layout(&doc, &NoImages, None);
        (doc, l)
    }

    #[test]
    fn only_what_shows_as_plain_text_can_be_counted() {
        let (_, l) = laid_out("**ab** cd\n");
        let index = Index::new(&l);
        assert_eq!(visible_chars(&l, &index, &(Shown::new(0)..Shown::new(0))), Some(0));
        assert_eq!(visible_chars(&l, &index, &(Shown::new(0)..Shown::new(1))), Some(1));
        assert_eq!(
            visible_chars(&l, &index, &(Shown::new(0)..Shown::new(5))),
            Some(4),
            "the space does not count"
        );
        assert_eq!(visible_chars(&l, &index, &(Shown::new(1)..Shown::new(3))), Some(1));
        let (_, l) = laid_out("a ![b](c.png) d\n");
        let index = Index::new(&l);
        assert_eq!(visible_chars(&l, &index, &(Shown::new(0)..Shown::new(1))), Some(1));
        assert_eq!(
            visible_chars(&l, &index, &(Shown::new(1)..Shown::new(4))),
            None,
            "a picture counts differently"
        );
        let (_, l) = laid_out("a – b\n");
        let index = Index::new(&l);
        assert_eq!(
            visible_chars(&l, &index, &(Shown::new(0)..Shown::new(5))),
            None,
            "a dash the settings may have made"
        );
        let (_, l) = laid_out("- ab\n- cd\n");
        let index = Index::new(&l);
        let whole: usize = l.blocks.iter().map(|b| b.len_u16).sum();
        assert_eq!(
            visible_chars(&l, &index, &(Shown::new(0)..Shown::new(whole))),
            Some(4),
            "list markers do not count"
        );
    }

    #[test]
    fn an_edit_shows_as_expected_or_not() {
        let expected = |typed, removed, same_paragraphs| Expected {
            typed,
            removed,
            same_paragraphs,
        };
        let edit = |range: Range<usize>, replacement: &str| Edit {
            range: bytes(range),
            replacement: replacement.to_string(),
            selection: bytes(0..0),
        };
        let holds = |t: &str, e: Edit, x: &Expected| {
            let (doc, _) = laid_out(t);
            shows_as_expected(&doc, &e, x)
        };
        assert!(holds("abc\n", edit(1..1, "x"), &expected("x", Some(0), true)));
        assert!(
            !holds("abc\n", edit(1..1, "*x*"), &expected("y", Some(0), true)),
            "something else shows"
        );
        assert!(
            !holds("abc\n", edit(0..0, "# "), &expected("#", Some(0), true)),
            "it became a heading and shows no mark"
        );
        assert!(
            !holds("a\n\nb\n", edit(1..3, ""), &expected("", Some(0), true)),
            "two paragraphs became one"
        );
        assert!(
            holds("a\n\nb\n", edit(1..3, ""), &expected("", Some(0), false)),
            "and that may be what was asked for"
        );
        assert!(
            !holds("x\n\nabc\n\ny\n", edit(3..6, ""), &expected("", Some(1), true)),
            "more went than the selection held"
        );
    }

    #[test]
    fn only_the_blocks_around_an_edit_are_read_again() {
        let t = "a\n\nb\n\nc\n\nd\n\ne\n";
        let (doc, _) = laid_out(t);
        let at = t.find('c').unwrap();
        assert_eq!(
            stretch(&doc, &bytes(at..at)),
            Some(bytes(3..10)),
            "the blocks on each side as well"
        );
        let (doc, _) = laid_out("a\n");
        assert_eq!(stretch(&doc, &bytes(1..1)), Some(bytes(0..1)));
        assert_eq!(stretch(&doc, &bytes(0..0)), Some(bytes(0..1)));
        let long = "a\n\n".to_string() + &"b ".repeat(MAX_CHECKED);
        let (doc, _) = laid_out(&long);
        assert_eq!(stretch(&doc, &bytes(0..0)), None, "too much to read for an edit");
    }

    #[test]
    fn only_one_stretch_may_change_and_only_as_typed() {
        assert!(stretch_of("abcd", "abXcd", "X", Some(0)));
        assert!(stretch_of("aa", "aaa", "a", Some(0)));
        assert!(stretch_of("abcd", "aXd", "X", Some(2)));
        assert!(stretch_of("abcd", "abd", "", Some(1)));
        assert!(stretch_of("abcd", "abd", "", None));
        assert!(!stretch_of("abcd", "aXcdY", "X", Some(0)), "two places changed");
        assert!(!stretch_of("abcd", "abcd", "X", Some(0)), "what was typed is gone");
        assert!(!stretch_of("abcd", "d", "", Some(1)), "more went than was selected");
        assert!(!stretch_of("abcd", "abY", "X", None), "something else appeared");
    }
}
