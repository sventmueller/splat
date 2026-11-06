//! A model the writer can rely on: nothing empty, no look on whitespace, runs that look the same in one piece.

use std::collections::HashSet;
use std::sync::Arc;

use splat_markdown::url;

use super::{Block, Cell, Item, List, ListKind, MOST_COLUMNS, Mark, Marks, Piece, Rich, Span, Table, visit_spans};

/// The highest number CommonMark allows in front of a list item.
pub(super) const HIGHEST_NUMBER: u64 = 999_999_999;

impl Rich {
    /// The same document in the form the writer needs: its `blocks` are empty if nothing would be left of it.
    #[must_use]
    pub(crate) fn tidied(self) -> Rich {
        let mut notes = Vec::new();
        let mut blocks = tidy_blocks(self.blocks, &mut notes);
        blocks.extend(notes);
        keep_answered_notes(&mut blocks);
        Rich { blocks }
    }
}

fn tidy_blocks(blocks: Vec<Block>, notes: &mut Vec<Block>) -> Vec<Block> {
    let mut out = Vec::with_capacity(blocks.len());
    for block in blocks {
        match block {
            Block::Paragraph(spans) => {
                let spans = tidy_spans(spans);
                if !spans.is_empty() {
                    out.push(Block::Paragraph(spans));
                }
            }
            Block::Heading { level, spans, anchor } => {
                let spans = without_whole_bold(tidy_spans(spans));
                if !spans.is_empty() {
                    out.push(Block::Heading {
                        level: level.clamp(1, 6),
                        spans,
                        anchor: anchor.map(|a| clean_name(&a)),
                    });
                }
            }
            Block::List(list) => {
                if let Some(list) = tidy_list(list, notes) {
                    out.push(Block::List(list));
                }
            }
            Block::Quote(inner) => {
                let inner = tidy_blocks(inner, notes);
                if !inner.is_empty() {
                    out.push(Block::Quote(inner));
                }
            }
            Block::Code { language, text } => {
                let text = clean_code(&text);
                if !text.trim().is_empty() {
                    out.push(Block::Code {
                        language: clean_language(&language),
                        text,
                    });
                }
            }
            Block::Rule => out.push(Block::Rule),
            Block::Table(table) => {
                if let Some(table) = tidy_table(table) {
                    out.push(Block::Table(table));
                }
            }
            Block::Math(tex) => {
                let tex = one_line(&tex);
                if !tex.is_empty() {
                    out.push(Block::Math(tex));
                }
            }
            Block::Note { name, blocks } => {
                let blocks = tidy_blocks(blocks, notes);
                if !blocks.is_empty() {
                    notes.push(Block::Note {
                        name: clean_name(&name),
                        blocks,
                    });
                }
            }
        }
    }
    out
}

fn tidy_list(list: List, notes: &mut Vec<Block>) -> Option<List> {
    let kind = match list.kind {
        ListKind::Numbered { start } => ListKind::Numbered {
            start: start.min(HIGHEST_NUMBER),
        },
        ListKind::Bullet => ListKind::Bullet,
    };
    let items: Vec<Item> = list
        .items
        .into_iter()
        .filter_map(|item| {
            let blocks = tidy_blocks(item.blocks, notes);
            let task = item
                .task
                .filter(|_| matches!(blocks.first(), Some(Block::Paragraph(_))));
            (!blocks.is_empty()).then_some(Item { task, blocks })
        })
        .collect();
    (!items.is_empty()).then_some(List { kind, items })
}

fn tidy_table(table: Table) -> Option<Table> {
    let mut rows: Vec<Vec<Cell>> = table
        .rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|cell| Cell {
                    spans: tidy_spans(cell.spans),
                    columns: cell.columns.clamp(1, MOST_COLUMNS),
                })
                .collect::<Vec<Cell>>()
        })
        .filter(|row| row.iter().any(|cell| !cell.spans.is_empty()))
        .collect();
    let header = rows.first_mut()?;
    for cell in header {
        cell.spans = without_whole_bold(std::mem::take(&mut cell.spans));
    }
    Some(Table {
        alignments: table.alignments,
        rows,
    })
}

/// Whether no look can be seen on it, so that it takes the look of what stands around it.
fn is_neutral(span: &Span) -> bool {
    is_blank(span) || matches!(span.piece, Piece::Note(_))
}

fn is_blank(span: &Span) -> bool {
    match &span.piece {
        Piece::Text(text) => text.chars().all(char::is_whitespace),
        Piece::Break => true,
        Piece::Code(_) | Piece::Image(_) | Piece::Note(_) | Piece::Math(_) => false,
    }
}

/// The link of a span if it may stay one; checking every run of one long address anew would take quadratic time.
#[derive(Default)]
struct LinkCheck {
    last: Option<(Arc<str>, bool)>,
}

impl LinkCheck {
    fn keep(&mut self, link: Option<Arc<str>>) -> Option<Arc<str>> {
        let link = link?;
        let allowed = match &self.last {
            Some((seen, allowed)) if Arc::ptr_eq(seen, &link) => *allowed,
            _ => !link.trim().is_empty() && url::is_harmless(&link),
        };
        self.last = Some((link.clone(), allowed));
        allowed.then_some(link)
    }
}

fn tidy_spans(spans: Vec<Span>) -> Vec<Span> {
    let mut out = split_edges(spans);
    inherit_looks(&mut out);
    trim_and_merge(out)
}

/// Each span cleaned, with the whitespace at its edges split off. Underlining is how a link looks anyway.
fn split_edges(spans: Vec<Span>) -> Vec<Span> {
    let mut out = Vec::with_capacity(spans.len());
    let mut links = LinkCheck::default();
    for span in spans {
        let link = links.keep(span.link);
        let marks = span
            .marks
            .with(Mark::Underline, span.marks.has(Mark::Underline) && link.is_none());
        match span.piece {
            Piece::Text(text) => push_with_edges(&clean_text(&text), marks, link, Piece::Text, &mut out),
            Piece::Code(code) => push_with_edges(&clean_text(&code), marks, link, Piece::Code, &mut out),
            Piece::Math(tex) => {
                let tex = one_line(&tex);
                if !tex.is_empty() {
                    out.push(Span {
                        piece: Piece::Math(tex),
                        marks,
                        link,
                    });
                }
            }
            Piece::Image(mut image) => {
                image.alt = clean_text(&image.alt).trim().to_string();
                out.push(Span {
                    piece: Piece::Image(image),
                    marks,
                    link,
                });
            }
            piece @ (Piece::Break | Piece::Note(_)) => out.push(Span { piece, marks, link }),
        }
    }
    out
}

fn push_with_edges(text: &str, marks: Marks, link: Option<Arc<str>>, piece: fn(String) -> Piece, out: &mut Vec<Span>) {
    let core = text.trim();
    if core.is_empty() {
        if !text.is_empty() {
            out.push(Span::text(text));
        }
        return;
    }
    let lead = text.len() - text.trim_start().len();
    if lead > 0 {
        out.push(Span::text(&text[..lead]));
    }
    out.push(Span {
        piece: piece(core.to_string()),
        marks,
        link,
    });
    if lead + core.len() < text.len() {
        out.push(Span::text(&text[lead + core.len()..]));
    }
}

/// What has no look of its own keeps what stands on both of its sides, so `**a** **b**` is `**a b**`. comrak does not
/// read a footnote reference inside link text, so it always stands outside.
fn inherit_looks(spans: &mut [Span]) {
    let mut before: Option<(Marks, Option<Arc<str>>)> = None;
    let mut at = 0;
    while at < spans.len() {
        if !is_neutral(&spans[at]) {
            before = Some((spans[at].marks, spans[at].link.clone()));
            at += 1;
            continue;
        }
        let end = spans[at..]
            .iter()
            .position(|s| !is_neutral(s))
            .map_or(spans.len(), |n| at + n);
        let (marks, link) = match (&before, spans.get(end)) {
            (Some((marks, link)), Some(after)) => (
                marks.common(after.marks),
                link.clone().filter(|l| after.link.as_ref() == Some(l)),
            ),
            _ => (Marks::default(), None),
        };
        for span in &mut spans[at..end] {
            span.marks = marks;
            span.link = if matches!(span.piece, Piece::Note(_)) {
                None
            } else {
                link.clone()
            };
        }
        at = end;
    }
}

/// No whitespace at the ends or around a break, at most one break in a row, and runs of one look as one span.
fn trim_and_merge(spans: Vec<Span>) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for mut span in spans {
        let is_break = matches!(span.piece, Piece::Break);
        if is_blank(&span) && (out.is_empty() || matches!(out.last().map(|s| &s.piece), Some(Piece::Break))) {
            continue;
        }
        if is_break {
            trim_end(&mut out);
        }
        // comrak reads `[^1][^1]` as one reference and some text.
        if matches!((out.last().map(|s| &s.piece), &span.piece), (Some(Piece::Note(a)), Piece::Note(b)) if a == b) {
            continue;
        }
        if let Some(last) = out.last_mut()
            && last.marks == span.marks
            && last.link == span.link
            && let (Piece::Text(a), Piece::Text(b)) | (Piece::Code(a), Piece::Code(b)) =
                (&mut last.piece, &mut span.piece)
        {
            a.push_str(b);
            continue;
        }
        out.push(span);
    }
    trim_end(&mut out);
    out
}

fn trim_end(spans: &mut Vec<Span>) {
    while spans.last().is_some_and(is_blank) {
        spans.pop();
    }
    if let Some(Span {
        piece: Piece::Text(text),
        ..
    }) = spans.last_mut()
    {
        text.truncate(text.trim_end().len());
    }
}

/// The spans without bold if all that show it have it: what a heading is anyway does not need saying. Runs that bold
/// alone kept apart become one, so that tidying again changes nothing.
fn without_whole_bold(mut spans: Vec<Span>) -> Vec<Span> {
    if spans.iter().all(|s| is_neutral(s) || s.marks.has(Mark::Bold)) {
        for span in &mut spans {
            span.marks = span.marks.with(Mark::Bold, false);
        }
        return trim_and_merge(spans);
    }
    spans
}

fn clean_text(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\n' | '\r' => Some(' '),
            '\t' => Some('\t'),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

fn clean_code(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut kept: String = text
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    kept.truncate(kept.trim_end_matches('\n').len());
    kept
}

/// `text` on one line, without control characters and without whitespace at its ends.
pub(super) fn one_line(text: &str) -> String {
    clean_text(text).trim().to_string()
}

fn clean_language(language: &str) -> String {
    language
        .trim()
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '+' | '#' | '.'))
        .collect()
}

/// A name for a footnote or a heading's anchor that survives Markdown.
fn clean_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    if clean.is_empty() { "note".to_string() } else { clean }
}

/// A reference without a definition would read as text and a definition without a reference as nothing, so neither stays.
fn keep_answered_notes(blocks: &mut Vec<Block>) {
    let mut defined = HashSet::new();
    blocks.retain(|block| match block {
        Block::Note { name, .. } => defined.insert(name.clone()),
        _ => true,
    });
    let mut referred = HashSet::new();
    let mut dropped_any = false;
    visit_spans(blocks, &mut |span| {
        if let Piece::Note(name) = &mut span.piece {
            *name = clean_name(name);
            if defined.contains(name) {
                referred.insert(name.clone());
            } else {
                span.piece = Piece::Text(String::new());
                dropped_any = true;
            }
        }
    });
    blocks.retain(|block| !matches!(block, Block::Note { name, .. } if !referred.contains(name)));
    if dropped_any {
        let mut notes = Vec::new();
        *blocks = tidy_blocks(std::mem::take(blocks), &mut notes);
        blocks.extend(notes);
    }
}
