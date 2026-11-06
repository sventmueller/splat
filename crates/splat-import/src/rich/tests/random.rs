//! Random documents read back as they were.

use super::*;

const TEXTS: &[&str] = &[
    "Wort",
    "wort",
    " ",
    "  ",
    "ä",
    "中",
    "😀",
    "x",
    "1",
    "*",
    "**",
    "***",
    "_",
    "__",
    "~",
    "~~",
    "~~~",
    "^",
    "=",
    "==",
    "===",
    "|",
    "||",
    "$",
    "$$",
    "%",
    "%%",
    ":",
    "::",
    ":rocket:",
    ":100:",
    "12:30:45",
    "[",
    "]",
    "[[",
    "]]",
    "[[a]]",
    "[a]",
    "[a](b)",
    "[^1]",
    "^[",
    "![",
    "![a](b)",
    "(",
    ")",
    "<",
    ">",
    "<b>",
    "</b>",
    "<a href=\"x\">",
    "<!--",
    "-->",
    "<?",
    "<x@y.z>",
    "&",
    ";",
    "&amp;",
    "&#35;",
    "&nosuch;",
    "\\",
    "\\\\",
    "\\(",
    "\\[",
    "`",
    "``",
    "$`",
    "`$",
    "#",
    "# ",
    "## ",
    "#tag",
    "#12",
    "@user",
    "[@doe]",
    "> ",
    ">",
    "- ",
    "-",
    "--",
    "---",
    "+ ",
    "+",
    "1. ",
    "1) ",
    "1.",
    ".",
    "2.",
    ",",
    "!",
    "?",
    "\"",
    "'",
    "\t",
    "\u{a0}",
    "http://a.bc",
    "https://a.bc/d_e",
    "www.a.bc",
    "a@b.cd",
    "[x] ",
    "[ ] ",
    "[!NOTE]",
    "{#a}",
    "{",
    "}",
    "{.k}",
    "```",
    "    ",
    ": ",
    ":-",
    "|-|",
    "||x||",
    "==x==",
    "^x^",
    "~x~",
    "%%x%%",
    "$x$",
    "$$x$$",
    "*x*",
    "**x**",
    "_x_",
    "__x__",
    "~~x~~",
    "`x`",
    "\\*",
    "\\_",
    "a_b",
    "a*b",
    "2 * 3",
    "a_",
    "_a",
    "é",
    "—",
    "«",
    "€",
    "\u{200b}",
    "\n",
    "\r\n",
    "\u{1}",
];

const FORMULAS: &[&str] = &[
    "x",
    "x^2",
    "\\\\frac{a}{b}",
    "a_b",
    "a*b*c",
    "\\\\$5",
    "5$",
    "a$b",
    "x|y",
    "[a](b)",
    "a`b",
    "`a",
    "a\\\\",
    " s ",
    "1",
    "<b>",
    "==",
];

const URLS: &[&str] = &[
    "https://a.bc/d",
    "http://a.bc/d_e*f",
    "mailto:a@b.cd",
    "#anker",
    "../a/b.md",
    "a b.png",
    "x(1).md",
    "x(1.md",
    "x)1.md",
    "a|b",
    "a&amp;b",
    "a\\*b",
    "a<b>c",
    "ä ö.png",
    "((((((((((((((((((((a))))))))))))))))))))",
];

fn random_marks(rng: &mut Rng) -> Marks {
    let mut set = Marks::default();
    for mark in Mark::ALL {
        if rng.below(4) == 0 {
            set = set.with(mark, true);
        }
    }
    set
}

fn random_text(rng: &mut Rng) -> String {
    let mut text = String::new();
    for _ in 0..1 + rng.below(4) {
        text.push_str(rng.pick(TEXTS));
    }
    text
}

fn random_span(rng: &mut Rng, notes: bool) -> Span {
    let piece = match rng.below(14) {
        0 => Piece::Code(random_text(rng)),
        1 => Piece::Image(Image {
            source: Source::Address((*rng.pick(URLS)).to_string()),
            alt: random_text(rng),
        }),
        2 => Piece::Break,
        3 if notes => Piece::Note(rng.pick(&["1", "2"]).to_string()),
        4 => Piece::Math((*rng.pick(FORMULAS)).to_string()),
        _ => Piece::Text(random_text(rng)),
    };
    Span {
        piece,
        marks: random_marks(rng),
        link: (rng.below(5) == 0).then(|| (*rng.pick(URLS)).into()),
    }
}

fn random_spans(rng: &mut Rng, notes: bool) -> Vec<Span> {
    (0..1 + rng.below(5)).map(|_| random_span(rng, notes)).collect()
}

fn random_table(rng: &mut Rng, notes: bool) -> Table {
    let width = 1 + rng.below(3);
    let spanning = rng.below(3) == 0;
    let rows = (0..1 + rng.below(3))
        .map(|_| {
            (0..width)
                .map(|_| Cell {
                    spans: random_spans(rng, notes && !spanning),
                    columns: if spanning && rng.below(2) == 0 { 2 } else { 1 },
                })
                .collect()
        })
        .collect();
    let alignments = (0..width)
        .map(|_| *rng.pick(&[Align::None, Align::Left, Align::Center, Align::Right]))
        .collect();
    Table { alignments, rows }
}

fn random_block(rng: &mut Rng, depth: usize, notes: bool) -> Block {
    match rng.below(if depth > 3 { 6 } else { 9 }) {
        0 => Block::Heading {
            level: *rng.pick(&[1, 2, 3, 4, 5, 6]),
            spans: random_spans(rng, notes),
            anchor: (rng.below(4) == 0).then(|| "anker".to_string()),
        },
        1 => Block::Code {
            language: rng.pick(&["", "rust", "c++", "a b"]).to_string(),
            text: random_text(rng),
        },
        2 => Block::Rule,
        3 => Block::Table(random_table(rng, notes)),
        4 => Block::Math((*rng.pick(FORMULAS)).to_string()),
        5 => Block::Paragraph(random_spans(rng, notes)),
        6 => Block::Quote(random_blocks(rng, depth + 1, notes)),
        _ => Block::List(List {
            kind: if rng.below(2) == 0 {
                ListKind::Bullet
            } else {
                ListKind::Numbered {
                    start: rng.below(1200) as u64,
                }
            },
            items: (0..1 + rng.below(3))
                .map(|_| Item {
                    task: (rng.below(4) == 0).then(|| rng.below(2) == 0),
                    blocks: random_blocks(rng, depth + 1, notes),
                })
                .collect(),
        }),
    }
}

fn random_blocks(rng: &mut Rng, depth: usize, notes: bool) -> Vec<Block> {
    (0..1 + rng.below(3)).map(|_| random_block(rng, depth, notes)).collect()
}

/// Each way to make a document a little smaller.
fn smaller(blocks: &[Block]) -> Vec<Vec<Block>> {
    let mut out = Vec::new();
    let with = |at: usize, replacement: Vec<Block>| {
        let mut changed = blocks[..at].to_vec();
        changed.extend(replacement);
        changed.extend_from_slice(&blocks[at + 1..]);
        changed
    };
    for (at, block) in blocks.iter().enumerate() {
        out.push(with(at, Vec::new()));
        match block {
            Block::Quote(inner) => out.push(with(at, inner.clone())),
            Block::List(list) => {
                out.push(with(at, list.items.iter().flat_map(|i| i.blocks.clone()).collect()));
                for item in 0..list.items.len() {
                    let mut fewer = list.clone();
                    fewer.items.remove(item);
                    out.push(with(at, vec![Block::List(fewer)]));
                    for inner in smaller(&list.items[item].blocks) {
                        let mut changed = list.clone();
                        changed.items[item].blocks = inner;
                        out.push(with(at, vec![Block::List(changed)]));
                    }
                }
            }
            Block::Paragraph(spans) => {
                for span in 0..spans.len() {
                    let mut fewer = spans.clone();
                    fewer.remove(span);
                    out.push(with(at, vec![Block::Paragraph(fewer)]));
                }
            }
            Block::Table(table) => {
                for row in 0..table.rows.len() {
                    let mut fewer = table.clone();
                    fewer.rows.remove(row);
                    out.push(with(at, vec![Block::Table(fewer)]));
                    for cell in 0..table.rows[row].len() {
                        for span in 0..table.rows[row][cell].spans.len() {
                            let mut fewer = table.clone();
                            fewer.rows[row][cell].spans.remove(span);
                            out.push(with(at, vec![Block::Table(fewer)]));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Fails with the smallest part of a document that still goes wrong.
fn assert_document_survives(rich: Rich) {
    if survives(&rich).is_ok() {
        return;
    }
    let mut smallest = rich.blocks;
    while let Some(less) = smaller(&smallest)
        .into_iter()
        .find(|less| survives(&Rich { blocks: less.clone() }).is_err())
    {
        smallest = less;
    }
    panic!("{}", survives(&Rich { blocks: smallest }).unwrap_err());
}

#[test]
fn any_line_of_marks_reads_back_as_it_was() {
    let mut rng = Rng::new(0x51EA_7001);
    for _ in 0..rounds(20_000) {
        assert_spans_survive(random_spans(&mut rng, false));
    }
}

#[test]
fn any_text_alone_reads_back_literally() {
    let mut rng = Rng::new(0x51EA_7002);
    for _ in 0..rounds(20_000) {
        assert_spans_survive(vec![Span::text(random_text(&mut rng))]);
    }
}

#[test]
fn every_piece_of_text_stays_literal_in_every_mark() {
    for text in TEXTS {
        assert_spans_survive(vec![Span::text(*text)]);
        for mark in Mark::ALL {
            let marked = Span::text(*text).marked(marks(&[mark]));
            assert_spans_survive(vec![marked.clone()]);
            assert_spans_survive(vec![Span::text("a"), marked.clone(), Span::text("b")]);
            assert_spans_survive(vec![Span::text(*text), Span::text("b").marked(marks(&[mark]))]);
        }
    }
}

#[test]
fn any_document_reads_back_as_it_was() {
    let mut rng = Rng::new(0x51EA_7003);
    for _ in 0..rounds(6000) {
        let mut blocks = random_blocks(&mut rng, 0, true);
        blocks.push(Block::Paragraph(vec![
            Span::of(Piece::Note("1".into())),
            Span::of(Piece::Note("2".into())),
        ]));
        blocks.push(Block::Note {
            name: "1".into(),
            blocks: random_blocks(&mut rng, 3, false),
        });
        blocks.push(Block::Note {
            name: "2".into(),
            blocks: vec![Block::Paragraph(random_spans(&mut rng, false))],
        });
        let rich = Rich { blocks };
        let tidy = rich.clone().tidied();
        assert_eq!(tidy.clone().tidied(), tidy, "tidying once is enough");
        assert_document_survives(rich);
    }
}
