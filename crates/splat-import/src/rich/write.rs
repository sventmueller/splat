//! Blocks as Markdown, line by line with what the blocks around them put in front of each.

use std::fmt::Write;

use super::inline::{Context, longest_run, write_line};
use super::tidy::HIGHEST_NUMBER;
use super::{Align, Block, Cell, List, ListKind, Mark, Piece, Rich, Source, Span, Table};

/// `rich`, which `Rich::tidied` made, as Markdown.
pub(super) fn markdown(rich: &Rich) -> String {
    let mut writer = Writer::default();
    writer.blocks(&rich.blocks, Between::BlankLine, true);
    writer.out
}

#[derive(Default)]
struct Writer {
    out: String,
    /// What each line starts with.
    prefix: String,
    /// What the next line starts with instead, as the marker of a list item.
    lead: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Between {
    BlankLine,
    /// A nested list directly below the line of its item.
    NothingBeforeList,
}

impl Writer {
    fn line(&mut self, text: &str) {
        match self.lead.take() {
            Some(lead) => self.out.push_str(&lead),
            None if text.is_empty() => self.out.push_str(self.prefix.trim_end()),
            None => self.out.push_str(&self.prefix),
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn lines(&mut self, text: &str) {
        for line in text.split('\n') {
            self.line(line);
        }
    }

    fn blocks(&mut self, blocks: &[Block], between: Between, document: bool) {
        let mut before: Option<&Block> = None;
        let mut other_marker = false;
        for block in blocks {
            if before.is_some() && !(between == Between::NothingBeforeList && directly_below(block)) {
                self.line("");
            }
            other_marker = match (before, block) {
                (Some(Block::List(a)), Block::List(b)) => same_family(a, b) && !other_marker,
                _ => false,
            };
            self.block(block, other_marker, document && before.is_none());
            before = Some(block);
        }
    }

    fn block(&mut self, block: &Block, other_marker: bool, opens_document: bool) {
        match block {
            Block::Paragraph(spans) => {
                let mut text = String::new();
                write_line(spans, Context::default(), &mut text);
                if reads_as_definition(&text) {
                    text.insert_str(0, "<span></span>");
                }
                self.lines(&text);
            }
            Block::Heading { level, spans, anchor } => self.heading(*level, spans, anchor.as_deref()),
            Block::List(list) => self.list(list, other_marker),
            Block::Quote(blocks) => self.nested(blocks, "> ", "> ", Between::BlankLine),
            Block::Code { language, text } => self.code(language, text),
            Block::Rule => self.rule(opens_document),
            Block::Table(table) => self.table(table),
            Block::Math(tex) => self.math(tex),
            Block::Note { name, blocks } => self.nested(blocks, &format!("[^{name}]: "), "    ", Between::BlankLine),
        }
    }

    fn nested(&mut self, blocks: &[Block], first: &str, rest: &str, between: Between) {
        let before = self.prefix.len();
        let lead = self.lead.take().unwrap_or_else(|| self.prefix.clone());
        self.lead = Some(lead + first);
        self.prefix.push_str(rest);
        self.blocks(blocks, between, false);
        self.prefix.truncate(before);
    }

    fn heading(&mut self, level: u8, spans: &[Span], anchor: Option<&str>) {
        let mut text = "#".repeat(usize::from(level));
        text.push(' ');
        write_line(
            spans,
            Context {
                heading: true,
                ..Context::default()
            },
            &mut text,
        );
        if let Some(anchor) = anchor {
            let _ = write!(text, " {{#{anchor}}}");
        }
        self.line(&text);
    }

    fn list(&mut self, list: &List, other_marker: bool) {
        let tight = list.items.iter().all(|item| {
            matches!(item.blocks.split_first(), Some((Block::Paragraph(_), rest))
                if rest.iter().all(|block| matches!(block, Block::List(_))))
        });
        for (n, item) in list.items.iter().enumerate() {
            if n > 0 && !tight {
                self.line("");
            }
            let marker = match list.kind {
                ListKind::Bullet => (if other_marker { "* " } else { "- " }).to_string(),
                ListKind::Numbered { start } => {
                    let number = start.saturating_add(n as u64).min(HIGHEST_NUMBER);
                    format!("{number}{} ", if other_marker { ')' } else { '.' })
                }
            };
            let indent = " ".repeat(marker.len());
            let first = match item.task {
                Some(true) => marker + "[x] ",
                Some(false) => marker + "[ ] ",
                None => marker,
            };
            let between = if tight {
                Between::NothingBeforeList
            } else {
                Between::BlankLine
            };
            self.nested(&item.blocks, &first, &indent, between);
        }
    }

    fn code(&mut self, language: &str, text: &str) {
        let fence = "`".repeat(longest_run(text, '`').max(2) + 1);
        self.line(&format!("{fence}{language}"));
        self.lines(text);
        self.line(&fence);
    }

    /// `---` opening a document may read as the start of front matter, and behind the marker of a list item as a rule that
    /// ends the list.
    fn rule(&mut self, opens_document: bool) {
        let rule = if self.lead.is_some() {
            "___"
        } else if opens_document {
            "***"
        } else {
            "---"
        };
        self.line(rule);
    }

    /// `$$…$$` ends at the first `$$`, also one that a dollar at the end of the formula helps to make; such a formula
    /// needs the fence.
    fn math(&mut self, tex: &str) {
        if tex.contains("$$") || tex.starts_with('$') || tex.ends_with('$') {
            self.code("math", tex);
        } else {
            self.line(&format!("$${tex}$$"));
        }
    }

    fn table(&mut self, table: &Table) {
        if table.rows.iter().flatten().any(|cell| cell.columns > 1) {
            self.html_table(table);
            return;
        }
        let width = table.rows.iter().map(Vec::len).max().unwrap_or(0);
        let context = Context {
            table: true,
            ..Context::default()
        };
        for (n, row) in table.rows.iter().enumerate() {
            // A row with fewer cells reads the same, which keeps one wide row from making the others as long.
            let cells = if n == 0 || width.saturating_mul(table.rows.len()) <= MOST_PADDED_CELLS {
                width
            } else {
                row.len()
            };
            let mut text = String::from("|");
            for at in 0..cells {
                text.push(' ');
                if let Some(cell) = row.get(at).filter(|c| !c.spans.is_empty()) {
                    write_line(&cell.spans, context, &mut text);
                    text.push(' ');
                }
                text.push('|');
            }
            self.line(&text);
            if n == 0 {
                self.line(&delimiter_row(&table.alignments, width));
            }
        }
    }

    fn html_table(&mut self, table: &Table) {
        self.line("<table>");
        for (n, row) in table.rows.iter().enumerate() {
            let tag = if n == 0 { "th" } else { "td" };
            let mut text = String::from("<tr>");
            let mut column = 0;
            for cell in row {
                html_cell(cell, tag, table.alignments.get(column).copied(), &mut text);
                column += cell.columns as usize;
            }
            text.push_str("</tr>");
            self.line(&text);
        }
        self.line("</table>");
    }
}

/// More empty cells than a table can get from its rows being padded to the widest.
const MOST_PADDED_CELLS: usize = 1 << 20;

/// Whether a paragraph starts like a link reference definition, as a link can whose code holds `]:`. comrak looks for those
/// before it reads anything else.
fn reads_as_definition(text: &str) -> bool {
    if !text.starts_with('[') {
        return false;
    }
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            ']' => return chars.next() == Some(':'),
            _ => {}
        }
    }
    false
}

/// Whether a block can stand on the next line after a paragraph; a list with another first number than 1 cannot.
fn directly_below(block: &Block) -> bool {
    matches!(
        block,
        Block::List(List {
            kind: ListKind::Bullet | ListKind::Numbered { start: 1 },
            ..
        })
    )
}

fn same_family(a: &List, b: &List) -> bool {
    matches!(
        (a.kind, b.kind),
        (ListKind::Bullet, ListKind::Bullet) | (ListKind::Numbered { .. }, ListKind::Numbered { .. })
    )
}

fn delimiter_row(alignments: &[Align], width: usize) -> String {
    let mut row = String::from("|");
    for at in 0..width {
        row.push_str(match alignments.get(at) {
            Some(Align::Left) => ":---|",
            Some(Align::Center) => ":---:|",
            Some(Align::Right) => "---:|",
            Some(Align::None) | None => "---|",
        });
    }
    row
}

fn html_cell(cell: &Cell, tag: &str, align: Option<Align>, out: &mut String) {
    let _ = write!(out, "<{tag}");
    if cell.columns > 1 {
        let _ = write!(out, " colspan=\"{}\"", cell.columns);
    }
    if let Some(name) = align.and_then(Align::html_name) {
        let _ = write!(out, " align=\"{name}\"");
    }
    out.push('>');
    for span in &cell.spans {
        html_span(span, out);
    }
    let _ = write!(out, "</{tag}>");
}

fn html_span(span: &Span, out: &mut String) {
    let tags: Vec<&str> = Mark::ALL
        .into_iter()
        .filter(|mark| span.marks.has(*mark))
        .map(html_tag)
        .collect();
    if let Some(link) = &span.link {
        let _ = write!(out, "<a href=\"{}\">", html_escape(link));
    }
    for tag in &tags {
        let _ = write!(out, "<{tag}>");
    }
    match &span.piece {
        Piece::Text(text) => out.push_str(&html_escape(text)),
        Piece::Code(code) => {
            let _ = write!(out, "<code>{}</code>", html_escape(code));
        }
        Piece::Image(image) => {
            if let Source::Address(address) = &image.source {
                let _ = write!(
                    out,
                    "<img src=\"{}\" alt=\"{}\">",
                    html_escape(address),
                    html_escape(&image.alt)
                );
            }
        }
        Piece::Break => out.push_str("<br>"),
        Piece::Note(name) => {
            let _ = write!(out, "<sup>{}</sup>", html_escape(name));
        }
        Piece::Math(tex) => {
            let _ = write!(out, "<code>${}$</code>", html_escape(tex));
        }
    }
    for tag in tags.iter().rev() {
        let _ = write!(out, "</{tag}>");
    }
    if span.link.is_some() {
        out.push_str("</a>");
    }
}

fn html_tag(mark: Mark) -> &'static str {
    match mark {
        Mark::Bold => "strong",
        Mark::Italic => "em",
        Mark::Strike => "del",
        Mark::Underline => "ins",
        Mark::Highlight => "mark",
        Mark::Superscript => "sup",
        Mark::Subscript => "sub",
    }
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
