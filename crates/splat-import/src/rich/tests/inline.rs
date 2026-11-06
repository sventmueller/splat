//! Marks, escapes, links, pictures and formulas within a line.

use super::*;

#[test]
fn whitespace_moves_out_of_marks_and_runs_of_one_look_merge() {
    let bold = marks(&[Mark::Bold]);
    assert_eq!(
        markdown_of(vec![
            Span::text("a "),
            Span::text(" b ").marked(bold),
            Span::text("c").marked(bold),
            Span::text("  ").marked(marks(&[Mark::Italic])),
            Span::text("").marked(bold),
            Span::text("d"),
        ]),
        "a  **b c**  d\n"
    );
    assert_eq!(
        markdown_of(vec![Span::text("Hal").marked(bold), Span::text("lo").marked(bold)]),
        "**Hallo**\n"
    );
    assert_eq!(markdown_of(vec![Span::text("   ").marked(bold)]), "");
    assert_eq!(
        markdown_of(vec![
            Span::text("a").marked(bold),
            Span::text(" "),
            Span::text("b").marked(bold)
        ]),
        "**a b**\n"
    );
}

#[test]
fn a_mark_that_markers_cannot_open_is_written_as_a_tag() {
    assert_eq!(
        markdown_of(vec![
            Span::text("a"),
            Span::text("\"b\"").marked(marks(&[Mark::Bold])),
            Span::text("c"),
        ]),
        "a<strong>\"b\"</strong>c\n"
    );
    assert_eq!(
        markdown_of(vec![
            Span::text("中"),
            Span::text("「文」").marked(marks(&[Mark::Italic])),
            Span::text("字"),
        ]),
        "中<em>「文」</em>字\n"
    );
}

#[test]
fn emphasis_next_to_what_comrak_looks_past_is_written_as_a_tag() {
    let bold = marks(&[Mark::Bold]);
    assert_eq!(
        markdown_of(vec![Span::text("=\tx").marked(bold), Span::text("y")]),
        "<strong>=\tx</strong>y\n"
    );
    let cell = Rich {
        blocks: vec![Block::Table(Table {
            alignments: Vec::new(),
            rows: vec![vec![Cell::new(vec![
                Span::text("===\t\\||x||").marked(bold),
                Span::text("[a]#").marked(marks(&[Mark::Underline, Mark::Superscript])),
            ])]],
        })],
    };
    assert_eq!(survives(&cell), Ok(()));
}

#[test]
fn prose_is_not_escaped_more_than_it_needs() {
    for text in [
        "Version 1.2. It costs 5 $ or $5, 100 % of 2 * 3 = 6.",
        "snake_case and a - b, c + d, 12:30:45, AT&T, x > y, a < b, foo(bar) [1] see (x).",
        "~5 km, 2^10, wow! #tag, @name, a | b, C:\\Users",
        "Dies ist ein Satz. Noch ein Satz! 1. Mai",
    ] {
        assert_eq!(markdown_of(vec![Span::text(text)]), format!("{text}\n"));
    }
}

#[test]
fn line_starts_are_escaped_where_they_would_make_blocks() {
    for (text, written) in [
        ("# a", "\\# a"),
        ("#a", "#a"),
        ("> a", "\\> a"),
        ("- a", "\\- a"),
        ("-a", "-a"),
        ("+ a", "\\+ a"),
        ("1. a", "1\\. a"),
        ("1) a", "1\\) a"),
        ("1.5 a", "1.5 a"),
        ("--- ", "\\---"),
        ("* a", "\\* a"),
        ("[x] a", "\\[x] a"),
        ("[a]: b", "\\[a]: b"),
        (": a", "\\: a"),
    ] {
        assert_eq!(markdown_of(vec![Span::text(text)]), format!("{written}\n"));
    }
}

#[test]
fn links_keep_only_what_is_harmless_and_parse_with_spaces_and_parentheses() {
    let link = |url: &str| markdown_of(vec![Span::text("t").linked(url)]);
    assert_eq!(link("javascript:alert(1)"), "t\n");
    assert_eq!(link("java\tscript:x"), "t\n");
    assert_eq!(link("data:text/html,x"), "t\n");
    assert_eq!(link("file:///etc/passwd"), "t\n");
    assert_eq!(link("  "), "t\n");
    assert_eq!(link("#a"), "[t](#a)\n");
    assert_eq!(link("mailto:a@b.cd"), "[t](mailto:a@b.cd)\n");
    assert_eq!(link("a b.png"), "[t](<a b.png>)\n");
    assert_eq!(link("x(1).md"), "[t](x(1).md)\n");
    assert_eq!(link("x)1.md"), "[t](<x)1.md>)\n");
}

#[test]
fn pictures_keep_their_place_or_their_description() {
    let image = |source: Source| {
        Span::of(Piece::Image(Image {
            source,
            alt: "a [b]".into(),
        }))
    };
    assert_eq!(
        markdown_of(vec![image(Source::Address("x y.png".into()))]),
        "![a \\[b&#93;](<x y.png>)\n"
    );
    assert_eq!(
        markdown_of(vec![image(Source::Address("javascript:x".into()))]),
        "a [b]\n"
    );
    assert_eq!(
        markdown_of(vec![image(Source::Data {
            mime: "image/png",
            bytes: vec![1],
        })]),
        "a [b]\n"
    );
}

#[test]
fn pasted_pictures_go_where_they_are_placed() {
    let picture = |source| {
        Span::of(Piece::Image(Image {
            source,
            alt: "alt".into(),
        }))
    };
    let mut rich = paragraph(vec![
        picture(Source::Address("https://a.bc/p.png".into())),
        Span::text(" "),
        picture(Source::Data {
            mime: "image/png",
            bytes: vec![1, 2],
        }),
        Span::text(" "),
        picture(Source::Address("gone".into())),
    ]);
    let mut asked = Vec::new();
    rich.place_images(&mut |source| match source {
        ImageSource::Address(address) => {
            asked.push((*address).to_string());
            (*address != "gone").then(|| (*address).to_string())
        }
        ImageSource::Data { mime, bytes } => {
            asked.push(format!("{mime} {}", bytes.len()));
            Some("pictures/a b.png".to_string())
        }
    });
    assert_eq!(asked, ["https://a.bc/p.png", "image/png 2", "gone"]);
    assert_eq!(
        rich.into_markdown(),
        "![alt](https://a.bc/p.png) ![alt](<pictures/a b.png>) alt\n"
    );
}

#[test]
fn fonts_of_one_width_are_known_by_name() {
    for name in ["Menlo-Regular", "Courier New", "Consolas", "MONACO", "SF Mono"] {
        assert!(is_monospace_font(name), "{name}");
    }
    assert!(!is_monospace_font("Helvetica Neue") && !is_monospace_font("Monotype Corsiva"));
}

#[test]
fn brackets_stay_text_next_to_what_comrak_reads_as_the_end_of_a_formula() {
    let spans = vec![Span::text("[x] a"), Span::of(Piece::Code("\\]".into()))];
    assert_eq!(survives(&paragraph(spans.clone())), Ok(()));
    assert_eq!(markdown_of(spans), "<span></span>&#91;x] a`\\]`\n");
    let spans = vec![Span::text("a [^b"), Span::of(Piece::Code("\\]".into()))];
    assert_eq!(survives(&paragraph(spans.clone())), Ok(()));
    assert_eq!(markdown_of(spans), "a &#91;^b`\\]`\n");
}

#[test]
fn a_line_that_starts_like_a_definition_stays_a_line() {
    let spans = vec![Span::of(Piece::Code("a]:b".into())).linked("https://a.bc")];
    assert_eq!(survives(&paragraph(spans.clone())), Ok(()));
    assert_eq!(markdown_of(spans), "<span></span>[`a]:b`](https://a.bc)\n");
}

#[test]
fn control_characters_in_an_address_are_percent_encoded() {
    assert_eq!(markdown_of(vec![Span::text("t").linked("a\tb")]), "[t](a%09b)\n");
    assert_eq!(markdown_of(vec![Span::text("t").linked("a \tb")]), "[t](<a %09b>)\n");
}

#[test]
fn a_formula_that_dollars_cannot_hold_is_written_in_the_form_that_can() {
    let formula = |tex: &str| markdown_of(vec![Span::of(Piece::Math(tex.into()))]);
    assert_eq!(formula("a"), "$a$\n");
    assert_eq!(formula("a$b"), "$`a$b`$\n");
    assert_eq!(formula("a`$b"), "``$a`$b$``\n");
}

#[test]
fn long_runs_of_one_look_take_no_quadratic_time() {
    let url: Arc<str> = "https://a.bc/".repeat(10_000).into();
    let spans: Vec<Span> = (0..20_000)
        .map(|n| Span {
            piece: Piece::Text(format!("t{}", n % 2)),
            marks: marks(if n % 2 == 0 { &[Mark::Bold] } else { &[Mark::Italic] }),
            link: Some(url.clone()),
        })
        .collect();
    assert!(markdown_of(spans).len() > 20_000);
}
