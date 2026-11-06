//! Blocks as Splat writes them: lists, rules, notes and tables.

use super::*;

#[test]
fn the_writer_uses_the_markers_splat_writes() {
    let out = markdown_of(vec![
        Span::text("a").marked(marks(&[Mark::Bold])),
        Span::text(" "),
        Span::text("b").marked(marks(&[Mark::Italic])),
        Span::text(" "),
        Span::text("c").marked(marks(&[Mark::Strike])),
        Span::text(" "),
        Span::text("d").marked(marks(&[Mark::Underline])),
        Span::text(" "),
        Span::text("e").marked(marks(&[Mark::Highlight])),
        Span::text(" "),
        Span::text("f").marked(marks(&[Mark::Superscript])),
        Span::text(" "),
        Span::text("g").marked(marks(&[Mark::Subscript])),
        Span::text(" "),
        Span::of(Piece::Code("h".into())),
        Span::text(" "),
        Span::text("i").linked("https://a.bc"),
    ]);
    assert_eq!(
        out,
        "**a** *b* ~~c~~ <ins>d</ins> <mark>e</mark> <sup>f</sup> <sub>g</sub> `h` [i](https://a.bc)\n"
    );
}

#[test]
fn blocks_are_written_as_splat_writes_them() {
    let item = |text: &str| Item {
        task: None,
        blocks: vec![Block::Paragraph(vec![Span::text(text)])],
    };
    let rich = Rich {
        blocks: vec![
            Block::Heading {
                level: 2,
                spans: vec![Span::text("T")],
                anchor: Some("a b".into()),
            },
            Block::List(List {
                kind: ListKind::Bullet,
                items: vec![
                    Item {
                        task: Some(true),
                        blocks: vec![
                            Block::Paragraph(vec![Span::text("x")]),
                            Block::List(List {
                                kind: ListKind::Numbered { start: 3 },
                                items: vec![item("y"), item("z")],
                            }),
                        ],
                    },
                    Item {
                        task: Some(false),
                        blocks: vec![Block::Paragraph(vec![Span::text("w")])],
                    },
                ],
            }),
            Block::Quote(vec![
                Block::Paragraph(vec![Span::text("q")]),
                Block::Paragraph(vec![Span::text("r")]),
            ]),
            Block::Code {
                language: "rust".into(),
                text: "fn a() {\n\n}\n".into(),
            },
            Block::Rule,
            Block::Table(Table {
                alignments: vec![Align::Left, Align::Right, Align::Center],
                rows: vec![
                    vec![
                        Cell::new(vec![Span::text("A")]),
                        Cell::new(vec![Span::text("B")]),
                        Cell::new(vec![Span::text("C")]),
                    ],
                    vec![Cell::new(vec![Span::text("1|2")])],
                ],
            }),
            Block::Math("x^2".into()),
        ],
    };
    assert_eq!(
        rich.into_markdown(),
        "## T {#ab}\n\n- [x] x\n\n  3. y\n  4. z\n- [ ] w\n\n> q\n>\n> r\n\n```rust\nfn a() {\n\n}\n```\n\n---\n\n\
         | A | B | C |\n|:---|---:|:---:|\n| 1\\|2 | | |\n\n$$x^2$$\n"
    );
}

#[test]
fn a_rule_at_the_start_of_a_document_or_item_stays_a_rule() {
    let text = || Block::Paragraph(vec![Span::text("x")]);
    let rich = Rich {
        blocks: vec![
            Block::Rule,
            text(),
            Block::Rule,
            Block::List(List {
                kind: ListKind::Bullet,
                items: vec![Item {
                    task: None,
                    blocks: vec![Block::Rule, text()],
                }],
            }),
        ],
    };
    assert_eq!(survives(&rich), Ok(()));
    assert!(rich.into_markdown().starts_with("***\n"));
}

#[test]
fn lists_next_to_each_other_stay_two_lists() {
    let list = |kind| {
        Block::List(List {
            kind,
            items: vec![Item {
                task: None,
                blocks: vec![Block::Paragraph(vec![Span::text("x")])],
            }],
        })
    };
    let rich = Rich {
        blocks: vec![
            list(ListKind::Bullet),
            list(ListKind::Bullet),
            list(ListKind::Numbered { start: 1 }),
            list(ListKind::Numbered { start: 5 }),
        ],
    };
    assert_eq!(survives(&rich), Ok(()));
    assert_eq!(rich.into_markdown(), "- x\n\n* x\n\n1. x\n\n5) x\n");
}

#[test]
fn a_heading_and_a_table_header_without_their_bold_are_tidy_after_one_pass() {
    let looks_alike_without_bold = || {
        vec![
            Span::of(Piece::Note("1".into())),
            Span::text(" "),
            Span::text("a").marked(marks(&[Mark::Bold])),
        ]
    };
    let rich = Rich {
        blocks: vec![
            Block::Heading {
                level: 1,
                spans: looks_alike_without_bold(),
                anchor: None,
            },
            Block::Table(Table {
                alignments: vec![Align::Left],
                rows: vec![vec![Cell {
                    spans: looks_alike_without_bold(),
                    columns: 1,
                }]],
            }),
            Block::Note {
                name: "1".into(),
                blocks: vec![Block::Paragraph(vec![Span::text("b")])],
            },
        ],
    };
    let tidy = rich.tidied();
    assert_eq!(tidy.clone().tidied(), tidy);
}

#[test]
fn a_reference_without_a_definition_and_a_definition_without_a_reference_go() {
    let rich = Rich {
        blocks: vec![
            Block::Paragraph(vec![Span::of(Piece::Note("gone".into()))]),
            Block::Paragraph(vec![Span::text("a"), Span::of(Piece::Note("1".into()))]),
            Block::Note {
                name: "1".into(),
                blocks: vec![Block::Paragraph(vec![Span::text("first")])],
            },
            Block::Note {
                name: "1".into(),
                blocks: vec![Block::Paragraph(vec![Span::text("second")])],
            },
            Block::Note {
                name: "3".into(),
                blocks: vec![Block::Paragraph(vec![Span::text("unused")])],
            },
        ],
    };
    assert_eq!(rich.into_markdown(), "a[^1]\n\n[^1]: first\n");
}

#[test]
fn a_table_with_a_wide_cell_is_an_html_table() {
    let rich = Rich {
        blocks: vec![Block::Table(Table {
            alignments: vec![Align::Right],
            rows: vec![
                vec![Cell {
                    spans: vec![Span::text("a<b")],
                    columns: 2,
                }],
                vec![
                    Cell::new(vec![
                        Span::text("x").marked(marks(&[Mark::Bold, Mark::Italic])),
                        Span::of(Piece::Break),
                        Span::text("y").linked("https://a.bc/?a&b"),
                    ]),
                    Cell::new(vec![
                        Span::of(Piece::Code("c".into())),
                        Span::of(Piece::Math("m".into())),
                        Span::of(Piece::Image(Image {
                            source: Source::Address("i.png".into()),
                            alt: "\"".into(),
                        })),
                    ]),
                ],
            ],
        })],
    };
    assert_eq!(
        rich.into_markdown(),
        "<table>\n<tr><th colspan=\"2\" align=\"right\">a&lt;b</th></tr>\n\
         <tr><td align=\"right\"><strong><em>x</em></strong><br><a href=\"https://a.bc/?a&amp;b\">y</a></td>\
         <td><code>c</code><code>$m$</code><img src=\"i.png\" alt=\"&quot;\"></td></tr>\n</table>\n"
    );
}

#[test]
fn flat_items_nest_by_their_levels() {
    let flat = |level, list, kind, text: &str| FlatItem {
        level,
        list,
        kind,
        item: Item {
            task: None,
            blocks: vec![Block::Paragraph(vec![Span::text(text)])],
        },
    };
    let items = vec![
        flat(0, 1, ListKind::Bullet, "a"),
        flat(2, 1, ListKind::Numbered { start: 4 }, "b"),
        flat(2, 1, ListKind::Numbered { start: 1 }, "c"),
        flat(1, 1, ListKind::Bullet, "d"),
        flat(0, 1, ListKind::Bullet, "e"),
        flat(0, 2, ListKind::Bullet, "f"),
        flat(0, 2, ListKind::Numbered { start: 1 }, "g"),
    ];
    let rich = Rich {
        blocks: nest_list(items),
    };
    assert_eq!(
        rich.into_markdown(),
        "- a\n\n  4. b\n  5. c\n  - d\n- e\n\n* f\n\n1. g\n"
    );
    let nested_first = Rich {
        blocks: nest_list(vec![
            flat(3, 1, ListKind::Bullet, "h"),
            flat(0, 1, ListKind::Bullet, "i"),
        ]),
    };
    assert_eq!(nested_first.into_markdown(), "- h\n\n* i\n");
}

#[test]
fn only_a_table_of_a_reasonable_size_is_padded_to_its_widest_row() {
    let row = |cells: usize| vec![Cell::new(vec![Span::text("a")]); cells];
    let table = |width: usize, rows: usize| Rich {
        blocks: vec![Block::Table(Table {
            alignments: Vec::new(),
            rows: std::iter::once(row(width)).chain((0..rows).map(|_| row(1))).collect(),
        })],
    };
    assert!(table(2, 1).into_markdown().ends_with("| a | |\n"));
    assert!(table(1100, 1000).into_markdown().ends_with("\n| a |\n"));
}

#[test]
fn a_note_in_an_html_table_shows_its_name() {
    let rich = Rich {
        blocks: vec![
            Block::Table(Table {
                alignments: Vec::new(),
                rows: vec![vec![Cell {
                    spans: vec![Span::text("a"), Span::of(Piece::Note("1".into()))],
                    columns: 2,
                }]],
            }),
            Block::Note {
                name: "1".into(),
                blocks: vec![Block::Paragraph(vec![Span::text("n")])],
            },
        ],
    };
    assert!(rich.into_markdown().contains("<th colspan=\"2\">a<sup>1</sup></th>"));
}
