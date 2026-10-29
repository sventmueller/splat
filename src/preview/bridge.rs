use std::ops::Range;

use crate::markdown::commands::{self, Edit, LinePrefix, Pending, leaf_at};
use crate::markdown::doc::{
    Block, BlockKind, Doc, Inline, InlineKind, ListKind, block_path, inline_path, walk_inlines,
};
use crate::markdown::table;

use super::layout::{Layout, Marker, ParaKind, RunMap};
use super::map::{Index, InsertPoint, deletion_ranges, insert_point, normalize};

/// Translates edits made in the rendered preview into edits of the Markdown source.
pub struct Bridge<'a> {
    pub text: &'a str,
    pub doc: &'a Doc,
    pub layout: &'a Layout,
    pub index: &'a Index,
}

impl Bridge<'_> {
    fn point(&self, p: usize) -> Option<InsertPoint> {
        insert_point(self.layout, self.index, p)
    }

    /// Source range for a preview selection: the start snaps into the first selected character.
    pub fn source_selection(&self, range: Range<usize>) -> Range<usize> {
        let end = self.point(range.end).map_or(0, |p| p.src);
        if range.is_empty() {
            return end..end;
        }
        let start = self.point(range.start + 1).and_then(|after| {
            let pi = self.index.para_at(range.start)?;
            let pp = self.index.paras[pi];
            let para = self.index.para(self.layout, pi);
            let off = range.start - pp.start;
            let mut u = 0;
            for run in &para.runs {
                let len = run.text.encode_utf16().count();
                if off >= u && off < u + len {
                    return Some(match &run.map {
                        RunMap::Text { chars, end } => {
                            let mut k = 0;
                            let mut acc = u;
                            for c in run.text.chars() {
                                if acc >= off {
                                    break;
                                }
                                acc += c.len_utf16();
                                k += 1;
                            }
                            chars.get(&run.text, k).unwrap_or(*end)
                        }
                        RunMap::Atom { src, .. } => src.start,
                    });
                }
                u += len;
            }
            Some(after.src)
        });
        let start = start.unwrap_or(end);
        start.min(end)..end
    }

    /// Source offset that text typed at preview offset `p` goes to.
    pub fn source_at(&self, p: usize) -> Option<usize> {
        self.point(p).map(|ip| ip.src)
    }

    /// Text typed at preview offset `p` while styles are pending at the source caret, as a source edit, plus the
    /// pending styles that still wait for the next word when only whitespace was typed.
    pub fn type_with_pending(&self, pending: &Pending, p: usize, typed: &str) -> Option<(Edit, Option<Pending>)> {
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
    pub fn escaped(&self, p: usize, typed: &str, code: bool) -> Option<String> {
        let mut ip = self.point(p)?;
        ip.code = code || matches!(ip.para_kind, ParaKind::Code { .. });
        Some(escape(typed, &ip, self.text))
    }

    /// Replaces a preview range with typed or pasted text.
    pub fn replace(&self, range: Range<usize>, typed: &str) -> Option<Edit> {
        let ip = self.point(range.start)?;
        let mut deletions = if range.is_empty() {
            vec![]
        } else {
            deletion_ranges(self.layout, self.index, range.clone())
        };
        if !deletions.is_empty() {
            deletions = self.drop_empty_containers(deletions);
        }
        let insert = escape(typed, &ip, self.text);
        Some(compose(self.text, &deletions, ip.src, &insert))
    }

    /// Adds the full range of every inline container whose content would be deleted completely.
    fn drop_empty_containers(&self, mut deletions: Vec<Range<usize>>) -> Vec<Range<usize>> {
        let covered =
            |r: &Range<usize>, dels: &[Range<usize>]| dels.iter().any(|d| d.start <= r.start && r.end <= d.end);
        loop {
            let mut grown = false;
            let leaves: Vec<&Block> = deletions.iter().filter_map(|d| leaf_at(self.doc, d.start)).collect();
            for leaf in leaves {
                let mut extra = Vec::new();
                walk_inlines(&leaf.inlines, &mut |i: &Inline| {
                    let content = match i.kind {
                        InlineKind::Code(_) => {
                            let src = &self.text[i.range.clone()];
                            let ticks = src.bytes().take_while(|&b| b == b'`').count();
                            i.range.start + ticks..i.range.end - ticks
                        }
                        _ if i.is_container() => i.content_range(),
                        _ => return,
                    };
                    if !content.is_empty() && covered(&content, &deletions) && !covered(&i.range, &deletions) {
                        extra.push(i.range.clone());
                    }
                });
                if !extra.is_empty() {
                    grown = true;
                    deletions.extend(extra);
                }
            }
            deletions = normalize(deletions);
            if !grown {
                return deletions;
            }
        }
    }

    /// Return key: splits paragraphs, continues lists and quotes, moves down in tables.
    pub fn newline(&self, p: usize) -> Option<Edit> {
        let ip = self.point(p)?;
        let at = ip.src;
        if ip.html
            || matches!(
                ip.para_kind,
                ParaKind::Rule | ParaKind::MathBlock | ParaKind::Diagram | ParaKind::FrontMatter
            )
        {
            let block = block_path(&self.doc.blocks, at).into_iter().last()?;
            return Some(compose(self.text, &[], block.range.end, "\n\n"));
        }
        if let ParaKind::TableCell { .. } = ip.para_kind {
            return table::move_down(self.text, self.doc, at);
        }
        let prefix = continuation_prefix(self.text, at);
        if let ParaKind::Code { .. } = ip.para_kind {
            return Some(compose(self.text, &[], at, &format!("\n{prefix}")));
        }
        let path = block_path(&self.doc.blocks, at);
        let para_empty = self.para_is_empty(p);
        if let Some(item) = path.iter().rev().find(|b| matches!(b.kind, BlockKind::Item { .. })) {
            let BlockKind::Item { marker, task } = &item.kind else {
                unreachable!()
            };
            if para_empty {
                let line_start = self.text[..marker.start].rfind('\n').map_or(0, |i| i + 1);
                let line_end = self.text[at..].find('\n').map_or(self.text.len(), |i| at + i);
                let keep = &self.text[line_start..marker.start];
                let keep = keep.trim_end_matches(' ');
                let replacement = keep.to_string();
                let caret = line_start + replacement.len();
                return Some(Edit {
                    range: line_start..line_end,
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
            let line_start = self.text[..marker.start].rfind('\n').map_or(0, |i| i + 1);
            let indent = &self.text[line_start..marker.start];
            let (close, open) = self.split_inlines(at);
            let insert = format!("{close}\n{indent}{new_marker}{task_box} {open}");
            return Some(compose(self.text, &[], at, &insert).with_caret_before(open.len()));
        }
        let in_quote = path.iter().any(|b| matches!(b.kind, BlockKind::BlockQuote));
        if in_quote && para_empty {
            let line_start = self.text[..at].rfind('\n').map_or(0, |i| i + 1);
            let line_end = self.text[at..].find('\n').map_or(self.text.len(), |i| at + i);
            let caret = line_start;
            return Some(Edit {
                range: line_start..line_end,
                replacement: String::new(),
                selection: caret..caret,
            });
        }
        let (close, open) = self.split_inlines(at);
        let quote = prefix.trim_end();
        let insert = if in_quote {
            format!("{close}\n{quote}\n{prefix}{open}")
        } else {
            format!("{close}\n\n{prefix}{open}")
        };
        Some(compose(self.text, &[], at, &insert).with_caret_before(open.len()))
    }

    fn para_is_empty(&self, p: usize) -> bool {
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

    /// Closing and reopening delimiters of the inline containers around `at`, so a split keeps formatting.
    fn split_inlines(&self, at: usize) -> (String, String) {
        let Some(leaf) = leaf_at(self.doc, at) else {
            return (String::new(), String::new());
        };
        let path = inline_path(&leaf.inlines, at);
        let mut close = String::new();
        let mut open = String::new();
        for i in path.iter().filter(|i| i.range.start < at && at < i.range.end) {
            if matches!(i.kind, InlineKind::Link { .. } | InlineKind::Image { .. }) {
                continue;
            }
            let content = i.content_range();
            if content.start < i.range.start || content.end > i.range.end {
                continue;
            }
            open.push_str(&self.text[i.range.start..content.start]);
            close.insert_str(0, &self.text[content.end..i.range.end]);
        }
        (close, open)
    }

    /// Shift-Return: a hard line break inside the paragraph.
    pub fn line_break(&self, p: usize) -> Option<Edit> {
        let ip = self.point(p)?;
        let prefix = continuation_prefix(self.text, ip.src);
        let insert = if ip.code {
            format!("\n{prefix}")
        } else {
            format!("\\\n{prefix}")
        };
        Some(compose(self.text, &[], ip.src, &insert))
    }

    /// Tab and Shift-Tab: next/previous table cell, or indent/outdent a list item.
    pub fn tab(&self, p: usize, backwards: bool) -> Option<Edit> {
        let ip = self.point(p)?;
        if let ParaKind::TableCell { .. } = ip.para_kind {
            return table::navigate(self.text, self.doc, ip.src, backwards);
        }
        let path = block_path(&self.doc.blocks, ip.src);
        let item_pos = path.iter().rposition(|b| matches!(b.kind, BlockKind::Item { .. }))?;
        let BlockKind::Item { marker, .. } = &path[item_pos].kind else {
            return None;
        };
        let line_start = self.text[..marker.start].rfind('\n').map_or(0, |i| i + 1);
        let current_indent = marker.start - line_start;
        if backwards {
            let parent = path[..item_pos].iter().rev().find_map(|b| match &b.kind {
                BlockKind::Item { marker, .. } => {
                    Some(marker.start - self.text[..marker.start].rfind('\n').map_or(0, |i| i + 1))
                }
                _ => None,
            })?;
            let remove = current_indent - parent.min(current_indent);
            let edit = Edit {
                range: line_start..line_start + remove,
                replacement: String::new(),
                selection: 0..0,
            };
            let caret = ip.src - remove;
            return Some(Edit {
                selection: caret..caret,
                ..edit
            });
        }
        let list = path[..item_pos]
            .iter()
            .rev()
            .find(|b| matches!(b.kind, BlockKind::List { .. }))?;
        let index = list.children.iter().position(|c| c.range == path[item_pos].range)?;
        let previous = list.children.get(index.checked_sub(1)?)?;
        let BlockKind::Item {
            marker: prev_marker, ..
        } = &previous.kind
        else {
            return None;
        };
        let prev_line = self.text[..prev_marker.start].rfind('\n').map_or(0, |i| i + 1);
        let content_col = prev_marker.end + 1 - prev_line;
        let add = content_col.saturating_sub(current_indent).max(1);
        let spaces = " ".repeat(add);
        let caret = ip.src + add;
        Some(Edit {
            range: line_start..line_start,
            replacement: spaces,
            selection: caret..caret,
        })
    }

    /// Backspace at the start of a block removes the block formatting instead of joining paragraphs.
    pub fn delete_at_block_start(&self, p: usize) -> Option<Edit> {
        let ip = self.point(p)?;
        if !ip.at_para_start {
            return None;
        }
        let pi = self.index.para_at(p)?;
        let para = self.index.para(self.layout, pi);
        match &para.style.kind {
            ParaKind::TableCell { .. } | ParaKind::Code { .. } => {
                return Some(Edit {
                    range: ip.src..ip.src,
                    replacement: String::new(),
                    selection: ip.src..ip.src,
                });
            }
            _ => {}
        }
        if para.runs.is_empty() && para.src.is_empty() && self.text[..ip.src].ends_with("\n\n") {
            let at = ip.src - 2;
            return Some(Edit {
                range: at..ip.src,
                replacement: String::new(),
                selection: at..at,
            });
        }
        if para.style.marker.is_some() {
            let path = block_path(&self.doc.blocks, ip.src);
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
        let line_start = self.text[..ip.src].rfind('\n').map_or(0, |i| i + 1);
        let prefix = LinePrefix::parse(&self.text[line_start..]);
        if let Some((r, _)) = prefix.heading {
            let range = line_start + r.start..line_start + r.end;
            let caret = range.start;
            return Some(Edit {
                range,
                replacement: String::new(),
                selection: caret..caret,
            });
        }
        if prefix.quote_end > 0 && para.style.quote_depth > 0 {
            let line = &self.text[line_start..];
            let gt = line.find('>')?;
            let end = gt + 1 + usize::from(line[gt + 1..].starts_with(' '));
            let caret = ip.src - end;
            return Some(Edit {
                range: line_start..line_start + end,
                replacement: String::new(),
                selection: caret..caret,
            });
        }
        None
    }

    /// Toggles the checkbox of the task item whose marker sits in the paragraph at `p`.
    /// A click on a callout's chevron flips its `-` and `+`, which folds or unfolds it.
    pub fn toggle_fold(&self, p: usize) -> Option<Edit> {
        let pi = self.index.para_at(p)?;
        let offset = p.checked_sub(self.index.paras[pi].start)?;
        let mut at = 0;
        for run in &self.index.para(self.layout, pi).runs {
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

    pub fn toggle_task(&self, p: usize) -> Option<Edit> {
        let pi = self.index.para_at(p)?;
        let Some(Marker::Task { checked, symbol }) = self.index.para(self.layout, pi).style.marker.clone() else {
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
    fn with_caret_before(mut self, n: usize) -> Edit {
        let caret = self.selection.end - n;
        self.selection = caret..caret;
        self
    }
}

/// Container prefix a continuation line needs: quote markers and list-item indentation.
pub fn continuation_prefix(text: &str, at: usize) -> String {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[line_start..];
    let p = LinePrefix::parse(line);
    let quote = &line[..p.quote_end];
    match &p.list {
        Some((r, _)) => format!("{quote}{}", " ".repeat(r.end - p.quote_end)),
        None => {
            let indent = line[p.quote_end..].len() - line[p.quote_end..].trim_start_matches(' ').len();
            format!("{quote}{}", " ".repeat(indent))
        }
    }
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
    let mut caret = None;
    let mut inserted = false;
    for d in deletions {
        if !inserted && at <= d.start {
            out.push_str(&text[pos..at]);
            out.push_str(insert);
            caret = Some(start + out.len());
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
        caret = Some(start + out.len());
        pos = at;
    }
    out.push_str(&text[pos..end]);
    let caret = caret.unwrap_or(start + out.len());
    Edit {
        range: start..end,
        replacement: out,
        selection: caret..caret,
    }
}

/// Escapes characters typed into the preview so they stay literal text in Markdown.
fn escape(typed: &str, ip: &InsertPoint, text: &str) -> String {
    if ip.code {
        return typed.to_string();
    }
    let in_table = matches!(ip.para_kind, ParaKind::TableCell { .. });
    let chars: Vec<char> = typed.chars().collect();
    let before = text[..ip.src].chars().next_back();
    let after = text[ip.src..].chars().next();
    let mut out = String::new();
    for (n, &c) in chars.iter().enumerate() {
        let prev = if n == 0 { before } else { Some(chars[n - 1]) };
        let next = chars.get(n + 1).copied().or(after);
        match c {
            '\\' | '`' | '*' | '[' | ']' | '<' | '$' | '~' | '^' => {
                out.push('\\');
                out.push(c);
            }
            '|' if in_table => out.push_str("\\|"),
            // Doubled, they would start a highlight, a spoiler or a comment.
            '=' | '|' | '%' if prev == Some(c) || next == Some(c) => {
                out.push('\\');
                out.push(c);
            }
            ':' if closes_shortcode(&text[text.floor_char_boundary(ip.src.saturating_sub(48))..ip.src], &out) => {
                out.push_str("\\:");
            }
            '{' if matches!(ip.para_kind, ParaKind::Heading(_)) => out.push_str("\\{"),
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
    if ip.at_para_start {
        if out.starts_with(['#', '>', '-', '+', '=']) {
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

/// The change to a `<details>` tag that adds or removes its `open` attribute.
fn toggle_open(tag: &str, open: bool) -> Option<(Range<usize>, String)> {
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
    let mut from = 0;
    while let Some(found) = lower[from..].find("open") {
        let start = from + found;
        from = start + 4;
        let before = start.checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(start + 4).copied();
        if !before.is_some_and(|b| b.is_ascii_whitespace())
            || !after.is_none_or(|b| b.is_ascii_whitespace() || matches!(b, b'=' | b'>' | b'/'))
        {
            continue;
        }
        let mut end = start + 4;
        let rest = lower[end..].trim_start();
        if let Some(value) = rest.strip_prefix('=') {
            let value_start = lower.len() - value.trim_start().len();
            end = match bytes.get(value_start) {
                Some(&q @ (b'"' | b'\'')) => lower[value_start + 1..]
                    .find(q as char)
                    .map_or(lower.len(), |e| value_start + e + 2),
                _ => {
                    value_start
                        + lower[value_start..]
                            .find(|c: char| c.is_ascii_whitespace() || c == '>')
                            .unwrap_or(0)
                }
            };
        }
        let space = lower[..start].trim_end().len();
        return Some((space..end, String::new()));
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
            b.type_with_pending(&pending, p, typed).map(|(e, next)| {
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
        let (s, caret) = edit("ab\n", |b| b.replace(1..1, "*"));
        assert_eq!(s, "a\\*b\n");
        assert_eq!(caret, 3);
        let (s, _) = edit("x\n", |b| b.replace(0..0, "# "));
        assert_eq!(s, "\\# x\n");
        let (s, _) = edit("snake\n", |b| b.replace(5..5, "_case"));
        assert_eq!(s, "snake_case\n");
        // Typed in the preview, none of the newer markup may take effect.
        for (typed, stored) in [
            ("==x== a = b", "\\=\\=x\\=\\= a = b"),
            ("x^2^", "x\\^2\\^"),
            ("||s|| a|b", "\\|\\|s\\|\\| a|b"),
            ("%%c%% 5%", "\\%\\%c\\%\\% 5%"),
            (":smile: 12:30", ":smile\\: 12:30"),
        ] {
            let (s, _) = edit("A\n", |b| b.replace(1..1, typed));
            assert_eq!(s, format!("A{stored}\n"));
            let parsed = parse(&s);
            let l = layout(&s, &parsed, &NoImages, None);
            let shown: String = l.blocks[0].paras[0].runs.iter().map(|r| r.text.as_str()).collect();
            assert_eq!(shown, format!("A{typed}"), "{s}");
        }
        let (s, _) = edit("# Titel\n", |b| b.replace(5..5, " {#x}"));
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
        b.replace(5..5, "z").expect("edit").apply_to(&mut s);
        assert_eq!(s, "Wort z\n");
    }

    #[test]
    fn deleting_whole_bold_content_removes_delimiters() {
        let (s, _) = edit("a **b** c\n", |b| b.replace(2..3, ""));
        assert_eq!(s, "a  c\n");
    }

    #[test]
    fn enter_continues_lists_and_splits_bold() {
        let (s, caret) = edit("- eins\n", |b| b.newline(6));
        assert_eq!(s, "- eins\n- \n");
        assert_eq!(caret, 9);
        let (s, _) = edit("1. a\n", |b| b.newline(4));
        assert_eq!(s, "1. a\n2. \n");
        let (s, _) = edit("x **fett**\n", |b| b.newline(4));
        assert_eq!(s, "x **fe**\n\n**tt**\n");
        let (s, _) = edit("- [x] a\n", |b| b.newline(3));
        assert_eq!(s, "- [x] a\n- [ ] \n");
    }

    #[test]
    fn return_adds_visible_empty_lines_and_backspace_removes_them() {
        let (s, caret) = edit("A\n\nB\n", |b| b.newline(1));
        assert_eq!((s.as_str(), caret), ("A\n\n\n\nB\n", 3));
        let (s, caret) = edit(&s, |b| b.newline(2));
        assert_eq!((s.as_str(), caret), ("A\n\n\n\n\n\nB\n", 5));
        let (s, caret) = edit(&s, |b| b.delete_at_block_start(3));
        assert_eq!((s.as_str(), caret), ("A\n\n\n\nB\n", 3));
        let (s, _) = edit(&s, |b| b.replace(2..2, "neu"));
        assert_eq!(s, "A\n\nneu\n\nB\n");
    }

    #[test]
    fn enter_on_empty_item_leaves_list() {
        let (s, _) = edit("- a\n- \n", |b| b.newline(6));
        assert_eq!(s, "- a\n\n");
    }

    #[test]
    fn enter_in_quote_and_table() {
        let (s, _) = edit("> ab\n", |b| b.newline(1));
        assert_eq!(s, "> a\n>\n> b\n");
        let (s, caret) = edit("| a | b |\n|---|---|\n| 1 | 2 |\n", |b| b.newline(5));
        assert_eq!(s.lines().count(), 4);
        assert!(caret > s.find("| 1").unwrap());
    }

    #[test]
    fn backspace_at_start_removes_block_prefix() {
        let (s, _) = edit("## Titel\n", |b| b.delete_at_block_start(0));
        assert_eq!(s, "Titel\n");
        let (s, _) = edit("- a\n", |b| b.delete_at_block_start(2));
        assert_eq!(s, "a\n");
    }

    #[test]
    fn task_toggle_and_tab_indent() {
        let (s, _) = edit("- [ ] a\n", |b| b.toggle_task(0));
        assert_eq!(s, "- [x] a\n");
        let (s, _) = edit("- a\n- b\n", |b| b.tab(6, false));
        assert_eq!(s, "- a\n  - b\n");
        let (s, _) = edit("- a\n  - b\n", |b| b.tab(6, true));
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
        let r = b.source_selection(2..6);
        assert_eq!(&t[r], "fett");
    }
}
