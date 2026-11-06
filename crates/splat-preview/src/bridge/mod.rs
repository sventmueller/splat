//! Translates edits made in the rendered preview into edits of the Markdown source: `replace` for what is typed, pasted or
//! deleted, `keys` for the keys that act on blocks, `escape` for what keeps typed text literal.

use std::ops::Range;

use splat_markdown::commands::{Edit, LinePrefix};
use splat_markdown::doc::Doc;
use splat_markdown::text::{Byte, TextAt, continuation_of, line_start};

use super::layout::{Layout, ParaKind, RunMap};
use super::map::{Index, InsertPoint, Shown, insert_point};

mod escape;
mod keys;
mod replace;

use escape::escape_typed;

/// Translates edits made in the rendered preview into edits of the Markdown source.
pub struct Bridge<'a> {
    text: &'a str,
    doc: &'a Doc,
    layout: &'a Layout,
    index: &'a Index,
}

impl<'a> Bridge<'a> {
    /// `None` when `layout` was made from another version of `doc`: what one says about a position means nothing in the
    /// other.
    pub fn new(doc: &'a Doc, layout: &'a Layout, index: &'a Index) -> Option<Bridge<'a>> {
        debug_assert!(index.fits(layout), "an index made for another layout");
        (layout.doc_version == doc.version()).then_some(Bridge {
            text: doc.text(),
            doc,
            layout,
            index,
        })
    }
}

impl Bridge<'_> {
    fn point(&self, p: Shown) -> Option<InsertPoint> {
        insert_point(self.layout, self.index, p)
    }

    /// Source range for a preview selection: the start snaps into the first selected character.
    pub fn source_selection(&self, range: Range<Shown>) -> Range<Byte> {
        let end = self.point(range.end).map_or(Byte::default(), |p| p.src);
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
    fn source_of_char_at(&self, p: Shown) -> Option<Byte> {
        let para_index = self.index.para_at(p)?;
        let para = self.index.para(self.layout, para_index);
        let (at, run) = para
            .runs_at(self.index.paras[para_index].start)
            .find(|(at, _)| at.contains(&p))?;
        Some(match &run.map {
            RunMap::Atom { src, .. } => src.start,
            RunMap::Text { chars, end } => {
                let mut before = at.start;
                let k = run
                    .text
                    .chars()
                    .take_while(|c| {
                        let behind = before < p;
                        before += c.len_utf16();
                        behind
                    })
                    .count();
                chars.get(&run.text, k).unwrap_or(*end)
            }
        })
    }

    /// Source position that text typed at preview offset `p` goes to.
    pub fn source_at(&self, p: Shown) -> Option<Byte> {
        self.point(p).map(|point| point.src)
    }

    /// `typed` escaped for insertion at preview offset `p`; inside code it stays literal.
    pub fn escaped(&self, p: Shown, typed: &str, code: bool) -> Option<String> {
        let mut point = self.point(p)?;
        point.code = code || matches!(point.para_kind, ParaKind::Code { .. });
        Some(escape_typed(typed, &point, self.text))
    }
}

/// Container prefix a continuation line needs: quote markers and list-item indentation.
pub(crate) fn continuation_prefix(text: &str, at: Byte) -> String {
    let line_begin = line_start(text, at);
    let line = text.from(line_begin);
    let p = LinePrefix::parse(line);
    let prefix_end = match &p.list {
        Some((r, _)) => r.end,
        None => p.quote_end + (line[p.quote_end..].len() - line[p.quote_end..].trim_start_matches(' ').len()),
    };
    continuation_of(&line[..prefix_end])
}

/// Builds one edit that removes `deletions` and inserts `insert` at `at`, so undo sees a single step.
fn compose(text: &str, deletions: &[Range<Byte>], at: Byte, insert: &str) -> Edit {
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
            out.push_str(text.at(pos..at));
            out.push_str(insert);
            caret = start + out.len();
            pos = at;
            inserted = true;
        }
        out.push_str(text.at(pos..d.start));
        pos = d.end;
    }
    if !inserted {
        let at = at.max(pos);
        out.push_str(text.at(pos..at));
        out.push_str(insert);
        caret = start + out.len();
        pos = at;
    }
    out.push_str(text.at(pos..end));
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

#[cfg(test)]
mod tests;
