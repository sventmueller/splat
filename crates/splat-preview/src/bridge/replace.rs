//! Typed, pasted and deleted text: an edit of the source that shows just that, found by trying and checking.

use std::ops::Range;

use splat_markdown::commands::{self, Edit, Pending, leaf_at};
use splat_markdown::doc::{Block, Inline, InlineKind, walk_blocks, walk_inlines};
use splat_markdown::text::{Byte, TextAt, changed, line_end, line_start, units};

use super::escape::escape_typed;
use super::{Bridge, compose, continuation_prefix, with_line_prefixes};
use crate::map::{Join, Shown, deletion_ranges, hidden_in_join, normalize};
use crate::verify::{Expected, shows_as_expected, visible_chars};

impl Bridge<'_> {
    /// Text typed at preview offset `p` while styles are pending at the source caret, as a source edit, plus the
    /// pending styles that still wait for the next word when only whitespace was typed.
    pub fn type_with_pending(&self, pending: &Pending, p: Shown, typed: &str) -> Option<(Edit, Option<Pending>)> {
        if typed.is_empty() || typed.contains('\n') || self.source_at(p)? != pending.at {
            return None;
        }
        let plan = commands::typing_plan(self.doc, pending)?;
        let typed = self.escaped(p, typed, plan.code())?;
        let placed = commands::place(&plan, &typed);
        let caret = placed.at + placed.caret;
        let next = placed.keep_pending.then(|| Pending {
            at: caret,
            styles: pending.styles.clone(),
        });
        let edit = Edit {
            range: placed.at..placed.at,
            replacement: placed.text,
            selection: caret..caret,
        };
        Some((edit, next))
    }

    /// Replaces a preview range with typed or pasted text, in a way that shows just that, or `None` if there is none: the
    /// preview then goes back to what the source says.
    pub fn replace(&self, range: Range<Shown>, typed: &str) -> Option<Edit> {
        let plain = self.plain_replace(range.clone(), typed, false, Join::AroundMarkup)?;
        let (first, last) = (self.index.para_at(range.start)?, self.index.para_at(range.end)?);
        let para = self.index.para(self.layout, first);
        let visible = range.end.get().saturating_sub(range.start.get()) < self.index.paras[first].len;
        let expected = Expected {
            typed,
            removed: visible_chars(self.layout, self.index, &range),
            same_paragraphs: first == last && !typed.contains('\n') && !para.runs.is_empty() && visible,
        };
        let holds = |edit: &Edit| shows_as_expected(self.doc, edit, &expected);
        if holds(&plain) {
            return Some(plain);
        }
        let new = plain.applied_to(self.text);
        let caret = plain.selection.end;
        let spaces_moved = spaces_out_of_markers(&new, caret).map(|(n, c)| self.edit_to(&n, c));
        let within_markup = self.plain_replace(range.clone(), typed, false, Join::WithinMarkup);
        let all_escaped = self.plain_replace(range.clone(), typed, true, Join::AroundMarkup);
        let one_stretch = (first != last).then(|| self.stretch_replaced(&range, typed)).flatten();
        let neighbour_escaped = [
            escaped_behind(&new, caret),
            escaped_before(&new, caret),
            without_blank_line(&new, caret, typed),
        ]
        .into_iter()
        .flatten()
        .map(|(n, c)| self.edit_to(&n, c));
        spaces_moved
            .into_iter()
            .chain(within_markup)
            .chain(all_escaped)
            .chain(one_stretch)
            .chain(neighbour_escaped)
            .find(holds)
            .or_else(|| {
                // A selection across several paragraphs can always go, or Delete would do nothing at all: what it leaves
                // of the markup at its ends shows, and undoing brings it back. Joining two paragraphs has no such excuse.
                let selected = units(&range) > 1;
                (first != last && selected).then_some(plain)
            })
    }

    /// `range` replaced as one stretch of the source, with the delimiters that close and reopen the spans its ends lie in.
    fn stretch_replaced(&self, range: &Range<Shown>, typed: &str) -> Option<Edit> {
        let (start, end) = (self.point(range.start)?.src, self.point(range.end)?.src);
        if start >= end {
            return None;
        }
        let (start, close, _) = self.split_inlines(start);
        let (end, _, open) = self.split_inlines(end);
        let caret = start + close.len() + typed.len();
        Some(Edit {
            range: start..end.max(start),
            replacement: format!("{close}{typed}{open}"),
            selection: caret..caret,
        })
    }

    /// The edit from this text to `new`, with the caret at `caret` there.
    fn edit_to(&self, new: &str, caret: Byte) -> Edit {
        let (old_range, new_range) = changed(self.text, new);
        Edit {
            range: old_range,
            replacement: new.at(new_range).to_string(),
            selection: caret..caret,
        }
    }

    /// `replace` of typing, and the styles that wait for the next word when only whitespace was typed at the edge of a
    /// span: Markdown ignores markers next to whitespace, so it goes outside of them.
    pub fn replace_keeping_styles(&self, range: Range<Shown>, typed: &str) -> Option<(Edit, Option<Pending>)> {
        let edit = self.replace(range.clone(), typed)?;
        let left_span = range.is_empty()
            && !typed.is_empty()
            && typed.trim().is_empty()
            && self
                .point(range.start)
                .is_some_and(|point| edit.selection.end != point.src + typed.len());
        let next = left_span
            .then(|| {
                let point = self.point(range.start)?;
                let styles = commands::styles_at(self.doc, point.src);
                (!styles.is_empty()).then_some(Pending {
                    at: edit.selection.end,
                    styles,
                })
            })
            .flatten();
        Some((edit, next))
    }

    fn plain_replace(&self, range: Range<Shown>, typed: &str, escape_all: bool, join: Join) -> Option<Edit> {
        let point = self.point(range.start)?;
        if point.code
            && typed.contains('`')
            && let Some(edit) = self.backticks_in_code_span(point.src, self.point(range.end)?.src, typed)
        {
            return Some(edit);
        }
        let mut deletions = if range.is_empty() {
            vec![]
        } else {
            deletion_ranges(self.doc, self.layout, self.index, range.clone(), join)
        };
        if !deletions.is_empty() {
            deletions = self.drop_empty_containers(deletions);
        }
        let insert = if escape_all && !point.code {
            escape_every_mark(typed)
        } else {
            escape_typed(typed, &point, self.text)
        };
        let insert = with_line_prefixes(&insert, &continuation_prefix(self.text, point.src));
        let edit = compose(self.text, &deletions, point.src, &insert);
        Some(self.keeping_hidden(edit, &range))
    }

    /// `edit` with what it would delete unseen between two joined paragraphs put back behind the second.
    fn keeping_hidden(&self, mut edit: Edit, range: &Range<Shown>) -> Edit {
        if let Some(hidden) = hidden_in_join(self.doc, self.layout, self.index, range)
            && edit.range.end <= hidden.behind
        {
            edit.replacement.push_str(self.text.at(edit.range.end..hidden.behind));
            edit.replacement.push_str(self.text.at(hidden.lines));
            edit.range.end = hidden.behind;
        }
        edit
    }

    /// Text with backticks replacing `start..end` of a code span, whose delimiters must then be longer than any of them.
    pub(super) fn backticks_in_code_span(&self, start: Byte, end: Byte, typed: &str) -> Option<Edit> {
        let mut span = None;
        walk_inlines(leaf_at(self.doc, start)?.inlines(), &mut |i: &Inline| {
            if matches!(i.kind, InlineKind::Code(_)) && i.range.start <= start && end <= i.range.end {
                span = Some(i);
            }
        });
        let span = span?;
        let inner = span.code_content(self.text);
        if start < inner.start || end > inner.end {
            return None;
        }
        let content = format!(
            "{}{typed}{}",
            self.text.at(inner.start..start),
            self.text.at(end..inner.end)
        );
        let (open, close) = commands::code_markers(&content);
        let caret = span.range.start + open.len() + (start - inner.start) + typed.len();
        Some(Edit {
            range: span.range.clone(),
            replacement: format!("{open}{content}{close}"),
            selection: caret..caret,
        })
    }

    /// Adds the full range of every inline container whose content would be deleted completely. `deletions` come sorted
    /// and apart, and may run into the tens of thousands when a whole document is replaced.
    fn drop_empty_containers(&self, mut deletions: Vec<Range<Byte>>) -> Vec<Range<Byte>> {
        let covered = |r: &Range<Byte>, dels: &[Range<Byte>]| {
            let after = dels.partition_point(|d| d.start <= r.start);
            after > 0 && r.end <= dels[after - 1].end
        };
        let mut leaves = Vec::new();
        walk_blocks(self.doc.blocks(), &mut |b: &Block| {
            let next = deletions.partition_point(|d| d.end <= b.range.start);
            if b.is_leaf() && deletions.get(next).is_some_and(|d| d.start < b.range.end) {
                leaves.push(b);
            }
        });
        loop {
            let mut extra = Vec::new();
            for leaf in &leaves {
                walk_inlines(leaf.inlines(), &mut |i: &Inline| {
                    let content = match i.kind {
                        InlineKind::Code(_) => i.code_content(self.text),
                        _ if i.is_container() => i.content_range(),
                        _ => return,
                    };
                    if !content.is_empty() && covered(&content, &deletions) && !covered(&i.range, &deletions) {
                        extra.push(i.range.clone());
                    }
                });
            }
            if extra.is_empty() {
                return deletions;
            }
            deletions.extend(extra);
            deletions = normalize(deletions);
        }
    }
}

/// Every punctuation mark escaped, which CommonMark allows for all of them; for where the usual rules were not enough.
fn escape_every_mark(typed: &str) -> String {
    let mut out = String::with_capacity(typed.len());
    for c in typed.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Markers that need text on their inner side, longest first.
const SPAN_MARKERS: [&str; 8] = ["**", "__", "~~", "==", "||", "*", "_", "~"];

/// `new` with whitespace at the inner edge of a span's markers moved out, as `**ab **` to `**ab** `.
pub(super) fn spaces_out_of_markers(new: &str, caret: Byte) -> Option<(String, Byte)> {
    let caret = caret.get();
    let is_space = |c: char| c == ' ' || c == '\t';
    let kept = new[..caret].trim_end_matches(is_space).len();
    if kept < caret {
        let mut closers = 0;
        while let Some(m) = SPAN_MARKERS.iter().find(|m| new[caret + closers..].starts_with(**m)) {
            closers += m.len();
        }
        if closers > 0 {
            let spaces = &new[kept..caret];
            let marks = &new[caret..caret + closers];
            return Some((
                format!("{}{marks}{spaces}{}", &new[..kept], &new[caret + closers..]),
                Byte::new(caret + closers),
            ));
        }
    }
    let spaces = new[caret..].len() - new[caret..].trim_start_matches(is_space).len();
    let marks = SPAN_MARKERS.iter().find(|m| new[..caret].ends_with(**m))?;
    (spaces > 0).then(|| {
        let start = caret - marks.len();
        (
            format!(
                "{}{}{marks}{}",
                &new[..start],
                &new[caret..caret + spaces],
                &new[caret + spaces..]
            ),
            Byte::new(caret + spaces),
        )
    })
}

/// `new` with a backslash before the mark behind the caret.
pub(super) fn escaped_behind(new: &str, caret: Byte) -> Option<(String, Byte)> {
    new.from(caret)
        .chars()
        .next()
        .filter(|c| c.is_ascii_punctuation() && *c != '\\')
        .map(|_| (format!("{}\\{}", new.upto(caret), new.from(caret)), caret))
}

/// `new` with a backslash before the mark in front of the caret, spaces aside: the space behind `3.` is what makes a list.
pub(super) fn escaped_before(new: &str, caret: Byte) -> Option<(String, Byte)> {
    let behind = new.upto(caret).trim_end_matches([' ', '\t']);
    let mark = behind.chars().next_back().filter(char::is_ascii_punctuation)?;
    let at = behind.len() - mark.len_utf8();
    (!new[..at].ends_with('\\')).then(|| (format!("{}\\{}", &new[..at], &new[at..]), caret + 1))
}

/// `new` without the line the caret is on, if a deletion left nothing but a container's prefix there: an empty line would
/// end the paragraph.
pub(super) fn without_blank_line(new: &str, caret: Byte, typed: &str) -> Option<(String, Byte)> {
    if !typed.is_empty() {
        return None;
    }
    let start = line_start(new, caret);
    let end = line_end(new, caret);
    if !new.at(start..end).chars().all(|c| matches!(c, ' ' | '\t' | '>'))
        || start == Byte::default()
        || end == new.end()
    {
        return None;
    }
    let at = start - if new.upto(start).ends_with("\r\n") { 2 } else { 1 };
    Some((format!("{}{}", new.upto(at), new.from(end)), at))
}
