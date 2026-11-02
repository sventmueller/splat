use std::ops::Range;

use crate::markdown::commands::{self, Edit, LinePrefix, Pending, leaf_at};
use crate::markdown::doc::{
    Block, BlockKind, Doc, Inline, InlineKind, ListKind, block_path, inline_path, walk_blocks, walk_inlines,
};
use crate::markdown::incremental::changed;
use crate::markdown::table;
use crate::markdown::text::{decode_entity, line_end, line_start};

use super::layout::{Layout, Marker, ParaKind, RunMap};
use super::map::{Index, InsertPoint, Shown, deletion_ranges, insert_point, normalize};
use super::verify::{Expected, shows_as_expected, visible_chars};

/// Translates edits made in the rendered preview into edits of the Markdown source.
pub struct Bridge<'a> {
    pub text: &'a str,
    pub doc: &'a Doc,
    pub layout: &'a Layout,
    pub index: &'a Index,
}

impl Bridge<'_> {
    fn point(&self, p: Shown) -> Option<InsertPoint> {
        insert_point(self.layout, self.index, p)
    }

    /// Source range for a preview selection: the start snaps into the first selected character.
    pub fn source_selection(&self, range: Range<Shown>) -> Range<usize> {
        let end = self.point(range.end).map_or(0, |p| p.src);
        if range.is_empty() {
            return end..end;
        }
        let start = self
            .point(range.start + 1)
            .and_then(|after| self.source_of_char_at(range.start).or(Some(after.src)));
        let start = start.unwrap_or(end);
        start.min(end)..end
    }

    /// Where the character behind preview offset `p` stands in the source.
    fn source_of_char_at(&self, p: Shown) -> Option<usize> {
        let para_index = self.index.para_at(p)?;
        let off = p - self.index.paras[para_index].start;
        let mut unit = 0;
        for run in &self.index.para(self.layout, para_index).runs {
            let len = run.text.encode_utf16().count();
            if (unit..unit + len).contains(&off) {
                return Some(match &run.map {
                    RunMap::Atom { src, .. } => src.start,
                    RunMap::Text { chars, end } => {
                        let mut before = unit;
                        let k = run
                            .text
                            .chars()
                            .take_while(|c| {
                                let behind = before < off;
                                before += c.len_utf16();
                                behind
                            })
                            .count();
                        chars.get(&run.text, k).unwrap_or(*end)
                    }
                });
            }
            unit += len;
        }
        None
    }

    /// Source offset that text typed at preview offset `p` goes to.
    pub fn source_at(&self, p: Shown) -> Option<usize> {
        self.point(p).map(|point| point.src)
    }

    /// Text typed at preview offset `p` while styles are pending at the source caret, as a source edit, plus the
    /// pending styles that still wait for the next word when only whitespace was typed.
    pub fn type_with_pending(&self, pending: &Pending, p: Shown, typed: &str) -> Option<(Edit, Option<Pending>)> {
        if typed.is_empty() || typed.contains('\n') || self.source_at(p)? != pending.at {
            return None;
        }
        let plan = commands::typing_plan(self.text, self.doc, pending)?;
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

    /// `typed` escaped for insertion at preview offset `p`; inside code it stays literal.
    pub fn escaped(&self, p: Shown, typed: &str, code: bool) -> Option<String> {
        let mut point = self.point(p)?;
        point.code = code || matches!(point.para_kind, ParaKind::Code { .. });
        Some(escape_typed(typed, &point, self.text))
    }

    /// Replaces a preview range with typed or pasted text, in a way that shows just that, or `None` if there is none: the
    /// preview then goes back to what the source says. A selection over several paragraphs always goes through.
    pub fn replace(&self, range: Range<Shown>, typed: &str) -> Option<Edit> {
        let plain = self.plain_replace(range.clone(), typed, false)?;
        let (first, last) = (self.index.para_at(range.start)?, self.index.para_at(range.end)?);
        let para = self.index.para(self.layout, first);
        let visible = range.end.0.saturating_sub(range.start.0) < self.index.paras[first].len;
        let expected = Expected {
            typed,
            removed: visible_chars(self.layout, self.index, &range),
            same_paragraphs: first == last && !typed.contains('\n') && !para.runs.is_empty() && visible,
        };
        let holds = |edit: &Edit| shows_as_expected(self.text, self.doc, edit, &expected);
        if holds(&plain) {
            return Some(plain);
        }
        let new = plain.applied_to(self.text);
        let caret = plain.selection.end;
        let spaces_moved = spaces_out_of_markers(&new, caret).map(|(n, c)| self.edit_to(&n, c));
        let all_escaped = self.plain_replace(range.clone(), typed, true);
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
            .chain(all_escaped)
            .chain(neighbour_escaped)
            .find(holds)
            .or_else(|| (first != last).then_some(plain))
    }

    /// The edit from this text to `new`, with the caret at `caret` there.
    fn edit_to(&self, new: &str, caret: usize) -> Edit {
        let (old_range, new_range) = changed(self.text, new);
        Edit {
            range: old_range,
            replacement: new[new_range].to_string(),
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
                let styles = commands::styles_at(self.text, self.doc, point.src);
                (!styles.is_empty()).then_some(Pending {
                    at: edit.selection.end,
                    styles,
                })
            })
            .flatten();
        Some((edit, next))
    }

    fn plain_replace(&self, range: Range<Shown>, typed: &str, escape_all: bool) -> Option<Edit> {
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
            deletion_ranges(self.text, self.layout, self.index, range.clone())
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
        Some(compose(self.text, &deletions, point.src, &insert))
    }

    /// Text with backticks replacing `start..end` of a code span, whose delimiters must then be longer than any of them.
    fn backticks_in_code_span(&self, start: usize, end: usize, typed: &str) -> Option<Edit> {
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
            &self.text[inner.start..start],
            &self.text[end..inner.end]
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
    fn drop_empty_containers(&self, mut deletions: Vec<Range<usize>>) -> Vec<Range<usize>> {
        let covered = |r: &Range<usize>, dels: &[Range<usize>]| {
            let after = dels.partition_point(|d| d.start <= r.start);
            after > 0 && r.end <= dels[after - 1].end
        };
        let mut leaves = Vec::new();
        walk_blocks(&self.doc.blocks, &mut |b: &Block| {
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

    /// Return key: splits paragraphs, continues lists and quotes, moves down in tables.
    pub fn newline(&self, p: Shown) -> Option<Edit> {
        let point = self.point(p)?;
        let at = point.src;
        if point.html
            || matches!(
                point.para_kind,
                ParaKind::Rule | ParaKind::MathBlock | ParaKind::Diagram | ParaKind::FrontMatter
            )
        {
            let block = block_path(&self.doc.blocks, at).into_iter().last()?;
            return Some(compose(self.text, &[], block.range.end, "\n\n"));
        }
        if let ParaKind::TableCell { .. } = point.para_kind {
            return table::move_down(self.text, self.doc, at);
        }
        let prefix = continuation_prefix(self.text, at);
        if let ParaKind::Code { .. } = point.para_kind {
            return Some(compose(self.text, &[], at, &format!("\n{prefix}")));
        }
        let path = block_path(&self.doc.blocks, at);
        let para_empty = self.para_is_empty(p);
        if let Some(item) = path.iter().rev().find(|b| matches!(b.kind, BlockKind::Item { .. })) {
            let BlockKind::Item { marker, task } = &item.kind else {
                unreachable!()
            };
            if para_empty {
                let line_begin = line_start(self.text, marker.start);
                let line_end = line_end(self.text, at);
                let keep = &self.text[line_begin..marker.start];
                let keep = keep.trim_end_matches(' ');
                let replacement = keep.to_string();
                let caret = line_begin + replacement.len();
                return Some(Edit {
                    range: line_begin..line_end,
                    replacement,
                    selection: caret..caret,
                });
            }
            let list = path.iter().rev().find(|b| matches!(b.kind, BlockKind::List { .. }));
            let src_marker = &self.text[marker.clone()];
            let new_marker = match list.map(|l| &l.kind) {
                Some(BlockKind::List {
                    kind: ListKind::Ordered { delim, .. },
                    ..
                }) => {
                    let n: usize = src_marker.trim_end_matches(['.', ')']).parse().unwrap_or(0);
                    format!("{}{}", n + 1, delim)
                }
                _ => src_marker.to_string(),
            };
            let task_box = if task.is_some() { " [ ]" } else { "" };
            let line_begin = line_start(self.text, marker.start);
            let indent = &self.text[line_begin..marker.start];
            let (at, close, open) = self.split_inlines(at);
            let insert = format!("{close}\n{indent}{new_marker}{task_box} {open}");
            return Some(compose(self.text, &[], at, &insert).with_caret_before(open.len()));
        }
        let in_quote = path
            .iter()
            .any(|b| matches!(b.kind, BlockKind::BlockQuote | BlockKind::Alert { .. }));
        if in_quote && para_empty {
            let line_begin = line_start(self.text, at);
            let line_end = line_end(self.text, at);
            let caret = line_begin;
            return Some(Edit {
                range: line_begin..line_end,
                replacement: String::new(),
                selection: caret..caret,
            });
        }
        let (at, close, open) = self.split_inlines(at);
        let quote = prefix.trim_end();
        let insert = if in_quote {
            format!("{close}\n{quote}\n{prefix}{open}")
        } else {
            format!("{close}\n\n{prefix}{open}")
        };
        Some(compose(self.text, &[], at, &insert).with_caret_before(open.len()))
    }

    fn para_is_empty(&self, p: Shown) -> bool {
        self.index.para_at(p).is_none_or(|i| {
            self.index.para(self.layout, i).runs.iter().all(|r| {
                matches!(
                    r.map,
                    RunMap::Atom {
                        atom: super::layout::Atom::Marker,
                        ..
                    }
                )
            })
        })
    }

    /// Where a paragraph splits at `at`, with the delimiters that close and reopen the containers around it, so a split
    /// keeps formatting. At the very start or end of a container it splits outside of it: inside, one side would hold
    /// nothing but its delimiters.
    fn split_inlines(&self, at: usize) -> (usize, String, String) {
        let mut at = at;
        let Some(leaf) = leaf_at(self.doc, at) else {
            return (at, String::new(), String::new());
        };
        let path = inline_path(leaf.inlines(), at);
        let mut close = String::new();
        let mut open = String::new();
        // A code span holds nothing but its text, so it comes first, from inside out.
        let mut code = None;
        walk_inlines(leaf.inlines(), &mut |i: &Inline| {
            if matches!(i.kind, InlineKind::Code(_)) && i.range.start < at && at < i.range.end {
                code = Some(i);
            }
        });
        if let Some(span) = code {
            let content = span.code_content(self.text);
            if at <= content.start {
                at = span.range.start;
            } else if at >= content.end {
                at = span.range.end;
            } else {
                open.push_str(&self.text[span.range.start..content.start]);
                close.push_str(&self.text[content.end..span.range.end]);
            }
        }
        for i in path.iter().rev() {
            if !(i.range.start < at && at < i.range.end) || matches!(i.kind, InlineKind::Image { .. }) {
                continue;
            }
            let content = i.content_range();
            if at <= content.start {
                at = i.range.start;
            } else if at >= content.end {
                at = i.range.end;
            } else {
                open.insert_str(0, &self.text[i.range.start..content.start]);
                close.push_str(&self.text[content.end..i.range.end]);
            }
        }
        (at, close, open)
    }

    /// Shift-Return: a hard line break inside the paragraph.
    pub fn line_break(&self, p: Shown) -> Option<Edit> {
        let point = self.point(p)?;
        // Cells and headings are one line of source.
        if matches!(point.para_kind, ParaKind::TableCell { .. } | ParaKind::Heading(_)) {
            return (!point.code).then(|| compose(self.text, &[], point.src, "<br>"));
        }
        let prefix = continuation_prefix(self.text, point.src);
        let insert = if point.code {
            format!("\n{prefix}")
        } else {
            format!("\\\n{prefix}")
        };
        Some(compose(self.text, &[], point.src, &insert))
    }

    /// Tab and Shift-Tab: next/previous table cell, or indent/outdent a list item.
    pub fn tab(&self, p: Shown, backwards: bool) -> Option<Edit> {
        let point = self.point(p)?;
        if let ParaKind::TableCell { .. } = point.para_kind {
            return table::navigate(self.text, self.doc, point.src, backwards);
        }
        let path = block_path(&self.doc.blocks, point.src);
        let item_pos = path.iter().rposition(|b| matches!(b.kind, BlockKind::Item { .. }))?;
        let BlockKind::Item { marker, .. } = &path[item_pos].kind else {
            return None;
        };
        // Indentation counts from behind the marks of a quote.
        let content_start = |at: usize| {
            let line = line_start(self.text, at);
            line + LinePrefix::parse(&self.text[line..]).quote_end
        };
        let line_begin = content_start(marker.start);
        // In front of this marker may stand another list's, as in `- - a`, or a byte order mark.
        if !self.text[line_begin..marker.start]
            .chars()
            .all(|c| matches!(c, ' ' | '\t'))
        {
            return None;
        }
        let current_indent = marker.start - line_begin;
        if backwards {
            let parent = path[..item_pos].iter().rev().find_map(|b| match &b.kind {
                BlockKind::Item { marker, .. } => Some(marker.start - content_start(marker.start)),
                _ => None,
            })?;
            let remove = current_indent - parent.min(current_indent);
            let edit = Edit {
                range: line_begin..line_begin + remove,
                replacement: String::new(),
                selection: 0..0,
            };
            let caret = point.src - remove;
            return Some(Edit {
                selection: caret..caret,
                ..edit
            });
        }
        let list = path[..item_pos]
            .iter()
            .rev()
            .find(|b| matches!(b.kind, BlockKind::List { .. }))?;
        let index = list.children().iter().position(|c| c.range == path[item_pos].range)?;
        let previous = list.children().get(index.checked_sub(1)?)?;
        let BlockKind::Item {
            marker: prev_marker, ..
        } = &previous.kind
        else {
            return None;
        };
        let prev_line = content_start(prev_marker.start);
        let content_col = prev_marker.end + 1 - prev_line;
        let add = content_col.saturating_sub(current_indent).max(1);
        let spaces = " ".repeat(add);
        let caret = point.src + add;
        Some(Edit {
            range: line_begin..line_begin,
            replacement: spaces,
            selection: caret..caret,
        })
    }

    /// Backspace at the start of a block removes the block formatting instead of joining paragraphs.
    pub fn delete_at_block_start(&self, p: Shown) -> Option<Edit> {
        let point = self.point(p)?;
        if !point.at_para_start {
            return None;
        }
        let para_index = self.index.para_at(p)?;
        let para = self.index.para(self.layout, para_index);
        match &para.style.kind {
            ParaKind::TableCell { .. } | ParaKind::Code { .. } => {
                return Some(Edit {
                    range: point.src..point.src,
                    replacement: String::new(),
                    selection: point.src..point.src,
                });
            }
            _ => {}
        }
        if para.runs.is_empty() && para.src.is_empty() && self.text[..point.src].ends_with("\n\n") {
            let at = point.src - 2;
            return Some(Edit {
                range: at..point.src,
                replacement: String::new(),
                selection: at..at,
            });
        }
        if para.style.marker.is_some() {
            let path = block_path(&self.doc.blocks, point.src);
            if let Some(BlockKind::Item { marker, task }) = path
                .iter()
                .rev()
                .map(|b| &b.kind)
                .find(|k| matches!(k, BlockKind::Item { .. }))
            {
                let end = task.as_ref().map_or(marker.end, |t| t.symbol + 2);
                let end = end + usize::from(self.text[end..].starts_with(' '));
                let caret = marker.start;
                return Some(Edit {
                    range: marker.start..end.max(marker.start),
                    replacement: String::new(),
                    selection: caret..caret,
                });
            }
        }
        let line_begin = line_start(self.text, point.src);
        let prefix = LinePrefix::parse(&self.text[line_begin..]);
        if let Some((r, _)) = prefix.heading {
            let range = line_begin + r.start..line_begin + r.end;
            let caret = range.start;
            return Some(Edit {
                range,
                replacement: String::new(),
                selection: caret..caret,
            });
        }
        if prefix.quote_end > 0 && para.style.quote_depth > 0 {
            let line = &self.text[line_begin..];
            let gt = line.find('>')?;
            let end = gt + 1 + usize::from(line[gt + 1..].starts_with(' '));
            let caret = point.src.saturating_sub(end).max(line_begin);
            return Some(Edit {
                range: line_begin..line_begin + end,
                replacement: String::new(),
                selection: caret..caret,
            });
        }
        None
    }

    /// A click on a callout's chevron flips its `-` and `+`, which folds or unfolds it.
    pub fn toggle_fold(&self, p: Shown) -> Option<Edit> {
        let para_index = self.index.para_at(p)?;
        let offset = p.0.checked_sub(self.index.paras[para_index].start.0)?;
        let mut at = 0;
        for run in &self.index.para(self.layout, para_index).runs {
            let len = run.text.encode_utf16().count();
            if !(at..at + len).contains(&offset) {
                at += len;
                continue;
            }
            match &run.map {
                RunMap::Atom {
                    atom: super::layout::Atom::Fold { open },
                    src,
                } => {
                    return Some(Edit {
                        range: src.clone(),
                        replacement: if *open { "-" } else { "+" }.to_string(),
                        selection: src.end..src.end,
                    });
                }
                RunMap::Atom {
                    atom: super::layout::Atom::Details { open },
                    src,
                } => {
                    let (range, replacement) = toggle_open(&self.text[src.clone()], *open)?;
                    let range = src.start + range.start..src.start + range.end;
                    let caret = range.start + replacement.len();
                    return Some(Edit {
                        range,
                        replacement,
                        selection: caret..caret,
                    });
                }
                _ => {}
            }
            at += len;
        }
        None
    }

    /// Toggles the checkbox of the task item whose marker sits in the paragraph at `p`.
    pub fn toggle_task(&self, p: Shown) -> Option<Edit> {
        let para_index = self.index.para_at(p)?;
        let Some(Marker::Task {
            checked,
            symbol: Some(symbol),
        }) = self.index.para(self.layout, para_index).style.marker.clone()
        else {
            return None;
        };
        if symbol >= self.text.len() {
            return None;
        }
        let replacement = if checked { " " } else { "x" }.to_string();
        Some(Edit {
            range: symbol..symbol + 1,
            replacement,
            selection: symbol + 1..symbol + 1,
        })
    }
}

impl Edit {
    fn applied_to(&self, text: &str) -> String {
        format!(
            "{}{}{}",
            &text[..self.range.start],
            self.replacement,
            &text[self.range.end..]
        )
    }

    fn with_caret_before(mut self, n: usize) -> Edit {
        let caret = self.selection.end - n;
        self.selection = caret..caret;
        self
    }
}

/// Container prefix a continuation line needs: quote markers and list-item indentation.
pub fn continuation_prefix(text: &str, at: usize) -> String {
    let line_begin = line_start(text, at);
    let line = &text[line_begin..];
    let p = LinePrefix::parse(line);
    let prefix_end = match &p.list {
        Some((r, _)) => r.end,
        None => p.quote_end + (line[p.quote_end..].len() - line[p.quote_end..].trim_start_matches(' ').len()),
    };
    commands::continuation_of(&line[..prefix_end])
}

/// Builds one edit that removes `deletions` and inserts `insert` at `at`, so undo sees a single step.
fn compose(text: &str, deletions: &[Range<usize>], at: usize, insert: &str) -> Edit {
    let at = deletions
        .iter()
        .find(|d| d.start < at && at < d.end)
        .map_or(at, |d| d.start);
    let start = deletions.first().map_or(at, |d| d.start.min(at));
    let end = deletions.last().map_or(at, |d| d.end.max(at));
    let mut out = String::new();
    let mut pos = start;
    let mut caret = start;
    let mut inserted = false;
    for d in deletions {
        if !inserted && at <= d.start {
            out.push_str(&text[pos..at]);
            out.push_str(insert);
            caret = start + out.len();
            pos = at;
            inserted = true;
        }
        out.push_str(&text[pos..d.start]);
        pos = d.end;
    }
    if !inserted {
        let at = at.max(pos);
        out.push_str(&text[pos..at]);
        out.push_str(insert);
        caret = start + out.len();
        pos = at;
    }
    out.push_str(&text[pos..end]);
    Edit {
        range: start..end,
        replacement: out,
        selection: caret..caret,
    }
}

/// Lines behind the first of a text inserted in a quote or list need the prefix of that container, or they end it.
fn with_line_prefixes(insert: &str, prefix: &str) -> String {
    if prefix.is_empty() || !insert.contains('\n') {
        return insert.to_string();
    }
    let mut lines = insert.split('\n');
    let mut out = lines.next().unwrap_or("").to_string();
    for line in lines {
        out.push('\n');
        out.push_str(if line.is_empty() { prefix.trim_end() } else { prefix });
        out.push_str(line);
    }
    out
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
fn spaces_out_of_markers(new: &str, caret: usize) -> Option<(String, usize)> {
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
                caret + closers,
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
            caret + spaces,
        )
    })
}

/// `new` with a backslash before the mark behind the caret.
fn escaped_behind(new: &str, caret: usize) -> Option<(String, usize)> {
    new[caret..]
        .chars()
        .next()
        .filter(|c| c.is_ascii_punctuation() && *c != '\\')
        .map(|_| (format!("{}\\{}", &new[..caret], &new[caret..]), caret))
}

/// `new` with a backslash before the mark in front of the caret, spaces aside: the space behind `3.` is what makes a list.
fn escaped_before(new: &str, caret: usize) -> Option<(String, usize)> {
    let behind = new[..caret].trim_end_matches([' ', '\t']);
    let mark = behind.chars().next_back().filter(char::is_ascii_punctuation)?;
    let at = behind.len() - mark.len_utf8();
    (!new[..at].ends_with('\\')).then(|| (format!("{}\\{}", &new[..at], &new[at..]), caret + 1))
}

/// `new` without the line the caret is on, if a deletion left nothing but a container's prefix there: an empty line would
/// end the paragraph.
fn without_blank_line(new: &str, caret: usize, typed: &str) -> Option<(String, usize)> {
    if !typed.is_empty() {
        return None;
    }
    let start = line_start(new, caret);
    let end = new[caret..].find(['\n', '\r']).map_or(new.len(), |i| caret + i);
    if !new[start..end].chars().all(|c| matches!(c, ' ' | '\t' | '>')) || start == 0 || end == new.len() {
        return None;
    }
    let at = start - if new[..start].ends_with("\r\n") { 2 } else { 1 };
    Some((format!("{}{}", &new[..at], &new[end..]), at))
}

/// Escapes characters typed into the preview so they stay literal text in Markdown.
fn escape_typed(typed: &str, point: &InsertPoint, text: &str) -> String {
    let in_table = matches!(point.para_kind, ParaKind::TableCell { .. });
    if point.code {
        // A `|` in a table ends the cell even inside a code span.
        return if in_table && !matches!(point.para_kind, ParaKind::Code { .. }) {
            typed.replace('|', "\\|")
        } else {
            typed.to_string()
        };
    }
    if point.html {
        return typed.replace('&', "&amp;").replace('<', "&lt;");
    }
    let chars: Vec<char> = typed.chars().collect();
    let before = text[..point.src].chars().next_back();
    let after = text[point.src..].chars().next();
    let mut out = String::new();
    for (n, &c) in chars.iter().enumerate() {
        let prev = if n == 0 { before } else { Some(chars[n - 1]) };
        let next = chars.get(n + 1).copied().or(after);
        match c {
            '\\' | '`' | '*' | '[' | ']' | '<' | '$' | '~' | '^' => {
                out.push('\\');
                out.push(c);
            }
            '&' if decode_entity(&format!(
                "&{}",
                chars[n + 1..].iter().take(LONGEST_REFERENCE).collect::<String>()
            ))
            .is_some() =>
            {
                out.push_str("\\&");
            }
            // The end of what would otherwise read as a character reference.
            ';' if closes_reference(
                &context(text, point.src, LONGEST_REFERENCE),
                tail(&out, LONGEST_REFERENCE),
            ) =>
            {
                out.push_str("\\;");
            }
            '|' if in_table => out.push_str("\\|"),
            // Doubled, they would start a highlight, a spoiler or a comment.
            '=' | '|' | '%' if prev == Some(c) || next == Some(c) => {
                out.push('\\');
                out.push(c);
            }
            ':' if closes_shortcode(
                &context(text, point.src, LONGEST_SHORTCODE),
                tail(&out, LONGEST_SHORTCODE),
            ) =>
            {
                out.push_str("\\:");
            }
            '{' if matches!(point.para_kind, ParaKind::Heading(_)) => out.push_str("\\{"),
            '_' => {
                if prev.is_none_or(|p| !p.is_alphanumeric()) || next.is_none_or(|x| !x.is_alphanumeric()) {
                    out.push('\\');
                }
                out.push('_');
            }
            '\n' if in_table => out.push(' '),
            _ => out.push(c),
        }
    }
    let prefix = &text[line_start(text, point.src)..point.src];
    if point.at_para_start || prefix.chars().all(|c| matches!(c, ' ' | '\t' | '>')) {
        if out.starts_with(['#', '>', '-', '+', '=', ':']) {
            out.insert(0, '\\');
        } else {
            let digits = out.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && matches!(out.as_bytes().get(digits), Some(b'.' | b')')) {
                out.insert(digits, '\\');
            }
        }
    }
    out
}

/// More bytes than a character reference, `&CounterClockwiseContourIntegral;`, and an emoji shortcode can have: looking
/// no further back keeps pasting a long text of `&` and `;` linear.
const LONGEST_REFERENCE: usize = 40;
const LONGEST_SHORTCODE: usize = 48;

fn tail(s: &str, bytes: usize) -> &str {
    &s[s.floor_char_boundary(s.len().saturating_sub(bytes))..]
}

fn context(text: &str, end: usize, bytes: usize) -> String {
    tail(&text[..end], bytes).to_string()
}

/// Whether a `;` behind `before` and `typed` ends a character reference.
fn closes_reference(before: &str, typed: &str) -> bool {
    let text = format!("{before}{typed};");
    text.rfind('&').is_some_and(|at| {
        !text[..at].ends_with('\\') && decode_entity(&text[at..]).is_some_and(|(_, len)| at + len == text.len())
    })
}

/// The change to a `<details>` tag that adds or removes its `open` attribute.
fn toggle_open(tag: &str, open: bool) -> Option<(Range<usize>, String)> {
    let is_details = matches!(
        crate::markdown::markup::read_tag(tag, 0),
        Some((crate::markdown::markup::Tag::Open { name, .. }, end)) if name == "details" && end == tag.len()
    );
    if !is_details {
        return None;
    }
    let lower = tag.to_ascii_lowercase();
    if !open {
        let end = if lower.ends_with("/>") {
            tag.len() - 2
        } else {
            tag.len().checked_sub(1)?
        };
        return Some((end..end, " open".to_string()));
    }
    let bytes = lower.as_bytes();
    let is_space = |at: usize| bytes.get(at).is_some_and(u8::is_ascii_whitespace);
    // From attribute to attribute: `open` in the value of another one is not it.
    let mut at = "<details".len();
    while at < bytes.len() && !matches!(bytes[at], b'>' | b'/') {
        if is_space(at) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && !is_space(at) && !matches!(bytes[at], b'=' | b'>' | b'/') {
            at += 1;
        }
        let name = &lower[start..at];
        let behind_name = at;
        let mut probe = at;
        while is_space(probe) {
            probe += 1;
        }
        if bytes.get(probe) == Some(&b'=') {
            probe += 1;
            while is_space(probe) {
                probe += 1;
            }
            at = match bytes.get(probe) {
                Some(&q @ (b'"' | b'\'')) => lower[probe + 1..]
                    .find(q as char)
                    .map_or(lower.len(), |e| probe + e + 2),
                _ => {
                    probe
                        + lower[probe..]
                            .find(|c: char| c.is_ascii_whitespace() || c == '>')
                            .unwrap_or(lower.len() - probe)
                }
            };
        }
        if name == "open" {
            let end = if at > behind_name { at } else { behind_name };
            let space = lower[..start].trim_end().len();
            return Some((space..end, String::new()));
        }
    }
    None
}

/// Whether a `:` typed after `before` and `typed` would end an emoji shortcode such as `:smile:`.
fn closes_shortcode(before: &str, typed: &str) -> bool {
    let context = format!("{before}{typed}");
    let name: usize = context
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))
        .map(char::len_utf8)
        .sum();
    let rest = &context[..context.len() - name];
    name > 0 && rest.ends_with(':') && !rest.ends_with("\\:")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The source after `edit` of the preview of `t`.
    fn edited(t: &str, edit: impl Fn(&Bridge) -> Option<Edit>) -> String {
        let d = crate::markdown::doc::parse(t);
        let l = super::super::layout::layout(t, &d, &super::super::layout::NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge {
            text: t,
            doc: &d,
            layout: &l,
            index: &index,
        };
        let mut out = t.to_string();
        edit(&bridge).expect("an edit").apply_to(&mut out);
        out
    }

    /// What the preview shows of `t`, its paragraphs in lines.
    fn shown_of(t: &str) -> String {
        let d = crate::markdown::doc::parse(t);
        let l = super::super::layout::layout(t, &d, &super::super::layout::NoImages, None);
        l.blocks
            .iter()
            .flat_map(|b| &b.paras)
            .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Types `keys` one by one behind `after` in what the preview shows, each on the text the one before left, styles
    /// that wait for the next word included.
    fn typed_after(t: &str, after: &str, keys: &str) -> String {
        let mut text = t.to_string();
        let shown = shown_of(t);
        let start = shown[..shown.find(after).unwrap() + after.len()].encode_utf16().count();
        let mut caret = None;
        let mut pending: Option<Pending> = None;
        for key in keys.chars() {
            let d = crate::markdown::doc::parse(&text);
            let l = super::super::layout::layout(&text, &d, &super::super::layout::NoImages, caret);
            let index = Index::new(&l);
            let p = caret.map_or(start, |at| super::super::map::preview_offset(&l, &index, at).0);
            let bridge = Bridge {
                text: &text,
                doc: &d,
                layout: &l,
                index: &index,
            };
            let key_text = key.to_string();
            let waiting = pending
                .as_ref()
                .and_then(|w| bridge.type_with_pending(w, Shown(p), &key_text));
            let (edit, next) = match waiting {
                Some(typed) => typed,
                None => bridge
                    .replace_keeping_styles(Shown(p)..Shown(p), &key_text)
                    .unwrap_or_else(|| panic!("{key:?} refused at {p} of {text:?}")),
            };
            pending = next;
            caret = Some(edit.selection.end);
            let mut new = text.clone();
            edit.apply_to(&mut new);
            text = new;
        }
        text
    }

    #[test]
    fn typing_never_turns_text_into_syntax() {
        for (start, after, keys, shows) in [
            ("[a] x\n", "[a]", ":", "[a]: x"),
            ("x [Link](#a)\n", "x ", "!", "x !Link"),
            ("Oktober\n", "", "3. ", "3. Oktober"),
            ("foo\nbar\n", "foo ", "# ", "foo # bar"),
            ("foo\nbar\n", "foo ", "- ", "foo - bar"),
            ("# Issue\n", "Issue", " #12", "Issue #12"),
            ("amp; x\n", "", "&", "&amp; x"),
            ("smile: x\n", "smile", ":", "smile:: x"),
            ("[a] x\n", "[a]", "(b)", "[a](b) x"),
        ] {
            let text = typed_after(start, after, keys);
            assert_eq!(shown_of(&text), shows, "{start:?} + {keys:?} -> {text:?}");
        }
    }

    #[test]
    fn a_space_at_the_end_of_a_span_leaves_it_and_the_next_word_takes_up_its_style() {
        assert_eq!(typed_after("**ab** c\n", "ab", " "), "**ab**  c\n");
        let text = typed_after("**ab** c\n", "ab", " de");
        assert_eq!(text, "**ab** **de** c\n");
        assert_eq!(shown_of(&text), "ab de c");
        assert_eq!(typed_after("*ab*\n", "ab", " "), "*ab* \n");
    }

    #[test]
    fn edits_find_their_place_where_the_parser_is_wrong_about_it() {
        assert_eq!(
            typed_after("[a]: b\nfoo **bar** baz\n", "bar", "x"),
            "[a]: b\nfoo **barx** baz\n"
        );
        assert_eq!(typed_after("a\rb\n\n**c** d\n", "c", "x"), "a\rb\n\n**cx** d\n");
        assert_eq!(
            typed_after("x\r\ry\r\r# H\r\rsome **bold** word\r", "bold", "x"),
            "x\r\ry\r\r# H\r\rsome **boldx** word\r"
        );
        assert_eq!(
            typed_after("> [a](b\n> \"c\") **d** e\n", "d", "x"),
            "> [a](b\n> \"c\") **dx** e\n"
        );
    }

    #[test]
    fn deleting_a_little_removes_just_that() {
        let delete = |t: &str, what: &str| {
            let shown = shown_of(t);
            let at = shown.find(what).unwrap();
            let from = shown[..at].encode_utf16().count();
            let to = from + what.encode_utf16().count();
            edited(t, |b| b.replace(Shown(from)..Shown(to), ""))
        };
        assert_eq!(delete("😀\r\rWort zwei 😀", "Wor"), "😀\r\rt zwei 😀");
        assert_eq!(delete("[a]x(b)\n", "x"), "[a]\\(b)\n");
        assert_eq!(delete("x# a\n", "x"), "\\# a\n");
        assert_eq!(delete("**a b** c\n", "b"), "**a**  c\n");
        assert_eq!(
            delete("vor\n**fett** und\n![x](y.png)text\nnach\n", "\u{FFFC}text"),
            "vor\n**fett** und\nnach\n",
            "a line goes as a whole"
        );
    }

    #[test]
    fn return_in_an_alert_continues_it() {
        assert_eq!(
            edited("> [!NOTE]\n> ab\n", |b| {
                let shown = shown_of("> [!NOTE]\n> ab\n");
                b.newline(Shown(shown[..shown.find("ab").unwrap()].encode_utf16().count() + 1))
            }),
            "> [!NOTE]\n> a\n>\n> b\n"
        );
    }

    #[test]
    fn a_line_break_stays_in_the_one_line_of_a_cell_or_heading() {
        let table = "| a |\n|---|\n| xy |\n";
        assert_eq!(edited(table, |b| b.line_break(Shown(3))), "| a |\n|---|\n| x<br>y |\n");
        assert_eq!(edited("# ab\n", |b| b.line_break(Shown(1))), "# a<br>b\n");
    }

    #[test]
    fn tab_indents_a_list_item_behind_the_marks_of_a_quote() {
        let t = "> - a\n> - b\n";
        let at = shown_of(t).find('b').unwrap();
        let shown = shown_of(t);
        let at = shown[..at].encode_utf16().count();
        assert_eq!(edited(t, |b| b.tab(Shown(at), false)), "> - a\n>   - b\n");
    }

    #[test]
    fn several_lines_pasted_into_a_quote_stay_in_it() {
        let t = "> ```\n> x\n> ```\n";
        assert_eq!(
            edited(t, |b| b.replace(Shown(1)..Shown(1), "a\nb")),
            "> ```\n> xa\n> b\n> ```\n"
        );
    }

    #[test]
    fn a_pipe_in_a_code_span_stays_inside_its_cell() {
        let t = "| a |\n|---|\n| `x` |\n";
        assert_eq!(
            edited(t, |b| b.replace(Shown(3)..Shown(3), "|")),
            "| a |\n|---|\n| `x\\|` |\n"
        );
    }

    #[test]
    fn toggling_open_looks_at_attributes_and_not_into_their_values() {
        assert_eq!(
            toggle_open("<details title=\"a open b\" open>", true),
            Some((25..30, String::new()))
        );
        assert_eq!(toggle_open("<details title=\"a open b\">", true), None);
        assert_eq!(
            toggle_open("<details open=\"open\">", true),
            Some((8..20, String::new()))
        );
    }

    #[test]
    fn typed_pipes_and_line_breaks_stay_inside_a_table_cell() {
        let t = "| a | b |\n|---|---|\n| x | y |\n";
        let at = 2 + 2; // Behind the header cells, each followed by a paragraph break, is the cell `x`.
        assert_eq!(
            edited(t, |b| b.replace(Shown(at)..Shown(at), "|")),
            "| a | b |\n|---|---|\n| \\|x | y |\n"
        );
        assert_eq!(
            edited(t, |b| b.replace(Shown(at)..Shown(at), "n\nm")),
            "| a | b |\n|---|---|\n| n mx | y |\n"
        );
    }

    #[test]
    fn return_in_a_code_block_adds_a_line_to_it() {
        assert_eq!(edited("```\nab\n```\n", |b| b.newline(Shown(1))), "```\na\nb\n```\n");
        assert_eq!(
            edited("> ```\n> ab\n> ```\n", |b| b.newline(Shown(1))),
            "> ```\n> a\n> b\n> ```\n"
        );
    }

    #[test]
    fn typed_text_that_would_start_a_block_stays_text() {
        for (typed, source) in [
            ("1. ", "1\\. x\n"),
            ("12) ", "12\\) x\n"),
            ("- ", "\\- x\n"),
            ("# ", "\\# x\n"),
            ("> ", "\\> x\n"),
            ("1 ", "1 x\n"),
        ] {
            assert_eq!(
                edited("x\n", |b| b.replace(Shown(0)..Shown(0), typed)),
                source,
                "{typed:?}"
            );
        }
        assert_eq!(
            edited("x\n", |b| b.replace(Shown(1)..Shown(1), "1. ")),
            "x1. \n",
            "only at the start"
        );
    }

    #[test]
    fn tab_indents_a_list_item_under_the_one_before_and_back() {
        assert_eq!(edited("- a\n- b\n", |b| b.tab(Shown(6), false)), "- a\n  - b\n");
        assert_eq!(
            edited("x\n\n1. a\n2. b\n", |b| b.tab(Shown(8), false)),
            "x\n\n1. a\n   2. b\n"
        );
        assert_eq!(edited("- a\n  - b\n", |b| b.tab(Shown(6), true)), "- a\n- b\n");
        assert_eq!(
            edited("x\n\n- a\n  - b\n", |b| b.tab(Shown(8), true)),
            "x\n\n- a\n- b\n"
        );
        assert_eq!(
            edited("- a\n  - b\n    - c\n", |b| b.tab(Shown(10), true)),
            "- a\n  - b\n  - c\n"
        );
    }

    #[test]
    fn tab_keeps_the_caret_on_its_character_and_leaves_what_cannot_move() {
        let caret = |t: &str, p: usize, back: bool| {
            let d = crate::markdown::doc::parse(t);
            let l = super::super::layout::layout(t, &d, &super::super::layout::NoImages, None);
            let index = Index::new(&l);
            let b = Bridge {
                text: t,
                doc: &d,
                layout: &l,
                index: &index,
            };
            b.tab(Shown(p), back).map(|edit| {
                let at = edit.selection.start;
                assert_eq!(edit.selection.end, at);
                let mut out = t.to_string();
                edit.apply_to(&mut out);
                out[at..].chars().next()
            })
        };
        assert_eq!(caret("- a\n- bc\n", 7, false), Some(Some('c')));
        assert_eq!(caret("- a\n  - bc\n", 7, true), Some(Some('c')));
        assert_eq!(caret("- a\n", 2, false), None, "the first item has none to go under");
        assert_eq!(caret("- a\n", 2, true), None, "nor a level to go back to");
        assert_eq!(caret("Text\n", 2, false), None);
    }

    #[test]
    fn backspace_at_the_start_of_a_block_takes_its_formatting_away() {
        let backspace = |t: &str, p: usize| edited(t, |b| b.delete_at_block_start(Shown(p)));
        assert_eq!(backspace("x\n\n# Titel\n", 2), "x\n\nTitel\n");
        assert_eq!(backspace("x\n\n> Zitat\n", 2), "x\n\nZitat\n");
        assert_eq!(backspace("x\n\n>Zitat\n", 2), "x\n\nZitat\n");
        assert_eq!(backspace("x\n\n- Punkt\n", 2 + 2), "x\n\nPunkt\n");
        assert_eq!(backspace("x\n\n- [ ] Aufgabe\n", 2 + 2), "x\n\nAufgabe\n");
        assert_eq!(backspace("x\n\n1. Eins\n", 2 + 3), "x\n\nEins\n");
        // Inside a table or code it stays where it is, without a step to undo.
        assert_eq!(backspace("```\nab\n```\n", 0), "```\nab\n```\n");
        assert_eq!(backspace("| a |\n|---|\n| b |\n", 2), "| a |\n|---|\n| b |\n");
    }

    #[test]
    fn backspace_inside_a_block_or_in_plain_text_is_left_to_the_ordinary_deletion() {
        let t = "x\n\n# Titel\n\nText\n";
        let d = crate::markdown::doc::parse(t);
        let l = super::super::layout::layout(t, &d, &super::super::layout::NoImages, None);
        let index = Index::new(&l);
        let b = Bridge {
            text: t,
            doc: &d,
            layout: &l,
            index: &index,
        };
        assert_eq!(b.delete_at_block_start(Shown(3)), None, "inside the heading");
        assert_eq!(
            b.delete_at_block_start(Shown(8)),
            None,
            "at the start of a plain paragraph"
        );
    }

    #[test]
    fn checkboxes_and_chevrons_find_their_own_place_further_down() {
        let t = "Text\n\n- [ ] a\n- [x] b\n";
        assert_eq!(edited(t, |b| b.toggle_task(Shown(5))), "Text\n\n- [x] a\n- [x] b\n");
        assert_eq!(edited(t, |b| b.toggle_task(Shown(9))), "Text\n\n- [ ] a\n- [ ] b\n");
        let t = "x\n\n> [!note]- Titel\n> Inhalt\n\ny\n";
        let chevron = 2 + 1 + " Titel".len();
        assert_eq!(
            edited(t, |b| b.toggle_fold(Shown(chevron))),
            "x\n\n> [!note]+ Titel\n> Inhalt\n\ny\n"
        );
        let t = "x\n\n> [!note]+ Titel\n> Inhalt\n\ny\n";
        assert_eq!(
            edited(t, |b| b.toggle_fold(Shown(chevron))),
            "x\n\n> [!note]- Titel\n> Inhalt\n\ny\n"
        );
    }

    #[test]
    fn nothing_but_the_chevron_folds_and_nothing_but_a_checkbox_toggles() {
        let t = "x\n\n> [!note]- Titel\n> Inhalt\n\n- [ ] y\n";
        let d = crate::markdown::doc::parse(t);
        let l = super::super::layout::layout(t, &d, &super::super::layout::NoImages, None);
        let index = Index::new(&l);
        let b = Bridge {
            text: t,
            doc: &d,
            layout: &l,
            index: &index,
        };
        assert_eq!(b.toggle_fold(Shown(0)), None);
        assert_eq!(b.toggle_fold(Shown(3)), None, "the title itself");
        assert_eq!(b.toggle_task(Shown(0)), None);
    }

    #[test]
    fn the_caret_stays_behind_what_a_click_changed() {
        let t = "- [ ] a\n";
        let d = crate::markdown::doc::parse(t);
        let l = super::super::layout::layout(t, &d, &super::super::layout::NoImages, None);
        let index = Index::new(&l);
        let b = Bridge {
            text: t,
            doc: &d,
            layout: &l,
            index: &index,
        };
        let edit = b.toggle_task(Shown(0)).unwrap();
        assert_eq!(edit.range, 3..4);
        assert_eq!(edit.selection, 4..4);
    }

    #[test]
    fn the_open_attribute_goes_and_comes_whatever_way_it_is_written() {
        let toggled = |tag: &str, open: bool| {
            let (range, replacement) = toggle_open(tag, open).unwrap();
            format!("{}{replacement}{}", &tag[..range.start], &tag[range.end..])
        };
        assert_eq!(toggled("<details>", false), "<details open>");
        assert_eq!(toggled("<details/>", false), "<details open/>");
        assert_eq!(toggled("<details class=\"a\">", false), "<details class=\"a\" open>");
        assert_eq!(toggled("<details open>", true), "<details>");
        assert_eq!(toggled("<DETAILS OPEN>", true), "<DETAILS>");
        assert_eq!(toggled("<details open class=\"a\">", true), "<details class=\"a\">");
        assert_eq!(toggled("<details open=\"\">", true), "<details>");
        assert_eq!(toggled("<details open='open' id=x>", true), "<details id=x>");
        assert_eq!(toggled("<details open=open id=x>", true), "<details id=x>");
        assert_eq!(toggled("<details open = \"\">", true), "<details>");
        assert_eq!(
            toggled("<details class=\"open\" open>", true),
            "<details class=\"open\">"
        );
        assert_eq!(toggled("<details data-open open>", true), "<details data-open>");
        assert_eq!(toggled("<details open=\"a\" b>", true), "<details b>");
        assert_eq!(toggled("<details open='a' b>", true), "<details b>");
        assert_eq!(toggled("<details openly open>", true), "<details openly>");
    }

    #[test]
    fn a_shift_return_breaks_the_line_and_keeps_the_prefix() {
        assert_eq!(edited("ab\n", |b| b.line_break(Shown(1))), "a\\\nb\n");
        assert_eq!(edited("- ab\n", |b| b.line_break(Shown(3))), "- a\\\n  b\n");
        assert_eq!(edited("> ab\n", |b| b.line_break(Shown(1))), "> a\\\n> b\n");
        assert_eq!(edited("```\nab\n```\n", |b| b.line_break(Shown(1))), "```\na\nb\n```\n");
        assert_eq!(edited("x\n\n  ab\n", |b| b.line_break(Shown(3))), "x\n\n  a\\\n  b\n");
    }

    #[test]
    fn return_splits_a_paragraph_and_keeps_formatting_on_both_sides() {
        assert_eq!(edited("ab\n", |b| b.newline(Shown(1))), "a\n\nb\n");
        assert_eq!(edited("**ab** c\n", |b| b.newline(Shown(1))), "**a**\n\n**b** c\n");
        assert_eq!(edited("**ab** c\n", |b| b.newline(Shown(0))), "\n\n**ab** c\n");
        assert_eq!(edited("**ab** c\n", |b| b.newline(Shown(2))), "**ab**\n\n c\n");
        assert_eq!(edited("[ab](c) d\n", |b| b.newline(Shown(1))), "[a](c)\n\n[b](c) d\n");
        assert_eq!(edited("`ab` c\n", |b| b.newline(Shown(1))), "`a`\n\n`b` c\n");
        assert_eq!(edited("> ab\n", |b| b.newline(Shown(1))), "> a\n>\n> b\n");
        assert_eq!(edited("x\n\n> ab\n", |b| b.newline(Shown(3))), "x\n\n> a\n>\n> b\n");
        assert_eq!(edited("- [ ] ab\n", |b| b.newline(Shown(3))), "- [ ] a\n- [ ] b\n");
    }

    #[test]
    fn typed_text_is_escaped_only_where_it_would_mean_something() {
        let typed = |t: &str, p: usize, typed: &str| edited(t, |b| b.replace(Shown(p)..Shown(p), typed));
        assert_eq!(typed("ab\n", 1, "_"), "a_b\n", "inside a word");
        assert_eq!(typed("a b\n", 1, "_"), "a\\_ b\n", "at the end of a word");
        assert_eq!(typed("a b\n", 2, "_"), "a \\_b\n", "at the start of a word");
        assert_eq!(typed("ab\n", 1, "{"), "a{b\n", "in a paragraph");
        assert_eq!(
            typed("# ab\n", 1, "{"),
            "# a\\{b\n",
            "in a heading, where it could start attributes"
        );
        assert_eq!(typed("ab\n", 1, "=="), "a\\=\\=b\n");
        assert_eq!(typed("ab\n", 1, "="), "a=b\n");
        assert_eq!(typed("a=b\n", 2, "="), "a=\\=b\n", "next to one there already");
        assert_eq!(
            typed("ab\n", 1, "\n"),
            "a\nb\n",
            "only a table cell turns it into a space"
        );
        assert_eq!(typed("ab\n", 0, "1"), "1ab\n", "digits alone start nothing");
        assert_eq!(typed(":smile\n", 6, ":"), ":smile\\:\n", "it would close a shortcode");
        assert_eq!(typed(":\n", 1, ":"), "::\n", "there is no name to close");
        assert_eq!(typed("ab\n", 0, "1"), "1ab\n", "digits without a delimiter");
        assert_eq!(typed("ab\n", 0, "12x"), "12xab\n");
        assert_eq!(typed("smile\n", 5, ":"), "smile:\n");
    }

    #[test]
    fn return_inside_nested_formatting_closes_and_reopens_it_inside_out() {
        assert_eq!(
            edited("***ab*** c\n", |b| b.newline(Shown(1))),
            "***a***\n\n***b*** c\n"
        );
        assert_eq!(
            edited("**a *bc* d**\n", |b| b.newline(Shown(3))),
            "**a *b***\n\n***c* d**\n"
        );
        assert_eq!(
            edited("**a *b* c**\n", |b| b.newline(Shown(2))),
            "**a **\n\n***b* c**\n"
        );
        assert_eq!(edited("x **ab**\n", |b| b.newline(Shown(2))), "x \n\n**ab**\n");
        assert_eq!(edited("x **ab**\n", |b| b.newline(Shown(4))), "x **ab**\n\n\n");
        assert_eq!(edited("- **ab**\n", |b| b.newline(Shown(3))), "- **a**\n- **b**\n");
    }

    #[test]
    fn a_split_list_item_and_a_quote_keep_what_stands_before_them() {
        assert_eq!(edited("x\n\n  - ab\n", |b| b.newline(Shown(5))), "x\n\n  - a\n  - b\n");
        assert_eq!(
            edited("x\n\n- ab\n\ny\n", |b| b.newline(Shown(5))),
            "x\n\n- a\n- b\n\ny\n"
        );
        assert_eq!(
            edited("x\n\n```\nab\n```\n", |b| b.newline(Shown(3))),
            "x\n\n```\na\nb\n```\n"
        );
        assert_eq!(edited("x\n\n> \n", |b| b.newline(Shown(2))), "x\n\n\n");
    }

    #[test]
    fn return_on_what_cannot_be_split_goes_behind_it() {
        assert_eq!(edited("---\n\nx\n", |b| b.newline(Shown(0))), "---\n\n\n\nx\n");
        assert_eq!(
            edited("<div>a</div>\n\nx\n", |b| b.newline(Shown(0))),
            "<div>a</div>\n\n\n\nx\n"
        );
        assert_eq!(edited("$$x$$\n\ny\n", |b| b.newline(Shown(0))), "$$x$$\n\n\n\ny\n");
    }

    #[test]
    fn typing_over_formatting_goes_where_the_selection_starts() {
        // A replacement: the text stays one edit, so it is undone in one step.
        let replaced = |t: &str, from: usize, to: usize| edited(t, |b| b.replace(Shown(from)..Shown(to), "X"));
        assert_eq!(replaced("a **bc** d\n", 3, 4), "a **bX** d\n");
        assert_eq!(
            replaced("a **bc** d\n", 2, 4),
            "a X d\n",
            "all of it goes, with its markup"
        );
        assert_eq!(replaced("a **bc** d\n", 1, 3), "aX**c** d\n");
        assert_eq!(
            replaced("a **bc** d\n", 3, 6),
            "a **bX**\n",
            "over the end of the formatting"
        );
        assert_eq!(replaced("ab\n\ncd\n", 1, 4), "aXd\n");
        assert_eq!(replaced("a ![b](c.png) d\n", 2, 3), "a X d\n");
    }

    #[test]
    fn the_caret_ends_behind_what_was_typed_over() {
        let t = "a **bc** d\n";
        let d = crate::markdown::doc::parse(t);
        let l = super::super::layout::layout(t, &d, &super::super::layout::NoImages, None);
        let index = Index::new(&l);
        let b = Bridge {
            text: t,
            doc: &d,
            layout: &l,
            index: &index,
        };
        let edit = b.replace(Shown(1)..Shown(3), "XY").unwrap();
        let caret = edit.selection.start;
        let mut out = t.to_string();
        edit.apply_to(&mut out);
        assert_eq!(&out[..caret], "aXY");
        let edit = b.replace(Shown(3)..Shown(3), "Z").unwrap();
        let caret = edit.selection.start;
        let mut out = t.to_string();
        edit.apply_to(&mut out);
        assert_eq!(&out[..caret], "a **bZ");
    }

    /// The source range of a selection of the preview of `t`.
    fn selected(t: &str, from: usize, to: usize) -> String {
        let d = crate::markdown::doc::parse(t);
        let l = super::super::layout::layout(t, &d, &super::super::layout::NoImages, None);
        let index = Index::new(&l);
        let b = Bridge {
            text: t,
            doc: &d,
            layout: &l,
            index: &index,
        };
        t[b.source_selection(Shown(from)..Shown(to))].to_string()
    }

    #[test]
    fn a_selection_of_the_preview_starts_in_its_first_character_and_counts_utf16() {
        let t = "ä😀 **fett** wort ![b](c.png) ende\n";
        // In the preview: ä, 😀 as two units, space, then `fett`.
        assert_eq!(selected(t, 0, 1), "ä");
        assert_eq!(selected(t, 1, 3), "😀");
        assert_eq!(selected(t, 4, 8), "fett");
        assert_eq!(selected(t, 6, 8), "tt");
        assert_eq!(selected(t, 9, 13), "** wort".trim_start_matches("** "));
        assert_eq!(selected(t, 14, 15), "![b](c.png)");
    }

    #[test]
    fn what_is_typed_in_code_stays_as_it_is() {
        let d = crate::markdown::doc::parse("```\nab\n```\n\ncd\n");
        let l = super::super::layout::layout("```\nab\n```\n\ncd\n", &d, &super::super::layout::NoImages, None);
        let index = Index::new(&l);
        let b = Bridge {
            text: "```\nab\n```\n\ncd\n",
            doc: &d,
            layout: &l,
            index: &index,
        };
        assert_eq!(b.escaped(Shown(1), "*", false).unwrap(), "*", "in a code block");
        assert_eq!(
            b.escaped(Shown(4), "*", true).unwrap(),
            "*",
            "when the caller knows it is code"
        );
        assert_eq!(b.escaped(Shown(4), "*", false).unwrap(), "\\*", "in running text");
    }

    #[test]
    fn the_caret_stays_behind_a_backtick_typed_in_a_code_span() {
        let t = "a `bc` d\n";
        let d = crate::markdown::doc::parse(t);
        let l = super::super::layout::layout(t, &d, &super::super::layout::NoImages, None);
        let index = Index::new(&l);
        let b = Bridge {
            text: t,
            doc: &d,
            layout: &l,
            index: &index,
        };
        let edit = b.replace(Shown(3)..Shown(3), "`").unwrap();
        let mut out = t.to_string();
        let caret = edit.selection.start;
        edit.apply_to(&mut out);
        assert_eq!(&out[..caret], "a ``b`");
        let at_edge = b.replace(Shown(2)..Shown(2), "`").unwrap();
        let mut out = t.to_string();
        at_edge.apply_to(&mut out);
        assert!(out.contains('`'), "{out}");
        assert_eq!(b.backticks_in_code_span(0, 0, "`"), None, "outside the span");
        assert_eq!(
            b.backticks_in_code_span(t.find('`').unwrap(), 4, "`"),
            None,
            "on its delimiter"
        );
    }

    #[test]
    fn return_numbers_the_next_item_and_ends_a_list_on_an_empty_one() {
        assert_eq!(edited("1. a\n", |b| b.newline(Shown(4))), "1. a\n2. \n");
        assert_eq!(edited("7) a\n", |b| b.newline(Shown(4))), "7) a\n8) \n");
        assert_eq!(edited("x\n\n3. a\n", |b| b.newline(Shown(6))), "x\n\n3. a\n4. \n");
        assert_eq!(edited("- a\n- \n", |b| b.newline(Shown(6))), "- a\n\n");
        assert_eq!(edited("x\n\n  - a\n  - \n", |b| b.newline(Shown(8))), "x\n\n  - a\n\n");
        assert_eq!(
            edited("> a\n>\n> \n", |b| b.replace(Shown(1)..Shown(1), "")),
            "> a\n>\n> \n"
        );
    }

    #[test]
    fn a_container_goes_with_its_content_only_when_all_of_it_is_deleted() {
        assert_eq!(
            edited("a **b** **c** d\n", |b| b.replace(Shown(2)..Shown(3), "")),
            "a  **c** d\n"
        );
        assert_eq!(edited("a [b](c) d\n", |b| b.replace(Shown(2)..Shown(3), "")), "a  d\n");
        assert_eq!(
            edited("x **a** y\n\nz **b** w\n", |b| b.replace(Shown(2)..Shown(3), "")),
            "x  y\n\nz **b** w\n"
        );
        assert_eq!(edited("# **a**\n", |b| b.replace(Shown(0)..Shown(1), "x")), "# x\n");
    }

    #[test]
    fn a_backtick_typed_in_a_code_span_lengthens_its_delimiters() {
        assert_eq!(
            edited("a `bc` d\n", |b| b.replace(Shown(3)..Shown(3), "`")),
            "a ``b`c`` d\n"
        );
        assert_eq!(
            edited("a ``b`c`` d\n", |b| b.replace(Shown(4)..Shown(4), "`")),
            "a ```b``c``` d\n"
        );
        assert_eq!(
            edited("a `bc` d\n", |b| b.replace(Shown(3)..Shown(4), "`")),
            "a `` b` `` d\n"
        );
    }

    #[test]
    fn typed_character_references_stay_text() {
        assert_eq!(edited("x\n", |b| b.replace(Shown(1)..Shown(1), "&amp;")), "x\\&amp;\n");
        assert_eq!(
            edited("x &amp\n", |b| b.replace(Shown(6)..Shown(6), ";")),
            "x &amp\\;\n"
        );
        assert_eq!(
            edited("x\n", |b| b.replace(Shown(1)..Shown(1), "& b; &keins;")),
            "x& b; &keins;\n"
        );
    }

    #[test]
    fn replacing_all_of_a_document_leaves_no_breaks_behind() {
        for t in [
            "Eins\n\nZwei\n\nDrei\n",
            "---\ntitle: x\n---\n\nEins\n\nZwei\n",
            "Eins\n\n---\n\nZwei\n",
            "Eins\n\n$$x$$\n",
        ] {
            let d = crate::markdown::doc::parse(t);
            let l = super::super::layout::layout(t, &d, &super::super::layout::NoImages, None);
            let shown: usize = l.blocks.iter().map(|b| b.len_u16).sum();
            let out = edited(t, |b| b.replace(Shown(0)..Shown(shown), "neu"));
            assert_eq!(out.trim_end(), "neu", "{t:?}");
        }
    }

    #[test]
    fn a_task_checkbox_toggles_both_ways() {
        assert_eq!(edited("- [ ] a\n", |b| b.toggle_task(Shown(0))), "- [x] a\n");
        assert_eq!(edited("- [x] a\n", |b| b.toggle_task(Shown(0))), "- [ ] a\n");
    }

    #[test]
    fn deleting_a_picture_or_formula_removes_its_whole_source() {
        assert_eq!(
            edited("a ![b](c.png) d\n", |b| b.replace(Shown(2)..Shown(3), "")),
            "a  d\n"
        );
        assert_eq!(edited("a $x^2$ d\n", |b| b.replace(Shown(2)..Shown(7), "")), "a  d\n");
    }

    #[test]
    fn deleting_all_of_a_container_takes_its_markup_along() {
        assert_eq!(edited("a **b** c\n", |b| b.replace(Shown(2)..Shown(3), "")), "a  c\n");
        assert_eq!(edited("a ***b*** c\n", |b| b.replace(Shown(2)..Shown(3), "")), "a  c\n");
        assert_eq!(edited("a `b` c\n", |b| b.replace(Shown(2)..Shown(3), "")), "a  c\n");
        assert_eq!(
            edited("a **bc** d\n", |b| b.replace(Shown(2)..Shown(3), "")),
            "a **c** d\n"
        );
    }

    #[test]
    fn the_chevron_of_an_indented_details_changes_its_opening_tag() {
        for (t, want) in [
            (
                "  <details>\n  <summary>Mehr</summary>\n  Inhalt\n  </details>\n",
                "  <details open>\n  <summary>Mehr</summary>\n  Inhalt\n  </details>\n",
            ),
            (
                " <details>\n<summary>Mehr</summary>\n\nInhalt\n</details>\n",
                " <details open>\n<summary>Mehr</summary>\n\nInhalt\n</details>\n",
            ),
        ] {
            let d = crate::markdown::doc::parse(t);
            let l = super::super::layout::layout(t, &d, &super::super::layout::NoImages, None);
            let index = Index::new(&l);
            let b = Bridge {
                text: t,
                doc: &d,
                layout: &l,
                index: &index,
            };
            let mut out = t.to_string();
            b.toggle_fold(Shown(0)).unwrap().apply_to(&mut out);
            assert_eq!(out, want);
        }
    }

    #[test]
    fn only_a_whole_details_tag_is_ever_toggled() {
        assert_eq!(toggle_open("<details>", false), Some((8..8, " open".to_string())));
        assert_eq!(toggle_open("<details>\n</details>", false), None);
        assert_eq!(toggle_open("<summary>", false), None);
        assert_eq!(toggle_open("nichts", false), None);
    }

    #[test]
    fn typing_after_a_named_entity_lands_behind_it() {
        let t = "Caf&eacute; au lait\n";
        let d = crate::markdown::doc::parse(t);
        let l = super::super::layout::layout(t, &d, &super::super::layout::NoImages, None);
        let index = Index::new(&l);
        let b = Bridge {
            text: t,
            doc: &d,
            layout: &l,
            index: &index,
        };
        let mut out = t.to_string();
        b.replace(Shown(5)..Shown(7), "X").unwrap().apply_to(&mut out);
        assert_eq!(out, "Caf&eacute; X lait\n");
    }

    #[test]
    fn replacing_many_formatted_pieces_stays_one_edit_and_drops_their_markup() {
        let t: String = (0..3000).map(|i| format!("w{i} **b{i}** ")).collect::<String>() + "\n";
        let d = crate::markdown::doc::parse(&t);
        let l = super::super::layout::layout(&t, &d, &super::super::layout::NoImages, None);
        let index = Index::new(&l);
        let b = Bridge {
            text: &t,
            doc: &d,
            layout: &l,
            index: &index,
        };
        let shown: usize = l.blocks[0].paras[0]
            .runs
            .iter()
            .map(|r| r.text.encode_utf16().count())
            .sum();
        let mut out = t.clone();
        b.replace(Shown(0)..Shown(shown), "x").unwrap().apply_to(&mut out);
        assert!(!out.contains("**"), "{}", &out[..out.len().min(80)]);
        assert!(out.starts_with('x'));
    }
    use crate::markdown::doc::parse;

    #[test]
    fn details_open_and_close_in_their_tag() {
        let apply = |tag: &str, open: bool| {
            let (r, with) = toggle_open(tag, open).unwrap();
            format!("{}{with}{}", &tag[..r.start], &tag[r.end..])
        };
        assert_eq!(apply("<details>", false), "<details open>");
        assert_eq!(apply("<details open>", true), "<details>");
        assert_eq!(
            apply("<details class=\"x\" open=\"open\">", true),
            "<details class=\"x\">"
        );
        assert_eq!(apply("<details OPEN data-opener=1>", true), "<details data-opener=1>");
        assert!(closes_shortcode("so :smile", ""));
        assert!(!closes_shortcode("Uhr", ""));
    }
    use crate::preview::layout::{NoImages, layout};

    fn edit(t: &str, f: impl Fn(&Bridge) -> Option<Edit>) -> (String, usize) {
        let d = parse(t);
        let l = layout(t, &d, &NoImages, None);
        let i = Index::new(&l);
        let b = Bridge {
            text: t,
            doc: &d,
            layout: &l,
            index: &i,
        };
        let e = f(&b).expect("edit");
        let mut s = t.to_string();
        e.apply_to(&mut s);
        (s, e.selection.start)
    }

    #[test]
    fn typing_in_the_preview_at_pending_styles_wraps_the_word() {
        let pending = Pending {
            at: 8,
            styles: vec![commands::InlineStyle::Bold],
        };
        let typed = |p: usize, typed: &str| {
            let t = "ein Wort\n";
            let d = parse(t);
            let l = layout(t, &d, &NoImages, None);
            let i = Index::new(&l);
            let b = Bridge {
                text: t,
                doc: &d,
                layout: &l,
                index: &i,
            };
            b.type_with_pending(&pending, Shown(p), typed).map(|(e, next)| {
                let mut s = t.to_string();
                e.apply_to(&mut s);
                (s, next.map(|n| n.at))
            })
        };
        assert_eq!(typed(8, "x"), Some(("ein Wort**x**\n".into(), None)));
        assert_eq!(typed(8, " "), Some(("ein Wort \n".into(), Some(9))));
        assert_eq!(typed(8, "\n"), None);
        assert_eq!(typed(3, "x"), None);
    }

    #[test]
    fn typing_escapes_markdown_characters() {
        let (s, caret) = edit("ab\n", |b| b.replace(Shown(1)..Shown(1), "*"));
        assert_eq!(s, "a\\*b\n");
        assert_eq!(caret, 3);
        let (s, _) = edit("x\n", |b| b.replace(Shown(0)..Shown(0), "# "));
        assert_eq!(s, "\\# x\n");
        let (s, _) = edit("snake\n", |b| b.replace(Shown(5)..Shown(5), "_case"));
        assert_eq!(s, "snake_case\n");
        // Typed in the preview, none of the newer markup may take effect.
        for (typed, stored) in [
            ("==x== a = b", "\\=\\=x\\=\\= a = b"),
            ("x^2^", "x\\^2\\^"),
            ("||s|| a|b", "\\|\\|s\\|\\| a|b"),
            ("%%c%% 5%", "\\%\\%c\\%\\% 5%"),
            (":smile: 12:30", ":smile\\: 12:30"),
        ] {
            let (s, _) = edit("A\n", |b| b.replace(Shown(1)..Shown(1), typed));
            assert_eq!(s, format!("A{stored}\n"));
            let parsed = parse(&s);
            let l = layout(&s, &parsed, &NoImages, None);
            let shown: String = l.blocks[0].paras[0].runs.iter().map(|r| r.text.as_str()).collect();
            assert_eq!(shown, format!("A{typed}"), "{s}");
        }
        let (s, _) = edit("# Titel\n", |b| b.replace(Shown(5)..Shown(5), " {#x}"));
        assert_eq!(s, "# Titel \\{#x}\n");
    }

    #[test]
    fn typing_after_a_space_at_the_paragraph_end_keeps_the_space() {
        let t = "Wort \n";
        let d = parse(t);
        let l = layout(t, &d, &NoImages, Some(5));
        let i = Index::new(&l);
        let b = Bridge {
            text: t,
            doc: &d,
            layout: &l,
            index: &i,
        };
        let mut s = t.to_string();
        b.replace(Shown(5)..Shown(5), "z").expect("edit").apply_to(&mut s);
        assert_eq!(s, "Wort z\n");
    }

    #[test]
    fn deleting_whole_bold_content_removes_delimiters() {
        let (s, _) = edit("a **b** c\n", |b| b.replace(Shown(2)..Shown(3), ""));
        assert_eq!(s, "a  c\n");
    }

    #[test]
    fn enter_continues_lists_and_splits_bold() {
        let (s, caret) = edit("- eins\n", |b| b.newline(Shown(6)));
        assert_eq!(s, "- eins\n- \n");
        assert_eq!(caret, 9);
        let (s, _) = edit("1. a\n", |b| b.newline(Shown(4)));
        assert_eq!(s, "1. a\n2. \n");
        let (s, _) = edit("x **fett**\n", |b| b.newline(Shown(4)));
        assert_eq!(s, "x **fe**\n\n**tt**\n");
        let (s, _) = edit("- [x] a\n", |b| b.newline(Shown(3)));
        assert_eq!(s, "- [x] a\n- [ ] \n");
    }

    #[test]
    fn return_adds_visible_empty_lines_and_backspace_removes_them() {
        let (s, caret) = edit("A\n\nB\n", |b| b.newline(Shown(1)));
        assert_eq!((s.as_str(), caret), ("A\n\n\n\nB\n", 3));
        let (s, caret) = edit(&s, |b| b.newline(Shown(2)));
        assert_eq!((s.as_str(), caret), ("A\n\n\n\n\n\nB\n", 5));
        let (s, caret) = edit(&s, |b| b.delete_at_block_start(Shown(3)));
        assert_eq!((s.as_str(), caret), ("A\n\n\n\nB\n", 3));
        let (s, _) = edit(&s, |b| b.replace(Shown(2)..Shown(2), "neu"));
        assert_eq!(s, "A\n\nneu\n\nB\n");
    }

    #[test]
    fn enter_on_empty_item_leaves_list() {
        let (s, _) = edit("- a\n- \n", |b| b.newline(Shown(6)));
        assert_eq!(s, "- a\n\n");
    }

    #[test]
    fn enter_in_quote_and_table() {
        let (s, _) = edit("> ab\n", |b| b.newline(Shown(1)));
        assert_eq!(s, "> a\n>\n> b\n");
        let (s, caret) = edit("| a | b |\n|---|---|\n| 1 | 2 |\n", |b| b.newline(Shown(5)));
        assert_eq!(s.lines().count(), 4);
        assert!(caret > s.find("| 1").unwrap());
    }

    #[test]
    fn backspace_at_start_removes_block_prefix() {
        let (s, _) = edit("## Titel\n", |b| b.delete_at_block_start(Shown(0)));
        assert_eq!(s, "Titel\n");
        let (s, _) = edit("- a\n", |b| b.delete_at_block_start(Shown(2)));
        assert_eq!(s, "a\n");
    }

    #[test]
    fn task_toggle_and_tab_indent() {
        let (s, _) = edit("- [ ] a\n", |b| b.toggle_task(Shown(0)));
        assert_eq!(s, "- [x] a\n");
        let (s, _) = edit("- a\n- b\n", |b| b.tab(Shown(6), false));
        assert_eq!(s, "- a\n  - b\n");
        let (s, _) = edit("- a\n  - b\n", |b| b.tab(Shown(6), true));
        assert_eq!(s, "- a\n- b\n");
    }

    #[test]
    fn selection_maps_to_source() {
        let t = "a **fett** b\n";
        let d = parse(t);
        let l = layout(t, &d, &NoImages, None);
        let i = Index::new(&l);
        let b = Bridge {
            text: t,
            doc: &d,
            layout: &l,
            index: &i,
        };
        let r = b.source_selection(Shown(2)..Shown(6));
        assert_eq!(&t[r], "fett");
    }

    fn edit_of(t: &str, edit: impl Fn(&Bridge) -> Option<Edit>) -> Option<Edit> {
        let d = crate::markdown::doc::parse(t);
        let l = super::super::layout::layout(t, &d, &super::super::layout::NoImages, None);
        let index = Index::new(&l);
        edit(&Bridge {
            text: t,
            doc: &d,
            layout: &l,
            index: &index,
        })
    }

    /// The source after `edit` of the preview of `t`, with `|` for the caret.
    fn with_caret(t: &str, edit: impl Fn(&Bridge) -> Option<Edit>) -> String {
        let edit = edit_of(t, edit).expect("an edit");
        let mut out = t.to_string();
        edit.apply_to(&mut out);
        out.insert(edit.selection.start, '|');
        out
    }

    #[test]
    fn a_backtick_typed_in_a_code_span_lengthens_the_span_it_is_in() {
        let typed = |t: &str, p: usize| with_caret(t, |b| b.replace(Shown(p)..Shown(p), "`"));
        assert_eq!(typed("`ab` c", 0), "`` `|ab `` c", "at its start");
        assert_eq!(typed("xx `ab` yy", 4), "xx ``a`|b`` yy", "inside");
        assert_eq!(typed("`a` `b`", 1), "`` a`| `` `b`", "not the next one");
    }

    #[test]
    fn return_in_an_empty_item_or_quote_clears_the_whole_line() {
        let enter = |t: &str, p: usize| with_caret(t, |b| b.newline(Shown(p)));
        assert_eq!(
            enter("x\n\n-   \n\nnext\n", 4),
            "x\n\n|\n\nnext\n",
            "spaces behind the marker"
        );
        assert_eq!(enter("x\n\n> - \n\nnext\n", 4), "x\n\n>|\n\nnext\n", "the quote stays");
        assert_eq!(enter("x\n\n>   \n\nnext\n", 2), "x\n\n|\n\nnext\n", "an empty quote");
    }

    #[test]
    fn return_inside_bold_in_a_list_leaves_the_caret_before_the_new_markers() {
        assert_eq!(with_caret("- **ab**\n", |b| b.newline(Shown(3))), "- **a**\n- |**b**\n");
    }

    #[test]
    fn backspace_takes_a_quoted_heading_marker_and_leaves_quote_marks_in_code() {
        assert_eq!(
            with_caret("x\n\n> # Titel\n", |b| b.delete_at_block_start(Shown(2))),
            "x\n\n> |Titel\n"
        );
        assert_eq!(
            edited("```\n> a\n```\n", |b| b.delete_at_block_start(Shown(0))),
            "```\n> a\n```\n"
        );
    }

    #[test]
    fn a_click_on_details_leaves_the_caret_behind_the_change() {
        let t = "x\n\n<details>\n<summary>Mehr</summary>\n\nInhalt\n\n</details>\n";
        let chevron = 2;
        assert_eq!(
            with_caret(t, |b| b.toggle_fold(Shown(chevron))),
            "x\n\n<details open|>\n<summary>Mehr</summary>\n\nInhalt\n\n</details>\n"
        );
    }

    #[test]
    fn a_click_before_a_chevron_folds_nothing() {
        let t = "x\n\n> [!note]- Titel\n> Inhalt\n";
        let chevron = 2 + 1 + " Titel".len();
        assert_eq!(edit_of(t, |b| b.toggle_fold(Shown(2))), None, "the icon");
        assert_eq!(
            edit_of(t, |b| b.toggle_fold(Shown(chevron - 1))),
            None,
            "the title's end"
        );
    }

    #[test]
    #[expect(clippy::single_range_in_vec_init, reason = "lists of one deletion")]
    fn an_insertion_inside_a_deletion_lands_at_its_start_and_later_ones_stay_where_they_are() {
        let composed = |deletions: &[Range<usize>], at: usize| {
            let edit = compose("abcdefgh", deletions, at, "X");
            let mut out = "abcdefgh".to_string();
            edit.apply_to(&mut out);
            out.insert(edit.selection.start, '|');
            out
        };
        assert_eq!(composed(&[1..3, 5..6], 2), "aX|degh", "in the first of two");
        assert_eq!(composed(&[1..2], 4), "acdX|efgh", "behind it");
        assert_eq!(composed(&[3..5], 1), "aX|bcfgh", "before it");
    }

    #[test]
    fn typed_punctuation_alone_at_the_start_of_a_paragraph_needs_no_backslash() {
        assert_eq!(edited("x\n", |b| b.replace(Shown(0)..Shown(0), ".")), ".x\n");
        assert_eq!(edited("x\n", |b| b.replace(Shown(0)..Shown(0), ")")), ")x\n");
    }

    #[test]
    fn a_quoted_open_value_goes_with_its_spaces() {
        let (range, replacement) = toggle_open("<details open=\"a b\" c>", true).unwrap();
        assert_eq!((range, replacement.as_str()), (8..19, ""));
    }

    #[test]
    fn typing_and_deleting_at_the_end_of_code_stays_inside_the_block() {
        let typed = |t: &str, p: usize| edited(t, |b| b.replace(Shown(p)..Shown(p), "c"));
        assert_eq!(typed("```\nab\n```\n", 2), "```\nabc\n```\n");
        assert_eq!(typed("~~~\nab\n~~~\n", 2), "~~~\nabc\n~~~\n");
        assert_eq!(typed("> ```\n> ab\n> ```\n", 2), "> ```\n> abc\n> ```\n");
        assert_eq!(typed("- ```\n  ab\n  ```\n", 2 + 2), "- ```\n  abc\n  ```\n");
        assert_eq!(typed("    ab\n", 2), "    abc\n");
        assert_eq!(typed("```\nab\n", 2), "```\nabc\n", "without a closing fence");
        let deleted = |t: &str, r: Range<usize>| edited(t, |b| b.replace(Shown(r.start)..Shown(r.end), ""));
        assert_eq!(deleted("```\nab\n```\n", 1..2), "```\na\n```\n");
        assert_eq!(deleted("```\nab\ncd\n```\n", 4..5), "```\nab\nc\n```\n");
        assert_eq!(deleted("x\n\n```\nab\n```\n\ny\n", 3..4), "x\n\n```\na\n```\n\ny\n");
        assert_eq!(deleted("> ```\n> ab\n> ```\n", 1..2), "> ```\n> a\n> ```\n");
        assert_eq!(edited("```\nab\n```\n", |b| b.newline(Shown(2))), "```\nab\n\n```\n");
    }
}
