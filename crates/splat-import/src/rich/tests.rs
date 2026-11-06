//! What defines the writer: Markdown read back with Splat's reader is what the model held.

use splat_markdown::doc::{Block as Read, BlockKind, Inline, InlineKind, ListKind as ReadList, plain_text};
use splat_markdown::markup::{self, Node};
use splat_markdown::reader::parse;
use splat_markdown::style;
use splat_testkit::{Rng, rounds};

use super::*;

mod blocks;
mod inline;
mod random;

fn marks(list: &[Mark]) -> Marks {
    Marks::from(list)
}

fn paragraph(spans: Vec<Span>) -> Rich {
    Rich {
        blocks: vec![Block::Paragraph(spans)],
    }
}

fn markdown_of(spans: Vec<Span>) -> String {
    paragraph(spans).into_markdown()
}

/// The inlines of a parsed document as spans.
fn read_spans(inlines: &[Inline], looks: Marks, link: Option<&Arc<str>>, out: &mut Vec<Span>) {
    for inline in inlines {
        let piece = match inline.kind() {
            InlineKind::Text(text) => Piece::Text(text.to_string()),
            InlineKind::SoftBreak => Piece::Text(" ".to_string()),
            InlineKind::LineBreak => Piece::Break,
            InlineKind::Code(code) => Piece::Code(code.to_string()),
            InlineKind::FootnoteRef { name, .. } => Piece::Note(name.to_string()),
            InlineKind::Math { literal, .. } => Piece::Math(literal.to_string()),
            InlineKind::Image { url, .. } => Piece::Image(Image {
                source: Source::Address(url.to_string()),
                alt: plain_text(inline.children()),
            }),
            InlineKind::Strong => {
                read_spans(inline.children(), looks.with(Mark::Bold, true), link, out);
                continue;
            }
            InlineKind::Emph => {
                read_spans(inline.children(), looks.with(Mark::Italic, true), link, out);
                continue;
            }
            InlineKind::Strikethrough => {
                read_spans(inline.children(), looks.with(Mark::Strike, true), link, out);
                continue;
            }
            InlineKind::Underline => {
                read_spans(inline.children(), looks.with(Mark::Underline, true), link, out);
                continue;
            }
            InlineKind::Highlight => {
                read_spans(inline.children(), looks.with(Mark::Highlight, true), link, out);
                continue;
            }
            InlineKind::Superscript => {
                read_spans(inline.children(), looks.with(Mark::Superscript, true), link, out);
                continue;
            }
            InlineKind::Subscript => {
                read_spans(inline.children(), looks.with(Mark::Subscript, true), link, out);
                continue;
            }
            InlineKind::Span => {
                read_spans(inline.children(), looks, link, out);
                continue;
            }
            InlineKind::Link { url, .. } => {
                let url: Arc<str> = url.as_ref().into();
                read_spans(inline.children(), looks, Some(&url), out);
                continue;
            }
            other => Piece::Text(format!("<unexpected {other:?}>")),
        };
        out.push(Span {
            piece,
            marks: looks,
            link: link.cloned(),
        });
    }
}

fn spans_of(inlines: &[Inline]) -> Vec<Span> {
    let mut out = Vec::new();
    read_spans(inlines, Marks::default(), None, &mut out);
    out
}

fn html_spans(nodes: &[Node], looks: Marks, link: Option<&Arc<str>>, out: &mut Vec<Span>) {
    for node in nodes {
        match node {
            Node::Text { text, .. } => out.push(Span {
                piece: Piece::Text(text.clone()),
                marks: looks,
                link: link.cloned(),
            }),
            Node::Element(element) => match element.name.as_str() {
                "br" => out.push(Span {
                    piece: Piece::Break,
                    marks: looks,
                    link: link.cloned(),
                }),
                "a" => {
                    let url: Arc<str> = element.attr("href").unwrap_or("").into();
                    html_spans(&element.children, looks, Some(&url), out);
                }
                "code" => {
                    let mut code = String::new();
                    markup::raw_text(&element.children, &mut code);
                    out.push(Span {
                        piece: Piece::Code(code),
                        marks: looks,
                        link: link.cloned(),
                    });
                }
                "img" => out.push(Span {
                    piece: Piece::Image(Image {
                        source: Source::Address(element.attr("src").unwrap_or("").to_string()),
                        alt: element.attr("alt").unwrap_or("").to_string(),
                    }),
                    marks: looks,
                    link: link.cloned(),
                }),
                name => {
                    let looks = match style::styled_element(name) {
                        Some(InlineKind::Strong) => looks.with(Mark::Bold, true),
                        Some(InlineKind::Emph) => looks.with(Mark::Italic, true),
                        Some(InlineKind::Strikethrough) => looks.with(Mark::Strike, true),
                        Some(InlineKind::Underline) => looks.with(Mark::Underline, true),
                        Some(InlineKind::Highlight) => looks.with(Mark::Highlight, true),
                        Some(InlineKind::Superscript) => looks.with(Mark::Superscript, true),
                        Some(InlineKind::Subscript) => looks.with(Mark::Subscript, true),
                        _ => looks,
                    };
                    html_spans(&element.children, looks, link, out);
                }
            },
        }
    }
}

fn read_table(literal: &str) -> Option<Table> {
    let nodes = markup::parse(literal);
    let table = markup::find(&nodes, markup::Role::Table)?;
    let rows = table
        .rows()
        .into_iter()
        .map(|(row, _)| {
            row.cells()
                .map(|cell| {
                    let mut spans = Vec::new();
                    html_spans(&cell.children, Marks::default(), None, &mut spans);
                    Cell {
                        spans,
                        columns: cell.span(),
                    }
                })
                .collect()
        })
        .collect();
    let first_row = table.rows().into_iter().next()?.0;
    let mut alignments = Vec::new();
    for cell in first_row.cells() {
        alignments.push(cell.align().unwrap_or_default());
        for _ in 1..cell.span() {
            alignments.push(Align::None);
        }
    }
    Some(Table { alignments, rows })
}

fn read_blocks(blocks: &[Read]) -> Vec<Block> {
    blocks.iter().map(read_block).collect()
}

fn read_block(block: &Read) -> Block {
    match &block.kind {
        BlockKind::Paragraph => Block::Paragraph(spans_of(block.inlines())),
        BlockKind::Heading { level, id, .. } => Block::Heading {
            level: *level,
            spans: spans_of(block.inlines()),
            anchor: id.as_ref().map(ToString::to_string),
        },
        BlockKind::BlockQuote => Block::Quote(read_blocks(block.children())),
        BlockKind::List { kind, .. } => Block::List(List {
            kind: match kind {
                ReadList::Bullet(_) => ListKind::Bullet,
                ReadList::Ordered { start, .. } => ListKind::Numbered { start: *start as u64 },
            },
            items: block
                .children()
                .iter()
                .map(|item| Item {
                    task: match &item.kind {
                        BlockKind::Item { task, .. } => task.as_ref().map(|t| t.checked),
                        _ => None,
                    },
                    blocks: read_blocks(item.children()),
                })
                .collect(),
        }),
        BlockKind::CodeBlock { info, literal, .. } if info.as_ref() == "math" => {
            Block::Math(literal.trim_end().to_string())
        }
        BlockKind::CodeBlock { info, literal, .. } => Block::Code {
            language: info.to_string(),
            text: literal.strip_suffix('\n').unwrap_or(literal).to_string(),
        },
        BlockKind::ThematicBreak => Block::Rule,
        BlockKind::Table { alignments } => Block::Table(Table {
            alignments: alignments.to_vec(),
            rows: block
                .children()
                .iter()
                .map(|row| {
                    row.children()
                        .iter()
                        .map(|c| Cell::new(spans_of(c.inlines())))
                        .collect()
                })
                .collect(),
        }),
        BlockKind::HtmlBlock { literal } => match read_table(literal) {
            Some(table) => Block::Table(table),
            None => Block::Code {
                language: "unexpected".into(),
                text: literal.to_string(),
            },
        },
        BlockKind::MathBlock { literal } => Block::Math(literal.to_string()),
        BlockKind::FootnoteDefinition { name, .. } => Block::Note {
            name: name.to_string(),
            blocks: read_blocks(block.children()),
        },
        other => Block::Code {
            language: "unexpected".into(),
            text: format!("{other:?}"),
        },
    }
}

/// What can be compared: tidied, with notes in order of name and tables as wide as their header.
fn comparable(mut rich: Rich) -> Rich {
    visit_spans(&mut rich.blocks, &mut |span| {
        // An address that shows itself may just as well be one comrak found by itself.
        if let (Piece::Text(text), Some(link)) = (&span.piece, &span.link)
            && ["", "http://", "https://", "mailto:"]
                .iter()
                .any(|scheme| format!("{scheme}{text}") == **link)
        {
            span.link = None;
        }
    });
    let mut rich = rich.tidied();
    let (mut notes, mut rest): (Vec<Block>, Vec<Block>) = rich
        .blocks
        .into_iter()
        .partition(|block| matches!(block, Block::Note { .. }));
    notes.sort_by_key(|block| match block {
        Block::Note { name, .. } => name.clone(),
        _ => String::new(),
    });
    rest.append(&mut notes);
    rich.blocks = rest;
    even_tables(&mut rich.blocks);
    rich.tidied()
}

fn even_tables(blocks: &mut [Block]) {
    for block in blocks {
        match block {
            Block::Table(table) => {
                let width = table
                    .rows
                    .iter()
                    .map(|row| row.iter().map(|c| c.columns as usize).sum())
                    .max()
                    .unwrap_or(0);
                table.alignments.resize(width, Align::None);
                if table.rows.iter().flatten().any(|c| c.columns > 1) {
                    // A cell's alignment stands for its first column, and HTML knows no formulas.
                    table.alignments.fill(Align::None);
                    for span in table.rows.iter_mut().flatten().flat_map(|c| &mut c.spans) {
                        if let Piece::Math(tex) = &span.piece {
                            span.piece = Piece::Code(format!("${tex}$"));
                        }
                    }
                } else {
                    for row in &mut table.rows {
                        row.resize(width, Cell::new(Vec::new()));
                    }
                    for span in table.rows.iter_mut().flatten().flat_map(|c| &mut c.spans) {
                        if let Some(link) = &span.link {
                            span.link = Some(link.replace("%7C", "|").into());
                        }
                        if let Piece::Image(Image {
                            source: Source::Address(address),
                            ..
                        }) = &mut span.piece
                        {
                            *address = address.replace("%7C", "|");
                        }
                        if let Piece::Code(code) = &span.piece
                            && code.contains("\\|")
                        {
                            span.piece = Piece::Text(code.clone());
                        }
                    }
                }
            }
            Block::List(list) => list.items.iter_mut().for_each(|item| even_tables(&mut item.blocks)),
            Block::Quote(blocks) | Block::Note { blocks, .. } => even_tables(blocks),
            _ => {}
        }
    }
}

/// What the reader finds in the Markdown of `rich`.
fn read_back(markdown: &str) -> Rich {
    let doc = parse(markdown);
    comparable(Rich {
        blocks: read_blocks(doc.blocks()),
    })
}

/// Where two documents part, with some lines around it, as a whole document says little.
fn first_difference(wanted: &Rich, got: &Rich) -> String {
    let wanted = format!("{wanted:#?}");
    let got = format!("{got:#?}");
    let wanted: Vec<&str> = wanted.lines().collect();
    let got: Vec<&str> = got.lines().collect();
    let at = wanted.iter().zip(got.iter()).position(|(a, b)| a != b).unwrap_or(0);
    let around = |lines: &[&str]| lines[at.saturating_sub(12)..(at + 6).min(lines.len())].join("\n");
    format!("wanted\n{}\ngot\n{}", around(&wanted), around(&got))
}

fn survives(rich: &Rich) -> Result<(), String> {
    let mut wanted = rich.clone();
    wanted.settle_images();
    let wanted = comparable(wanted.tidied());
    let markdown = rich.clone().into_markdown();
    let got = read_back(&markdown);
    if got == wanted {
        return Ok(());
    }
    Err(format!("{}\nof {markdown}", first_difference(&wanted, &got)))
}

/// Fails with the fewest spans and the shortest text that still go wrong.
fn assert_spans_survive(spans: Vec<Span>) {
    let fails = |spans: &Vec<Span>| survives(&paragraph(spans.clone())).is_err();
    if !fails(&spans) {
        return;
    }
    let mut smallest = spans;
    let mut at = 0;
    while at < smallest.len() {
        let mut fewer = smallest.clone();
        fewer.remove(at);
        if fails(&fewer) {
            smallest = fewer;
        } else {
            at += 1;
        }
    }
    for at in 0..smallest.len() {
        while let Piece::Text(text) = &smallest[at].piece {
            let shorter = (0..text.chars().count()).find_map(|skip| {
                let less: String = text
                    .chars()
                    .enumerate()
                    .filter(|(n, _)| *n != skip)
                    .map(|(_, c)| c)
                    .collect();
                let mut tried = smallest.clone();
                tried[at].piece = Piece::Text(less);
                fails(&tried).then_some(tried)
            });
            match shorter {
                Some(tried) => smallest = tried,
                None => break,
            }
        }
    }
    panic!("{}", survives(&paragraph(smallest)).unwrap_err());
}
