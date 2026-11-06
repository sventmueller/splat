//! The document as its tags and text arrive: an open element brings back the look it found when it closes, and the containers
//! it opened end with it.

use std::collections::{HashMap, HashSet};

use splat_markdown::markup;

use super::data;
use super::style::{Attributes, Sheet, Style, WordList, attr, is_preformatted, language_of};
use super::table::{CellShape, OpenRow, OpenTable};
use crate::rich::{
    Block, FlatItem, Image, Item, List, ListKind, Piece, Rich, Span, levels_by_indent, marker_kind, marker_task,
    nest_list, visit_spans,
};

/// Elements nested deeper give way to their content, so that nothing behind this reader has to recurse deeply.
const MOST_OPEN: usize = 64;
/// The number no Word list has, for `<li>` without a list around it.
const NO_LIST: u64 = u64::MAX;
/// The longest text in front of Word's spaces that is taken for a list's bullet or number, such as `iii.` or `1.2.10.`.
const LONGEST_MARKER: usize = 12;
/// The left margin of Word's list paragraphs where the page says none.
const LIST_INDENT: f32 = 36.0;

/// An open element and what its end brings back.
#[derive(Debug)]
struct Open {
    name: String,
    style: Style,
    frames: usize,
}

/// A container being filled.
#[derive(Debug)]
enum Frame {
    Quote(Vec<Block>),
    List(List),
    Item(Item),
    Table(OpenTable),
    Row(OpenRow),
    Cell { columns: u32 },
}

/// Text taken as it stands, up to the end of its element.
#[derive(Debug)]
struct Pre {
    element: String,
    /// How many elements of its name are open inside it.
    nested: usize,
    text: String,
    language: Option<std::sync::Arc<str>>,
}

impl Pre {
    fn end_line(&mut self) {
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            self.text.push('\n');
        }
    }

    fn open(&mut self, name: &str, attrs: &Attributes, empty: bool) {
        if self.language.is_none() {
            self.language = language_of(attrs);
        }
        if name == "br" {
            self.text.push('\n');
        } else if is_block(name) {
            self.end_line();
        }
        if name == self.element && !empty {
            self.nested += 1;
        }
    }

    /// Takes in a closing tag; whether it ends the text.
    fn close(&mut self, name: &str) -> bool {
        if name == self.element {
            if self.nested == 0 {
                return true;
            }
            self.nested -= 1;
        }
        if is_block(name) {
            self.end_line();
        }
        false
    }
}

/// Where the document stands.
#[derive(Debug)]
pub(super) struct Builder {
    root: Vec<Block>,
    elements: Vec<Open>,
    frames: Vec<Frame>,
    style: Style,
    /// The line being filled: a paragraph's, or the cell's that is open.
    line: Vec<Span>,
    /// Whitespace would add nothing here: at the start of the line or behind other whitespace.
    behind_space: bool,
    /// Something that makes a new line in a cell ended, and text has yet to follow it.
    pending_break: bool,
    in_head: bool,
    /// Inside what Word's conditional comments mark as a list's bullet or number.
    in_marker: bool,
    marker: Option<String>,
    word_items: Vec<FlatItem>,
    /// The left margin of each of `word_items` whose level the page does not say.
    indents: Vec<Option<f32>>,
    /// The text of each Word footnote reference, by the note's anchor.
    note_labels: HashMap<String, String>,
    /// The page comes from Cocoa, whose style sheets say what its tags do not.
    from_cocoa: bool,
    sheet: Sheet,
    pre: Option<Pre>,
}

impl Default for Builder {
    fn default() -> Builder {
        Builder {
            root: Vec::new(),
            elements: Vec::new(),
            frames: Vec::new(),
            style: Style::default(),
            line: Vec::new(),
            behind_space: true,
            pending_break: false,
            in_head: false,
            in_marker: false,
            marker: None,
            word_items: Vec::new(),
            indents: Vec::new(),
            note_labels: HashMap::new(),
            from_cocoa: false,
            sheet: Sheet::default(),
            pre: None,
        }
    }
}

/// Elements that end the line they stand in.
fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "main"
            | "nav"
            | "aside"
            | "figure"
            | "figcaption"
            | "address"
            | "center"
            | "dl"
            | "dt"
            | "dd"
            | "details"
            | "summary"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "ul"
            | "ol"
            | "li"
            | "table"
            | "caption"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "td"
            | "th"
            | "blockquote"
            | "pre"
            | "hr"
            | "form"
            | "fieldset"
            | "legend"
    )
}

/// What a browser takes as ended when `name` opens, among the elements inside the nearest barrier.
fn ends_when_opening(name: &str, open: &str) -> bool {
    match name {
        "li" => matches!(open, "li" | "p"),
        "tr" => matches!(open, "tr" | "td" | "th" | "p"),
        "thead" | "tbody" | "tfoot" => matches!(open, "thead" | "tbody" | "tfoot" | "tr" | "td" | "th" | "p"),
        "td" | "th" => matches!(open, "td" | "th" | "p"),
        "dt" | "dd" => matches!(open, "dt" | "dd" | "p"),
        _ => is_block(name) && open == "p",
    }
}

fn is_barrier(name: &str, open: &str) -> bool {
    match name {
        "li" => matches!(open, "ul" | "ol" | "table" | "blockquote" | "td" | "th"),
        "tr" | "thead" | "tbody" | "tfoot" | "td" | "th" => open == "table",
        "dt" | "dd" => matches!(open, "dl" | "table" | "td" | "th"),
        _ => matches!(
            open,
            "ul" | "ol" | "li" | "table" | "td" | "th" | "caption" | "dl" | "details" | "blockquote" | "div"
        ),
    }
}

/// Whether a closing tag of `name` may reach behind `open`: `</span>` in a cell does not end the table around it.
fn closes_across(name: &str, open: &str) -> bool {
    !matches!(open, "table" | "td" | "th" | "caption")
        || matches!(
            name,
            "table" | "tr" | "td" | "th" | "thead" | "tbody" | "tfoot" | "caption"
        )
}

fn is_blank(span: &Span) -> bool {
    match &span.piece {
        Piece::Text(text) => text.chars().all(char::is_whitespace),
        Piece::Break => true,
        _ => false,
    }
}

impl Builder {
    fn in_cell(&self) -> bool {
        matches!(self.frames.last(), Some(Frame::Cell { .. }))
    }

    /// Takes in a style sheet, which counts only on a page from Cocoa.
    pub(super) fn style_sheet(&mut self, css: &str) {
        if self.from_cocoa {
            self.sheet.read(css);
        }
    }

    /// Takes in what stands between `<!` and `>` or in a comment: only Word's marks around a list's bullet or number count.
    pub(super) fn conditional(&mut self, text: &str) {
        let text = text.trim();
        if text.eq_ignore_ascii_case("[if !supportLists]") {
            self.in_marker = true;
        } else if text.eq_ignore_ascii_case("[endif]") {
            self.in_marker = false;
        }
    }

    /// Takes in text as it stands in the HTML.
    pub(super) fn text(&mut self, raw: &str) {
        if self.in_head {
            return;
        }
        let decoded = markup::decode(raw);
        if let Some(pre) = &mut self.pre {
            let decoded = if pre.text.is_empty() {
                decoded
                    .strip_prefix('\r')
                    .unwrap_or(&decoded)
                    .strip_prefix('\n')
                    .unwrap_or(&decoded)
            } else {
                &decoded
            };
            pre.text.push_str(decoded);
            return;
        }
        if self.in_marker || self.style.in_marker {
            self.marker.get_or_insert_default().push_str(&decoded);
            return;
        }
        if self.style.note_number {
            return;
        }
        if let Some(note) = self.style.note_reference.clone() {
            self.note_reference(&note, &decoded);
            return;
        }
        let collapsed = self.collapsed(&decoded);
        if !collapsed.is_empty() {
            let piece = if self.style.code {
                Piece::Code(collapsed)
            } else {
                Piece::Text(collapsed)
            };
            self.push_piece(piece);
        }
    }

    fn collapsed(&mut self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            if c.is_ascii_whitespace() {
                if !self.behind_space {
                    out.push(' ');
                    self.behind_space = true;
                }
            } else {
                out.push(c);
                self.behind_space = false;
            }
        }
        out
    }

    fn note_reference(&mut self, note: &str, label: &str) {
        let label = label.trim();
        if label.is_empty() {
            return;
        }
        self.note_labels.entry(note.to_string()).or_default().push_str(label);
        if !matches!(self.line.last(), Some(Span { piece: Piece::Note(last), .. }) if last == note) {
            self.push_piece(Piece::Note(note.to_string()));
        }
        self.behind_space = false;
    }

    fn push_piece(&mut self, piece: Piece) {
        if std::mem::take(&mut self.pending_break) && !self.line.is_empty() {
            self.line.push(Span::of(Piece::Break));
        }
        self.line.push(Span {
            piece,
            marks: self.style.marks,
            link: self.style.link.clone(),
        });
    }

    /// Takes in an opening tag.
    pub(super) fn open(&mut self, name: String, attrs: &Attributes, self_closing: bool) {
        if name == "meta"
            && attr(attrs, "name").is_some_and(|meta| meta.eq_ignore_ascii_case("generator"))
            && attr(attrs, "content").is_some_and(|generator| generator.starts_with("Cocoa HTML Writer"))
        {
            self.from_cocoa = true;
        }
        if self.in_head {
            if matches!(name.as_str(), "meta" | "link" | "base") {
                return;
            }
            self.in_head = false;
        }
        let empty = self_closing || markup::is_void(&name);
        if let Some(pre) = &mut self.pre {
            pre.open(&name, attrs, empty);
            return;
        }
        if name == "head" {
            self.in_head = !self_closing;
            return;
        }
        if empty {
            self.open_empty(&name, attrs, self_closing);
            return;
        }
        if self.elements.len() >= MOST_OPEN {
            if is_block(&name) {
                self.flush_line();
            }
            return;
        }
        self.close_implied(&name);
        if is_block(&name) {
            self.flush_line();
        }
        if name == "pre" || is_preformatted(&name, attrs) {
            self.pre = Some(Pre {
                language: language_of(attrs).or_else(|| self.style.language.clone()),
                element: name,
                nested: 0,
                text: String::new(),
            });
            return;
        }
        self.open_frames_before(&name);
        let frames = self.frames.len();
        let mut style = self.style.inside(&name, attrs, &self.sheet);
        if style.marker_spaces && !self.style.marker_spaces {
            self.take_marker(&mut style);
        }
        self.open_frames(&name, attrs, &mut style);
        self.elements.push(Open {
            name,
            style: std::mem::replace(&mut self.style, style),
            frames,
        });
    }

    /// Word's filtered pages write a list's bullet or number as text in front of its spaces, without saying it is one.
    fn take_marker(&mut self, style: &mut Style) {
        if self.in_marker || self.style.in_marker || self.marker.is_some() || !self.style.hanging || self.in_cell() {
            return;
        }
        let mut marker = String::new();
        for span in &self.line {
            match &span.piece {
                Piece::Text(text) => marker.push_str(text),
                _ => return,
            }
        }
        let marker = marker.trim();
        if marker.is_empty() || marker.chars().count() > LONGEST_MARKER {
            return;
        }
        self.marker = Some(marker.to_string());
        self.line.clear();
        self.behind_space = true;
        style.in_marker = true;
    }

    fn open_empty(&mut self, name: &str, attrs: &Attributes, self_closing: bool) {
        match name {
            "br" => self.line_break(),
            "img" => self.image(attrs),
            "hr" => {
                self.close_implied(name);
                self.flush_line();
                if !self.in_cell() {
                    self.place(Block::Rule);
                }
            }
            "input" => self.checkbox(attrs),
            _ if self_closing && is_block(name) => self.flush_line(),
            _ => {}
        }
    }

    fn line_break(&mut self) {
        if self.in_marker || self.style.in_marker || self.pending_break {
            return;
        }
        let behind_break = matches!(
            self.line.last(),
            Some(Span {
                piece: Piece::Break,
                ..
            })
        );
        if behind_break && !self.in_cell() && self.style.heading.is_none() {
            self.flush_line();
        } else if !self.line.is_empty() && !behind_break {
            self.push_piece(Piece::Break);
            self.behind_space = true;
        }
    }

    fn image(&mut self, attrs: &Attributes) {
        if self.in_marker || self.style.in_marker {
            return;
        }
        let alt = attr(attrs, "alt").unwrap_or("").to_string();
        let piece = match data::source_of(attr(attrs, "src").unwrap_or("").trim()) {
            Some(source) => Piece::Image(Image { source, alt }),
            None if alt.is_empty() => return,
            None => Piece::Text(alt),
        };
        self.push_piece(piece);
        self.behind_space = false;
    }

    /// A checkbox makes a task of the list item it starts.
    fn checkbox(&mut self, attrs: &Attributes) {
        if !attr(attrs, "type").is_some_and(|kind| kind.eq_ignore_ascii_case("checkbox")) || !self.line.is_empty() {
            return;
        }
        if let Some(Frame::Item(item)) = self.frames.last_mut()
            && item.blocks.is_empty()
            && item.task.is_none()
        {
            item.task = Some(attr(attrs, "checked").is_some());
        }
    }

    /// What a browser ends by itself when `name` opens.
    fn close_implied(&mut self, name: &str) {
        if !is_block(name) {
            return;
        }
        let inside = self
            .elements
            .iter()
            .rposition(|open| is_barrier(name, &open.name))
            .map_or(0, |barrier| barrier + 1);
        if let Some(found) = self.elements[inside..]
            .iter()
            .position(|open| ends_when_opening(name, &open.name))
        {
            self.pop_to(inside + found);
        }
    }

    /// Ends the element at `index` and all inside it.
    fn pop_to(&mut self, index: usize) {
        if self.elements[index..].iter().any(|open| is_block(&open.name)) {
            self.flush_line();
        }
        let frames = self.elements[index].frames;
        self.close_frames(frames);
        self.elements.truncate(index + 1);
        if let Some(open) = self.elements.pop() {
            self.style = open.style;
        }
    }

    /// Takes in a closing tag.
    pub(super) fn close(&mut self, name: &str) {
        if self.in_head {
            self.in_head = name != "head";
            return;
        }
        if let Some(pre) = &mut self.pre {
            if pre.close(name) {
                self.end_pre();
            }
            return;
        }
        let mut found = None;
        for (index, open) in self.elements.iter().enumerate().rev() {
            if open.name == name {
                found = Some(index);
                break;
            }
            if !closes_across(name, &open.name) {
                break;
            }
        }
        match found {
            Some(index) => self.pop_to(index),
            None if is_block(name) => self.flush_line(),
            None => {}
        }
    }

    fn end_pre(&mut self) {
        let Some(pre) = self.pre.take() else { return };
        if self.in_cell() {
            for line in pre.text.lines() {
                self.push_piece(Piece::Code(line.to_string()));
                self.pending_break = true;
            }
            self.behind_space = true;
        } else {
            self.place(Block::Code {
                language: pre.language.as_deref().unwrap_or("").to_string(),
                text: pre.text,
            });
        }
    }

    /// Whatever `name` needs to stand around its containers, which stays open behind its end.
    fn open_frames_before(&mut self, name: &str) {
        if self.in_cell() {
            return;
        }
        match name {
            "ul" | "ol" => {
                self.flush_word_items();
                if let Some(Frame::List(list)) = self.frames.last_mut() {
                    let item = list.items.pop().unwrap_or(Item {
                        task: None,
                        blocks: Vec::new(),
                    });
                    self.frames.push(Frame::Item(item));
                }
            }
            "blockquote" | "table" => {
                self.flush_word_items();
                self.ensure_item();
            }
            "li" if matches!(self.frames.last(), Some(Frame::List(_))) => self.flush_word_items(),
            "li" if matches!(self.frames.last(), Some(Frame::Item(_))) => self.close_frames(self.frames.len() - 1),
            "tr" => {
                if matches!(self.frames.last(), Some(Frame::Row(_))) {
                    self.close_frames(self.frames.len() - 1);
                }
            }
            "td" | "th" => {
                if matches!(self.frames.last(), Some(Frame::Table(_))) {
                    self.frames.push(Frame::Row(OpenRow::default()));
                }
            }
            _ => {}
        }
    }

    fn open_frames(&mut self, name: &str, attrs: &Attributes, style: &mut Style) {
        if self.in_cell() {
            if matches!(name, "p" | "div") {
                self.align_cell(name, attrs);
            }
            return;
        }
        match name {
            "ul" => self.push_list(ListKind::Bullet),
            "ol" => {
                let start = attr(attrs, "start").and_then(|start| start.trim().parse().ok());
                self.push_list(ListKind::Numbered {
                    start: start.unwrap_or(1),
                });
            }
            "li" => self.open_item(attrs, style),
            "blockquote" => self.frames.push(Frame::Quote(Vec::new())),
            "table" => self.frames.push(Frame::Table(OpenTable::default())),
            "tr" => {
                if matches!(self.frames.last(), Some(Frame::Table(_))) {
                    self.frames.push(Frame::Row(OpenRow::default()));
                }
            }
            "td" | "th" => self.open_cell(name, attrs),
            _ => {}
        }
    }

    fn push_list(&mut self, kind: ListKind) {
        self.frames.push(Frame::List(List {
            kind,
            items: Vec::new(),
        }));
    }

    fn open_item(&mut self, attrs: &Attributes, style: &mut Style) {
        if !matches!(self.frames.last(), Some(Frame::List(_))) {
            style.word_list = Some(WordList {
                level: 0,
                list: NO_LIST,
            });
            return;
        }
        let task = attr(attrs, "role")
            .filter(|role| role.eq_ignore_ascii_case("checkbox"))
            .map(|_| attr(attrs, "aria-checked").is_some_and(|checked| checked.eq_ignore_ascii_case("true")));
        self.frames.push(Frame::Item(Item {
            task,
            blocks: Vec::new(),
        }));
    }

    fn open_cell(&mut self, name: &str, attrs: &Attributes) {
        let shape = CellShape::of(attrs, &self.sheet.declarations(name, attrs));
        let [.., Frame::Table(table), Frame::Row(row)] = self.frames.as_mut_slice() else {
            return;
        };
        table.start_cell(row, &shape);
        self.frames.push(Frame::Cell {
            columns: shape.columns(),
        });
    }

    /// Word aligns the paragraphs in a cell rather than the cell.
    fn align_cell(&mut self, name: &str, attrs: &Attributes) {
        let from_sheet = self.sheet.declarations(name, attrs);
        if let [.., Frame::Table(table), Frame::Row(row), Frame::Cell { columns }] = self.frames.as_mut_slice() {
            table.align_open_cell(row, *columns, attrs, &from_sheet);
        }
    }

    fn ensure_item(&mut self) {
        if matches!(self.frames.last(), Some(Frame::List(_))) {
            self.frames.push(Frame::Item(Item {
                task: None,
                blocks: Vec::new(),
            }));
        }
    }

    /// Ends the line being filled: in a cell the next text starts a new one, elsewhere it makes a block.
    fn flush_line(&mut self) {
        self.in_marker = false;
        self.behind_space = true;
        if self.in_cell() {
            self.marker = None;
            self.pending_break |= !self.line.is_empty();
            return;
        }
        let spans = std::mem::take(&mut self.line);
        let marker = self.marker.take();
        if spans.iter().all(is_blank) {
            return;
        }
        if let Some(level) = self.style.heading {
            self.place(Block::Heading {
                level,
                spans,
                anchor: None,
            });
            return;
        }
        let marker = marker.as_deref().map(str::trim).filter(|marker| !marker.is_empty());
        let word_list = self.style.word_list.or(marker.map(|_| WordList { level: 0, list: 0 }));
        match word_list {
            Some(WordList { level, list }) => {
                self.indents.push(
                    self.style
                        .word_list
                        .is_none()
                        .then(|| self.style.indent.unwrap_or(LIST_INDENT)),
                );
                self.word_items.push(FlatItem {
                    level,
                    list,
                    kind: marker.map_or(ListKind::Bullet, marker_kind),
                    item: Item {
                        task: marker.and_then(marker_task),
                        blocks: vec![Block::Paragraph(spans)],
                    },
                });
            }
            None => self.place(Block::Paragraph(spans)),
        }
    }

    fn flush_word_items(&mut self) {
        if !self.word_items.is_empty() {
            let mut items = std::mem::take(&mut self.word_items);
            levels_by_indent(&mut items, &std::mem::take(&mut self.indents));
            for block in nest_list(items) {
                self.push_block(block);
            }
        }
    }

    fn place(&mut self, block: Block) {
        self.flush_word_items();
        self.push_block(block);
    }

    /// Adds a block to the innermost container that can hold it; text that strays into a table goes in front of the table.
    fn push_block(&mut self, block: Block) {
        self.ensure_item();
        let container = self.frames.iter_mut().rev().find_map(|frame| match frame {
            Frame::Quote(blocks) => Some(blocks),
            Frame::Item(item) => Some(&mut item.blocks),
            _ => None,
        });
        match (container, &self.style.note) {
            (Some(container), _) => container.push(block),
            (None, Some(note)) => {
                if let Some(Block::Note { name, blocks }) = self.root.last_mut()
                    && name.as_str() == &**note
                {
                    blocks.push(block);
                } else {
                    self.root.push(Block::Note {
                        name: note.to_string(),
                        blocks: vec![block],
                    });
                }
            }
            (None, None) => self.root.push(block),
        }
    }

    fn close_frames(&mut self, count: usize) {
        while self.frames.len() > count {
            self.close_frame();
        }
    }

    fn close_frame(&mut self) {
        self.flush_line();
        self.flush_word_items();
        let Some(frame) = self.frames.pop() else { return };
        match frame {
            Frame::Quote(blocks) => self.push_block(Block::Quote(blocks)),
            Frame::List(list) => self.push_block(Block::List(list)),
            Frame::Item(item) => {
                if let Some(Frame::List(list)) = self.frames.last_mut() {
                    list.items.push(item);
                }
            }
            Frame::Table(table) => {
                if let Some(block) = table.into_block() {
                    self.push_block(block);
                }
            }
            Frame::Row(row) => {
                if let Some(Frame::Table(table)) = self.frames.last_mut() {
                    table.end_row(row);
                }
            }
            Frame::Cell { columns } => {
                let spans = std::mem::take(&mut self.line);
                self.pending_break = false;
                self.behind_space = true;
                if let Some(Frame::Row(row)) = self.frames.last_mut() {
                    row.end_cell(spans, columns);
                }
            }
        }
    }

    /// The document, with all that is still open ended.
    pub(super) fn finish(mut self) -> Rich {
        self.end_pre();
        if !self.elements.is_empty() {
            self.pop_to(0);
        }
        self.close_frames(0);
        self.flush_line();
        self.flush_word_items();
        let blocks = settle_notes(self.root, &self.note_labels);
        Rich { blocks }
    }
}

/// Numbers Word's footnotes in the order they are referred to and puts them in that order. A reference to a note that is
/// not there keeps its text, and a note nothing refers to stays as its paragraphs; the rule Word draws above its notes goes
/// with them.
fn settle_notes(mut blocks: Vec<Block>, labels: &HashMap<String, String>) -> Vec<Block> {
    let defined: HashSet<String> = blocks
        .iter()
        .filter_map(|block| match block {
            Block::Note { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mut numbers: HashMap<String, usize> = HashMap::new();
    visit_spans(&mut blocks, &mut |span| {
        if let Piece::Note(note) = &span.piece {
            span.piece = if defined.contains(note) {
                let next = numbers.len() + 1;
                Piece::Note(numbers.entry(note.clone()).or_insert(next).to_string())
            } else {
                Piece::Text(labels.get(note).cloned().unwrap_or_default())
            };
        }
    });
    let mut out = Vec::with_capacity(blocks.len());
    let mut notes = Vec::new();
    for block in blocks {
        match block {
            Block::Note { name, blocks } => match numbers.get(&name) {
                Some(number) => {
                    if notes.is_empty() && matches!(out.last(), Some(Block::Rule)) {
                        out.pop();
                    }
                    notes.push((*number, blocks));
                }
                None => out.extend(blocks),
            },
            block => out.push(block),
        }
    }
    notes.sort_by_key(|(number, _)| *number);
    out.extend(notes.into_iter().map(|(number, blocks)| Block::Note {
        name: number.to_string(),
        blocks,
    }));
    out
}
