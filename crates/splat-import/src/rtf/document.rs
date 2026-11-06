//! Paragraphs and tables as RTF read them become blocks once all of them are known, since a heading is a matter of
//! being larger than the body text and a code block of several lines in a row.

use std::collections::HashMap;

use super::DEFAULT_SIZE;
use super::fields::list_kind;
use crate::rich::{Block, FlatItem, Item, Mark, Marks, Piece, Span, Table, nest_list};

/// The longest text that can still be a heading.
const LONGEST_HEADING: usize = 200;

/// A paragraph's place in a list.
pub(super) struct ListPlace {
    pub(super) id: u64,
    pub(super) level: usize,
    pub(super) marker: Option<String>,
}

/// A paragraph with what decides what it becomes.
pub(super) struct Paragraph {
    pub(super) spans: Vec<Span>,
    /// The smallest font size of its text, in half points.
    pub(super) least_size: i32,
    /// Characters of its text other than spaces.
    pub(super) chars: usize,
    pub(super) list: Option<ListPlace>,
    /// The outline level its paragraph properties give, from 0 for the top.
    pub(super) outline: Option<u8>,
}

impl Paragraph {
    /// Whether it holds nothing but plain code in lines.
    fn is_code(&self) -> bool {
        self.list.is_none()
            && self.spans.iter().any(|span| matches!(span.piece, Piece::Code(_)))
            && self.spans.iter().all(|span| {
                span.link.is_none()
                    && match &span.piece {
                        Piece::Code(_) => span.marks == Marks::default(),
                        Piece::Text(text) => text.chars().all(char::is_whitespace),
                        _ => true,
                    }
            })
    }

    fn code_text(&self) -> String {
        if self.chars == 0 {
            return String::new();
        }
        self.spans
            .iter()
            .map(|span| match &span.piece {
                Piece::Code(text) | Piece::Text(text) => text.as_str(),
                _ => "\n",
            })
            .collect()
    }
}

/// What the document holds, in its order.
pub(super) enum Collected {
    Paragraph(Paragraph),
    Table(Table),
    /// A formula on lines of its own.
    Math(String),
}

/// The blocks of a document with `characters_by_size` characters at each font size.
pub(super) fn assemble(collected: Vec<Collected>, characters_by_size: &HashMap<i32, usize>) -> Vec<Block> {
    let mut assembler = Assembler {
        blocks: Vec::new(),
        flat: Vec::new(),
        code: Vec::new(),
        body_size: body_size(characters_by_size),
    };
    for item in collected {
        match item {
            Collected::Paragraph(paragraph) => assembler.add(paragraph),
            Collected::Table(table) => {
                assembler.end_runs();
                assembler.blocks.push(Block::Table(table));
            }
            Collected::Math(tex) => {
                assembler.end_runs();
                assembler.blocks.push(Block::Math(tex));
            }
        }
    }
    assembler.end_runs();
    assembler.blocks
}

/// The size most of the text has, the smaller one if two have the same.
fn body_size(characters_by_size: &HashMap<i32, usize>) -> i32 {
    characters_by_size
        .iter()
        .max_by_key(|(size, count)| (**count, std::cmp::Reverse(**size)))
        .map_or(DEFAULT_SIZE, |(size, _)| *size)
}

struct Assembler {
    blocks: Vec<Block>,
    flat: Vec<FlatItem>,
    /// Paragraphs of code in a row, with the blank ones between them.
    code: Vec<Paragraph>,
    body_size: i32,
}

impl Assembler {
    fn add(&mut self, paragraph: Paragraph) {
        if paragraph.chars == 0 {
            if !self.code.is_empty() {
                self.code.push(paragraph);
            }
            return;
        }
        if paragraph.is_code() {
            self.end_list();
            self.code.push(paragraph);
            return;
        }
        self.end_code();
        match paragraph.list {
            Some(place) => {
                let (kind, task) = list_kind(place.marker.as_deref().unwrap_or(""));
                self.flat.push(FlatItem {
                    level: place.level,
                    list: place.id,
                    kind,
                    item: Item {
                        task,
                        blocks: vec![Block::Paragraph(paragraph.spans)],
                    },
                });
            }
            None => {
                self.end_list();
                self.add_text(paragraph);
            }
        }
    }

    fn add_text(&mut self, paragraph: Paragraph) {
        let block = match heading_level(&paragraph, self.body_size) {
            Some(level) => Block::Heading {
                level,
                spans: without_whole_italic(paragraph.spans),
                anchor: None,
            },
            None => Block::Paragraph(paragraph.spans),
        };
        self.blocks.push(block);
    }

    fn end_runs(&mut self) {
        self.end_code();
        self.end_list();
    }

    fn end_list(&mut self) {
        if !self.flat.is_empty() {
            self.blocks.extend(nest_list(std::mem::take(&mut self.flat)));
        }
    }

    /// One line of code stays within its line, as a copied name can be meant for the middle of a sentence.
    fn end_code(&mut self) {
        let mut run = std::mem::take(&mut self.code);
        while run.last().is_some_and(|paragraph| paragraph.chars == 0) {
            run.pop();
        }
        if run.len() >= 2 {
            let lines: Vec<String> = run.iter().map(Paragraph::code_text).collect();
            self.blocks.push(Block::Code {
                language: String::new(),
                text: lines.join("\n"),
            });
        } else {
            for paragraph in run {
                self.add_text(paragraph);
            }
        }
    }
}

/// RTF writes the look of a heading's style on every run of it, which the heading says by itself.
fn without_whole_italic(mut spans: Vec<Span>) -> Vec<Span> {
    let shown = |span: &Span| match &span.piece {
        Piece::Text(text) => !text.chars().all(char::is_whitespace),
        Piece::Break | Piece::Note(_) => false,
        _ => true,
    };
    if spans
        .iter()
        .filter(|span| shown(span))
        .all(|span| span.marks.has(Mark::Italic))
    {
        for span in &mut spans {
            span.marks = span.marks.with(Mark::Italic, false);
        }
    }
    spans
}

/// The level of a paragraph with an outline level above the body text's 9, or else of a short paragraph clearly
/// larger than the body text.
fn heading_level(paragraph: &Paragraph, body_size: i32) -> Option<u8> {
    if let Some(level) = paragraph.outline {
        return (level < 9).then(|| (level + 1).min(6));
    }
    if paragraph.chars > LONGEST_HEADING {
        return None;
    }
    let (size, body) = (i64::from(paragraph.least_size), i64::from(body_size));
    [(180, 1), (140, 2), (115, 3)]
        .into_iter()
        .find(|(percent, _)| size * 100 >= body * percent)
        .map(|(_, level)| level)
}
