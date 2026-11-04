//! The keys that act on blocks: Return, Shift-Return, Tab and Backspace at the start of a block, and clicks on what folds or
//! checks.

use std::ops::Range;

use crate::layout::{Atom, Marker, ParaKind, RunMap};
use crate::map::Shown;
use splat_markdown::commands::{Edit, LinePrefix, leaf_at, line_span};
use splat_markdown::doc::{BlockKind, Inline, InlineKind, ListKind, block_path, inline_path, walk_inlines};
use splat_markdown::table;
use splat_markdown::text::{Byte, Direction, TextAt, line_end, line_start, lines_with_ends, shifted, signed, units};

use super::{Bridge, compose, continuation_prefix};

impl Bridge<'_> {
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
            return table::move_down(self.doc, at);
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
                let keep = self.text.at(line_begin..marker.start);
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
            let src_marker = self.text.at(marker.clone());
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
            let indent = self.text.at(line_begin..marker.start);
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
            self.index
                .para(self.layout, i)
                .runs
                .iter()
                .all(|r| matches!(r.map, RunMap::Atom { atom: Atom::Marker, .. }))
        })
    }

    /// Where a paragraph splits at `at`, with the delimiters that close and reopen the containers around it, so a split
    /// keeps formatting. At the very start or end of a container it splits outside of it: inside, one side would hold
    /// nothing but its delimiters.
    pub(super) fn split_inlines(&self, at: Byte) -> (Byte, String, String) {
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
                open.push_str(self.text.at(span.range.start..content.start));
                close.push_str(self.text.at(content.end..span.range.end));
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
                open.insert_str(0, self.text.at(i.range.start..content.start));
                close.push_str(self.text.at(content.end..i.range.end));
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
    pub fn tab(&self, p: Shown, way: Direction) -> Option<Edit> {
        let point = self.point(p)?;
        if let ParaKind::TableCell { .. } = point.para_kind {
            return table::navigate(self.doc, point.src, way);
        }
        let path = block_path(&self.doc.blocks, point.src);
        let item_pos = path.iter().rposition(|b| matches!(b.kind, BlockKind::Item { .. }))?;
        let BlockKind::Item { marker, .. } = &path[item_pos].kind else {
            return None;
        };
        // Indentation counts from behind the marks of a quote around the list; a quote in the item moves with it.
        let content_start = |at: Byte| {
            let line = line_start(self.text, at);
            line + LinePrefix::parse(self.text.from(line)).quote_end
        };
        let line_begin = content_start(marker.start);
        let quotes = self
            .text
            .at(line_start(self.text, marker.start)..line_begin)
            .matches('>')
            .count();
        // In front of this marker may stand another list's, as in `- - a`, or a byte order mark.
        if !self
            .text
            .at(line_begin..marker.start)
            .chars()
            .all(|c| matches!(c, ' ' | '\t'))
        {
            return None;
        }
        let current_indent = marker.start - line_begin;
        // What belongs to the item moves with it, or a nested list would end up beside it.
        let item_lines = line_span(self.text, &(marker.start..path[item_pos].range.end));
        let several_lines = self.text.at(item_lines.clone()).contains(['\n', '\r']);
        if way == Direction::Backward {
            let parent = path[..item_pos].iter().rev().find_map(|b| match &b.kind {
                BlockKind::Item { marker, .. } => Some(marker.start - content_start(marker.start)),
                _ => None,
            })?;
            let remove = current_indent - parent.min(current_indent);
            if several_lines {
                return Some(self.moved_lines(item_lines, point.src, -signed(remove), quotes));
            }
            let edit = Edit {
                range: line_begin..line_begin + remove,
                replacement: String::new(),
                selection: point.src..point.src,
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
        if several_lines {
            return Some(self.moved_lines(item_lines, point.src, signed(add), quotes));
        }
        let spaces = " ".repeat(add);
        let caret = point.src + add;
        Some(Edit {
            range: line_begin..line_begin,
            replacement: spaces,
            selection: caret..caret,
        })
    }

    /// `lines` moved right by `columns` behind the marks of the `quotes` quotes around them, or left as far as their
    /// indentation allows; blank lines stay as they are and the caret goes along.
    fn moved_lines(&self, lines: Range<Byte>, caret: Byte, columns: isize, quotes: usize) -> Edit {
        let mut out = String::with_capacity(units(&lines));
        let mut at = lines.start;
        let mut moved_caret = caret;
        for (line, ending) in lines_with_ends(self.text.at(lines.clone())) {
            let quote = LinePrefix::quote_end_of(line, quotes);
            let content = &line[quote..];
            out.push_str(&line[..quote]);
            let content_start = at + quote;
            if content.trim().is_empty() {
                out.push_str(content);
            } else if columns > 0 {
                out.extend(std::iter::repeat_n(' ', columns.unsigned_abs()));
                out.push_str(content);
                if caret >= content_start {
                    moved_caret += columns.unsigned_abs();
                }
            } else {
                let indent = content.len() - content.trim_start_matches([' ', '\t']).len();
                let removed = columns.unsigned_abs().min(indent);
                out.push_str(&content[removed..]);
                moved_caret -= removed.min(caret.get().saturating_sub(content_start.get()));
            }
            out.push_str(ending);
            at += line.len() + ending.len();
        }
        Edit {
            range: lines,
            replacement: out,
            selection: moved_caret..moved_caret,
        }
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
        if para.runs.is_empty() && para.src.is_empty() && self.text.upto(point.src).ends_with("\n\n") {
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
                let end = end + usize::from(self.text.from(end).starts_with(' '));
                let caret = marker.start;
                return Some(Edit {
                    range: marker.start..end.max(marker.start),
                    replacement: String::new(),
                    selection: caret..caret,
                });
            }
        }
        let line_begin = line_start(self.text, point.src);
        let prefix = LinePrefix::parse(self.text.from(line_begin));
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
            let line = self.text.from(line_begin);
            let gt = line.find('>')?;
            let end = gt + 1 + usize::from(line[gt + 1..].starts_with(' '));
            let caret = shifted(point.src, -signed(end)).max(line_begin);
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
        let para = self.index.para(self.layout, para_index);
        let (_, run) = para
            .runs_at(self.index.paras[para_index].start)
            .find(|(at, _)| at.contains(&p))?;
        match &run.map {
            RunMap::Atom {
                atom: Atom::Fold { open, sign },
                ..
            } => {
                let sign = (*sign)?;
                Some(Edit {
                    range: sign..sign + 1,
                    replacement: if *open { "-" } else { "+" }.to_string(),
                    selection: sign + 1..sign + 1,
                })
            }
            RunMap::Atom {
                atom: Atom::Details { open },
                src,
            } => {
                let (range, replacement) = toggle_open(self.text.at(src.clone()), *open)?;
                let range = src.start + range.start..src.start + range.end;
                let caret = range.start + replacement.len();
                Some(Edit {
                    range,
                    replacement,
                    selection: caret..caret,
                })
            }
            _ => None,
        }
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
        if symbol >= self.text.end() {
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

/// The change to a `<details>` tag that adds or removes its `open` attribute.
pub(super) fn toggle_open(tag: &str, open: bool) -> Option<(Range<usize>, String)> {
    let is_details = matches!(
        splat_markdown::markup::read_tag(tag, 0),
        Some((splat_markdown::markup::Tag::Open { name, .. }, end)) if name == "details" && end == tag.len()
    );
    if !is_details {
        return None;
    }
    if !open {
        let end = if tag.ends_with("/>") {
            tag.len() - 2
        } else {
            tag.len().checked_sub(1)?
        };
        return Some((end..end, " open".to_string()));
    }
    let attribute = splat_markdown::markup::attribute_range(tag, "open")?;
    Some((tag[..attribute.start].trim_end().len()..attribute.end, String::new()))
}
