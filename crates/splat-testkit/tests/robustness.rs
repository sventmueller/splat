//! Randomised documents and edits: every command must return an edit inside the text and on char boundaries.

use std::ops::Range;

use splat_markdown::commands::{self, Edit, InlineStyle, ListStyle, Pending, Toggle};
use splat_markdown::doc::{AlertKind, Doc};
use splat_markdown::docx::{WordExport, stored_entries, to_docx};
use splat_markdown::highlight::highlight;
use splat_markdown::incremental::{Parsed, parse_and_color, reparse_and_color};
use splat_markdown::reader::parse;
use splat_markdown::table::{TableOp, table_edit};
use splat_markdown::text::{Byte, Direction, TextAt, bytes, offsets};
use splat_preview::bridge::Bridge;
use splat_preview::layout::{NoImages, caret_shapes_layout, layout, relayout, reusable, reusable_behind_caret};
use splat_preview::map::{Index, Shown, preview_offset};
use splat_testkit::fuzz_checks::{edit_fits, xml_problem};
use splat_testkit::{Reporting, Rng, TimeLimit, rounds, time_limit, with_smallest_failure};

/// Far beyond what the rounds take when nothing hangs.
fn limited() -> TimeLimit {
    time_limit(
        std::time::Duration::from_secs(120)
            + std::time::Duration::from_millis(50) * u32::try_from(rounds(2000)).unwrap_or(u32::MAX),
    )
}

const PIECES: &[&str] = &[
    "Wort",
    " ",
    "ä",
    "😀",
    "x",
    "**",
    "*",
    "_",
    "~~",
    "`",
    "$",
    "$$",
    "[",
    "](#a)",
    ")",
    "<u>",
    "</u>",
    "<mark>",
    "</mark>",
    "<sup>",
    "</sup>",
    "\n",
    "\n\n",
    "> ",
    "> [!NOTE]\n> ",
    "- ",
    "1. ",
    "# ",
    "## ",
    "- [ ] ",
    "\\",
    "&amp;",
    "[^1]",
    "\n[^1]: Fuß\n",
    "| a | b |\n|---|---|\n| 1 | 2 |\n",
    "```\n",
    "```python\n",
    "  ",
    "\t",
    "---\n",
    "<br>",
    "![b](x.png)",
    "==",
    "^",
    "~",
    "||",
    ":rocket:",
    ":",
    "%%",
    "^[",
    "]",
    "[[Seite",
    "|Titel]]",
    "![[bild.png]]",
    " {#id .k}",
    "\nBegriff\n\n: ",
    "> [!tip]- ",
    "$`",
    "`$",
    "\\(",
    "\\)",
    "```math\n",
    "```dot\n",
    "#tag",
    "@user",
    "#12",
    "[@doe]",
    "\"",
    "--",
    "...",
    "<details>\n<summary>S</summary>\n\n",
    "\n\n</details>\n",
    "<p align=\"center\">",
    "</p>",
    "<img src=\"x.png\" width=10>",
    "<b>",
    "</b>",
    "<a href=\"#a\">",
    "</a>",
    "<table><tr><td colspan=2>z</td></tr></table>",
    "<!-- c -->",
    "<kbd>",
    "</kbd>",
    "\r",
    "\r\n",
    "\u{1b}",
    "\u{200d}",
    "\u{feff}",
    "[a]: b\n",
    "[a](b\n\"c\") ",
    "[a]",
    "[SS]: http://a\n\n",
    "[ß]: http://b\n\n",
    "[ß]",
    "[ss]",
];

fn document(rng: &mut Rng) -> String {
    (0..rng.below(40) + 1).map(|_| *rng.pick(PIECES)).collect()
}

fn check(text: &str, edit: &Edit, what: &str) -> String {
    edit_fits(text, edit, what)
}

#[test]
fn source_commands_never_break_the_text() {
    let _limit = limited();
    let mut rounds_rng = Rng::new(0x5eed_1234_abcd_0001);
    for _ in 0..rounds(400) {
        let seed = rounds_rng.below(usize::MAX) as u64 | 1;
        with_smallest_failure(&document(&mut rounds_rng), |text| source_commands_on(text, seed));
    }
}

fn source_commands_on(text: &str, seed: u64) {
    let text = text.to_string();
    let mut rng = Rng::new(seed);
    {
        let doc = parse(&text);
        highlight(&doc);
        let sel = rng.range(&text);
        let styles = [
            InlineStyle::Bold,
            InlineStyle::Italic,
            InlineStyle::Code,
            InlineStyle::Highlight,
            InlineStyle::Superscript,
        ];
        let style = styles[rng.below(styles.len())];
        match commands::toggle(&doc, sel.clone(), style, None) {
            Toggle::Edit(e) => {
                check(&text, &e, "toggle");
            }
            Toggle::Pending(Some(p)) => {
                if let Some(plan) = commands::typing_plan(&doc, &p) {
                    let placed = commands::place(&plan, "neu");
                    let mut typed = text.clone();
                    typed.insert_str(p.at.get(), "neu");
                    check(&typed, &commands::move_typed(&typed, p.at, 3, &placed), "move_typed");
                }
            }
            Toggle::Pending(None) => {}
        }
        check(&text, &commands::set_heading(&parse(&text), sel.clone(), 2), "heading");
        check(
            &text,
            &commands::toggle_list(&text, sel.clone(), ListStyle::Task),
            "list",
        );
        check(&text, &commands::toggle_quote(&text, sel.clone()), "quote");
        check(
            &text,
            &commands::insert_alert(&doc, sel.clone(), AlertKind::Warning),
            "alert",
        );
        check(&text, &commands::code_block_language(&doc, sel.clone(), "rust"), "code");
        check(&text, &commands::remove_formatting(&doc, sel.clone()), "clear");
        if let Some(e) = commands::insert_toc(&doc, sel.clone()) {
            check(&text, &e, "toc");
        }
        let _ = commands::typing_plan(
            &doc,
            &Pending {
                at: sel.start,
                styles: vec![InlineStyle::Bold],
            },
        );
    }
}

#[test]
fn preview_edits_never_break_the_text() {
    let _limit = limited();
    let mut rounds_rng = Rng::new(0x0dd_ba11_c0ff_ee42 ^ 0x9e37_79b9);
    for _ in 0..rounds(300) {
        let seed = rounds_rng.below(usize::MAX) as u64 | 1;
        with_smallest_failure(&document(&mut rounds_rng), |text| preview_edits_on(text, seed));
    }
}

fn preview_edits_on(text: &str, seed: u64) {
    let text = text.to_string();
    let mut rng = Rng::new(seed);
    {
        let doc = parse(&text);
        let caret = Some(rng.boundary(&text));
        let l = layout(&doc, &NoImages, caret);
        let index = Index::new(&l);
        let len: usize = l.blocks.iter().map(|b| b.len_u16).sum();
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let p = rng.below(len + 1);
        let q = (p + rng.below(6)).min(len);
        let typed = ["a", "*", " ", "ä", "\n", "#"][rng.below(6)];
        if let Some(e) = bridge.replace(Shown::new(p)..Shown::new(q), typed) {
            check(&text, &e, "replace");
        }
        if let Some(e) = bridge.newline(Shown::new(p)) {
            check(&text, &e, "newline");
        }
        if let Some(e) = bridge.delete_at_block_start(Shown::new(p)) {
            check(&text, &e, "backspace");
        }
        if let Some(e) = bridge.tab(
            Shown::new(p),
            if rng.below(2) == 0 {
                Direction::Backward
            } else {
                Direction::Forward
            },
        ) {
            check(&text, &e, "tab");
        }
        let s = bridge.source_selection(Shown::new(p)..Shown::new(q));
        assert!(
            s.end <= Byte::new(text.len())
                && text.is_char_boundary(s.start.get())
                && text.is_char_boundary(s.end.get())
        );
        let back = preview_offset(&l, &index, rng.boundary(&text));
        assert!(back <= Shown::new(len), "{back:?} > {len}");
    }
}

/// A text too large to parse, or one whose parsing failed, has no blocks at all; commands must cope with that.
#[test]
fn commands_on_a_text_without_blocks_never_break_it() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_0b1a_0008);
    for _ in 0..rounds(400) {
        let text = document(&mut rng);
        let none = Parsed::plain(text.as_str().into()).doc;
        let sel = rng.range(&text);
        if let Toggle::Edit(e) = commands::toggle(&none, sel.clone(), InlineStyle::Bold, None) {
            check(&text, &e, "toggle");
        }
        check(&text, &commands::set_heading(&none, sel.clone(), 2), "heading");
        check(
            &text,
            &commands::toggle_list(&text, sel.clone(), ListStyle::Task),
            "list",
        );
        check(&text, &commands::toggle_quote(&text, sel.clone()), "quote");
        check(
            &text,
            &commands::insert_alert(&none, sel.clone(), AlertKind::Warning),
            "alert",
        );
        check(
            &text,
            &commands::code_block_language(&none, sel.clone(), "rust"),
            "code",
        );
        check(&text, &commands::remove_formatting(&none, sel.clone()), "clear");
        let mut empty = layout(&Doc::default(), &NoImages, None);
        empty.doc_version = none.version();
        let index = Index::new(&empty);
        let bridge = Bridge::new(&none, &empty, &index).unwrap();
        assert!(
            bridge
                .replace(Shown::new(0)..Shown::new(0), "x")
                .is_none_or(|e| e.range.end <= Byte::new(text.len()))
        );
    }
}

/// Ordinary documents: blocks in quotes, lists and tables with running text of every kind, where comrak's positions are
/// exact. In the wild mix of `document` they are not always, for example behind an inline footnote.
const BLOCKS: [&str; 5] = [
    "```\nzeile eins\nzeile zwei\n```\n\n",
    "~~~rust\nlet x = 1;\n~~~\n\n",
    "> ```\n> im zitat\n> ```\n\n",
    "    eingerückt\n\n",
    "vor\n**fett** und\n![x](y.png)text\nnach\n\n",
];

fn ordinary_document(rng: &mut Rng) -> String {
    const INLINE: [&str; 21] = [
        "Wort",
        "äöü",
        "😀",
        "**fett**",
        "*schräg*",
        "`code`",
        "[Link](#a)",
        "~~weg~~",
        "<u>unter</u>",
        "<mark>hell</mark>",
        "Caf&eacute;",
        "&rarr;",
        "&#233;",
        "a &amp; b",
        "\\*stern\\*",
        "\"Zitat\"",
        "'x'",
        "a -- b",
        "c --- d",
        "e...",
        "#tag",
    ];
    let line = |rng: &mut Rng| -> String {
        (0..rng.below(4) + 1)
            .map(|_| *rng.pick(&INLINE))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut text = String::new();
    for _ in 0..rng.below(4) + 1 {
        let (first, next) = match rng.below(7) {
            0 => ("> ", "> "),
            1 => ("- ", "  "),
            2 => ("1. ", "   "),
            3 => ("> - ", ">   "),
            4 => ("## ", ""),
            5 => ("", ""),
            _ => ("> > ", "> > "),
        };
        text.push_str(first);
        text.push_str(&line(rng));
        if first != "## " && rng.below(2) == 0 {
            text.push('\n');
            text.push_str(next);
            text.push_str(&line(rng));
        }
        text.push_str("\n\n");
        if rng.below(5) == 0 {
            text.push_str(&format!("| {} | b |\n|---|---|\n| {} | y |\n\n", line(rng), line(rng)));
        }
        match rng.below(8) {
            0 | 1 => text.push_str(rng.pick(&BLOCKS)),
            2 => text.push_str("- ```\n  in der liste\n  ```\n\n"),
            _ => {}
        }
    }
    text
}

/// Text the preview cannot place in the source can only be edited as a whole, so running text must always find its place.
#[test]
fn running_text_finds_its_place_in_the_source() {
    let _limit = limited();
    use splat_preview::layout::{Atom, ParaKind, RunMap};
    let mut rng = Rng::new(0x5eed_a119_000b);
    for _ in 0..rounds(2000) {
        let text = ordinary_document(&mut rng);
        let l = layout(&parse(&text), &NoImages, None);
        for p in l.blocks.iter().flat_map(|b| &b.paras) {
            for r in &p.runs {
                let RunMap::Atom { atom: Atom::Text, src } = &r.map else {
                    continue;
                };
                // Atoms by design: a line end and a callout title.
                let designed = r.text == " " || matches!(p.style.kind, ParaKind::AlertTitle);
                assert!(
                    designed,
                    "{:?} has no place in {:?} of {text:?}",
                    r.text,
                    text.at(src.clone())
                );
            }
        }
    }
}

/// Typing inside any ordinary block must show what was typed there, whatever the source around it looks like.
#[test]
fn typing_in_ordinary_documents_shows_what_was_typed() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_7e57_000c);
    let mut checked = 0;
    for _ in 0..rounds(600) {
        let text = ordinary_document(&mut rng);
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        // Inside a word: at the edge of a paragraph, formatting or marker the character may belong to either side.
        let mut p = 1 + rng.below(shown.len() - 2);
        while (0xDC00..0xE000).contains(&shown[p]) {
            p -= 1;
        }
        let alphabetic = |u: u16| char::from_u32(u32::from(u)).is_some_and(|c| c.is_alphabetic());
        if p == 0 || !alphabetic(shown[p - 1]) || !alphabetic(shown[p]) {
            continue;
        }
        let edit = bridge
            .replace(Shown::new(p)..Shown::new(p), "q")
            .expect("typing in a word");
        let new_text = check(&text, &edit, "typing");
        let mut expected = shown.clone();
        expected.insert(p, u16::from(b'q'));
        assert_eq!(
            String::from_utf16_lossy(&preview(&new_text)),
            String::from_utf16_lossy(&expected),
            "typed at {p} of {text:?} -> {new_text:?}"
        );
        checked += 1;
    }
    assert!(checked > rounds(600) / 4, "only {checked} checks ran");
}

/// Any mark typed into running text shows as just that mark, or is refused; refusals stay the exception.
#[test]
fn typing_any_mark_shows_it_or_leaves_the_text_alone() {
    let _limit = limited();
    const MARKS: [&str; 26] = [
        "*", "_", "[", "]", "(", ")", "!", "#", ":", "|", "~", "^", "$", "`", "&", "<", ">", "=", "-", "+", ".", ";",
        "%", "\\", "\"", "1",
    ];
    let mut rng = Rng::new(0x5eed_6d61_726b);
    let (mut checked, mut refused) = (0, 0);
    for _ in 0..rounds(600) {
        let text = ordinary_document(&mut rng);
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        let mut p = 1 + rng.below(shown.len() - 2);
        while (0xDC00..0xE000).contains(&shown[p]) {
            p -= 1;
        }
        // In text only, not next to a marker or between paragraphs.
        let visible = |u: u16| char::from_u32(u32::from(u)).is_some_and(|c| c.is_alphanumeric() || c == ' ');
        if p == 0 || !visible(shown[p - 1]) || !visible(shown[p]) {
            continue;
        }
        let mark = *rng.pick(&MARKS);
        checked += 1;
        let Some(edit) = bridge.replace(Shown::new(p)..Shown::new(p), mark) else {
            refused += 1;
            continue;
        };
        let new_text = check(&text, &edit, "typing");
        let mut expected = shown.clone();
        expected.insert(p, mark.encode_utf16().next().unwrap());
        assert_eq!(
            String::from_utf16_lossy(&preview(&new_text)),
            String::from_utf16_lossy(&expected),
            "typed {mark:?} at {p} of {text:?} -> {new_text:?}"
        );
    }
    assert!(checked > rounds(600) / 4, "only {checked} checks ran");
    assert!(refused * 100 <= checked, "{refused} of {checked} refused");
}

/// Plain words and paragraphs, where formatting twice must give back the text it started from.
fn plain_document(rng: &mut Rng) -> String {
    const WORDS: [&str; 6] = ["Wort", "ä", "zwei", "😀", "x", "Ende"];
    let mut text = String::new();
    for p in 0..rng.below(3) + 1 {
        if p > 0 {
            text.push_str("\n\n");
        }
        for w in 0..rng.below(5) + 1 {
            if w > 0 {
                text.push(' ');
            }
            text.push_str(WORDS[rng.below(WORDS.len())]);
        }
    }
    text
}

/// A selection from the start of one word to the end of another in the same paragraph.
fn words(rng: &mut Rng, text: &str) -> Range<usize> {
    let paragraphs: Vec<(usize, &str)> = text
        .split("\n\n")
        .scan(0, |at, p| {
            let start = *at;
            *at += p.len() + 2;
            Some((start, p))
        })
        .collect();
    let (base, para) = paragraphs[rng.below(paragraphs.len())];
    let spans: Vec<Range<usize>> = para
        .split(' ')
        .scan(0, |at, w| {
            let start = *at;
            *at += w.len() + 1;
            Some(base + start..base + start + w.len())
        })
        .collect();
    let a = rng.below(spans.len());
    let b = a + rng.below(spans.len() - a);
    spans[a].start..spans[b].end
}

fn applied(text: &str, edit: &Edit) -> String {
    let mut out = text.to_string();
    edit.apply_to(&mut out);
    out
}

#[test]
fn formatting_twice_gives_back_the_text() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_7ec7_0002);
    for _ in 0..rounds(400) {
        let text = plain_document(&mut rng);
        let sel = words(&mut rng, &text);
        for style in [
            InlineStyle::Bold,
            InlineStyle::Italic,
            InlineStyle::Strikethrough,
            InlineStyle::Code,
            InlineStyle::Highlight,
        ] {
            let Toggle::Edit(on) = commands::toggle(&parse(&text), bytes(sel.clone()), style, None) else {
                panic!("{style:?} on {sel:?} of {text:?} made no edit");
            };
            let styled = check(&text, &on, "on");
            assert_ne!(styled, text, "{style:?} changed nothing");
            let Toggle::Edit(off) = commands::toggle(&parse(&styled), on.selection.clone(), style, None) else {
                panic!("{style:?} off made no edit in {styled:?}");
            };
            assert_eq!(check(&styled, &off, "off"), text, "{style:?} on {sel:?} via {styled:?}");
        }
        for list in [ListStyle::Bullet, ListStyle::Numbered, ListStyle::Task] {
            let on = commands::toggle_list(&text, bytes(sel.clone()), list);
            let listed = check(&text, &on, "list");
            let off = commands::toggle_list(&listed, on.selection.clone(), list);
            assert_eq!(applied(&listed, &off), text, "{list:?} via {listed:?}");
        }
        let quoted = commands::toggle_quote(&text, bytes(sel.clone()));
        let back = commands::toggle_quote(&applied(&text, &quoted), quoted.selection.clone());
        assert_eq!(applied(&applied(&text, &quoted), &back), text, "quote");
        let level = 1 + u8::try_from(rng.below(6)).unwrap();
        let heading = commands::set_heading(&parse(&text), bytes(sel.clone()), level);
        let titled = applied(&text, &heading);
        let body = commands::set_heading(&parse(&titled), heading.selection.clone(), 0);
        assert_eq!(applied(&titled, &body), text, "heading {level} via {titled:?}");
    }
}

#[test]
fn inserting_and_deleting_rows_and_columns_gives_back_the_table() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_7ab1_0003);
    let mut checked = 0;
    for _ in 0..rounds(300) {
        let (rows, cols) = (1 + rng.below(4), 1 + rng.below(4));
        let cell = |r: usize, c: usize| ["a", "ä", "😀", "Wort"][(r + c) % 4].to_string();
        let mut text = format!(
            "Text\n\n|{}|\n|{}|\n",
            (0..cols).map(|c| cell(9, c)).collect::<Vec<_>>().join("|"),
            vec!["---"; cols].join("|")
        );
        for r in 0..rows {
            text.push_str(&format!(
                "|{}|\n",
                (0..cols).map(|c| cell(r, c)).collect::<Vec<_>>().join("|")
            ));
        }
        let at = text.find('|').unwrap() + 1;
        let format = table_edit(&parse(&text), Byte::new(at), TableOp::Format).expect("a table");
        let table = applied(&text, &format);
        // A cell of a body row, or of the header.
        let line = rng.below(rows + 1);
        let row_start = table
            .match_indices('|')
            .filter(|(i, _)| *i == 0 || table.as_bytes()[i - 1] == b'\n')
            .nth(if line == 0 { 0 } else { line + 1 })
            .unwrap()
            .0;
        let at = row_start + 2;
        for (insert, delete) in [
            (TableOp::InsertRowBelow, TableOp::DeleteRow),
            (TableOp::InsertRowAbove, TableOp::DeleteRow),
            (TableOp::InsertColumnRight, TableOp::DeleteColumn),
            (TableOp::InsertColumnLeft, TableOp::DeleteColumn),
        ] {
            let Some(grown) = table_edit(&parse(&table), Byte::new(at), insert) else {
                continue;
            };
            let bigger = check(&table, &grown, "insert");
            let shrunk = table_edit(&parse(&bigger), grown.selection.start, delete)
                .unwrap_or_else(|| panic!("{delete:?} found no table in {bigger:?}"));
            assert_eq!(
                check(&bigger, &shrunk, "delete"),
                table,
                "{insert:?} then {delete:?} via {bigger:?}"
            );
            checked += 1;
        }
    }
    assert!(checked > rounds(300) * 3, "only {checked} checks ran");
}

/// Pieces of a paragraph whose source differs from what the preview shows.
const MARKED: [&str; 11] = [
    " **fett**",
    " *kursiv*",
    " `code`",
    " [Link](#a)",
    " ~~weg~~",
    " <mark>hell</mark>",
    " Caf&eacute;",
    " &rarr;",
    " &#233;",
    " \\*stern\\*",
    " a &amp; b",
];

/// The text of the preview, with a line break after every paragraph.
fn preview(text: &str) -> Vec<u16> {
    let l = layout(&parse(text), &NoImages, None);
    l.blocks
        .iter()
        .flat_map(|b| &b.paras)
        .flat_map(|p| p.runs.iter().map(|r| r.text.as_str()).chain(["\n"]))
        .flat_map(|s| s.encode_utf16().collect::<Vec<_>>())
        .collect()
}

/// Typing a character in the preview must show exactly that character there once the source is laid out again.
#[test]
fn typing_in_the_preview_shows_what_was_typed() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_7e57_0004);
    let mut checked = 0;
    for round in 0..rounds(400) {
        let mut text = plain_document(&mut rng);
        if round % 2 == 1 {
            let at = text.find(' ').unwrap_or(text.len());
            text.insert_str(at, MARKED[rng.below(MARKED.len())]);
        }
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        let mut p = rng.below(shown.len());
        // Only inside a paragraph: typing behind its end extends the next block.
        while p > 0 && (0xDC00..0xE000).contains(&shown[p]) {
            p -= 1;
        }
        if shown[p] == u16::from(b'\n') {
            continue;
        }
        let typed = ["a", "ä", "*", "#", "_", "|", "1.", "-", ">", "&amp;", "`", "\\"][rng.below(12)];
        let edit = bridge
            .replace(Shown::new(p)..Shown::new(p), typed)
            .expect("typing in a paragraph");
        let new_text = check(&text, &edit, "typing");
        let mut expected = shown.clone();
        expected.splice(p..p, typed.encode_utf16());
        assert_eq!(
            String::from_utf16_lossy(&preview(&new_text)),
            String::from_utf16_lossy(&expected),
            "typed {typed:?} at {p} of {text:?} -> {new_text:?}"
        );
        checked += 1;
    }
    assert!(checked > rounds(400) / 2, "only {checked} checks ran");
}

/// The end of a paragraph is where most typing happens, and what follows it in the source must stay untouched.
#[test]
fn typing_at_the_end_of_a_paragraph_stays_in_it() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_e4d0_000a);
    let mut checked = 0;
    let mut refused = 0;
    for _ in 0..rounds(300) {
        let text = ordinary_document(&mut rng);
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        for pp in &index.paras {
            let p = pp.start.get() + pp.len;
            let Some(edit) = bridge.replace(Shown::new(p)..Shown::new(p), "a") else {
                refused += 1;
                continue;
            };
            let new_text = check(&text, &edit, "typing at a paragraph's end");
            let mut expected = shown.clone();
            expected.insert(p, u16::from(b'a'));
            assert_eq!(
                String::from_utf16_lossy(&preview(&new_text)),
                String::from_utf16_lossy(&expected),
                "at {p} of {text:?} -> {new_text:?}"
            );
            checked += 1;
        }
    }
    assert!(checked > rounds(300), "only {checked} checks ran");
    assert!(refused * 50 <= checked, "{refused} edits refused, {checked} made");
}

/// What is selected in a paragraph of the preview and deleted is gone once the source is laid out again, and nothing
/// else is. Spaces at the ends of a line and paragraphs left empty do not show in Markdown.
#[test]
fn deleting_in_the_preview_removes_what_was_selected() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_de1e_0009);
    let mut checked = 0;
    let lines = |shown: &[u16]| -> Vec<String> {
        String::from_utf16_lossy(shown)
            .split('\n')
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|l| !l.is_empty())
            .collect()
    };
    for round in 0..rounds(400) {
        let mut text = plain_document(&mut rng);
        if round % 2 == 1 {
            let at = text.find(' ').unwrap_or(text.len());
            text.insert_str(at, MARKED[rng.below(MARKED.len())]);
        }
        if round % 3 == 0 {
            text.push_str("\n\n");
            text.push_str(rng.pick(&BLOCKS));
        }
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        let first = rng.below(shown.len());
        let mut last = first + rng.below(8);
        last = last.min(shown.len());
        let boundary = |p: usize| p >= shown.len() || !(0xDC00..0xE000).contains(&shown[p]);
        if !boundary(first) || !boundary(last) || shown[first..last].contains(&u16::from(b'\n')) || first == last {
            continue;
        }
        let edit = bridge
            .replace(Shown::new(first)..Shown::new(last), "")
            .expect("deleting in a paragraph");
        let new_text = check(&text, &edit, "deleting");
        let mut expected = shown.clone();
        expected.drain(first..last);
        assert_eq!(
            lines(&preview(&new_text)),
            lines(&expected),
            "deleted {first}..{last} of {text:?} -> {new_text:?}"
        );
        checked += 1;
    }
    assert!(checked > rounds(400) / 3, "only {checked} checks ran");
}

/// Removing formatting takes markers away and adds nothing, whatever the document.
#[test]
fn removing_formatting_only_removes() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_0e11_000a);
    for _ in 0..rounds(400) {
        let text = document(&mut rng);
        // On any shorter text during the reduction: all of it, so that a failure can shrink.
        with_smallest_failure(&text, |text| {
            let edit = commands::remove_formatting(&parse(text), bytes(0..text.len()));
            let out = check(text, &edit, "clear");
            let mut kept = text.chars();
            assert!(out.chars().all(|c| kept.any(|k| k == c)), "{text:?} -> {out:?}");
        });
    }
}

/// A command for whole lines changes those lines, and an underline that belongs to them.
#[test]
fn line_commands_stay_on_their_lines() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_11e5_0001);
    for _ in 0..rounds(400) {
        let text = document(&mut rng);
        let sel = rng.range(&text);
        let span = commands::line_span(&text, &sel);
        for (name, edit, may_grow) in [
            ("heading", commands::set_heading(&parse(&text), sel.clone(), 2), true),
            (
                "list",
                commands::toggle_list(&text, sel.clone(), ListStyle::Bullet),
                false,
            ),
            ("quote", commands::toggle_quote(&text, sel.clone()), false),
        ] {
            check(&text, &edit, name);
            assert_eq!(edit.range.start, span.start, "{name} of {sel:?} in {text:?}");
            if !may_grow {
                assert_eq!(edit.range.end, span.end, "{name} of {sel:?} in {text:?}");
            }
            let lines_behind = text.at(span.end..edit.range.end).chars().filter(|c| *c == '\n').count();
            assert!(lines_behind <= 1, "{name} of {sel:?} in {text:?}: {edit:?}");
        }
    }
}

/// Return splits what is there and adds no text and loses none, whatever the block.
#[test]
fn return_keeps_every_visible_character() {
    use splat_preview::layout::{Atom, RunMap};
    let _limit = limited();
    let visible = |text: &str| -> String {
        layout(&parse(text), &NoImages, None)
            .blocks
            .iter()
            .flat_map(|b| &b.paras)
            .flat_map(|p| &p.runs)
            .filter(|r| !matches!(r.map, RunMap::Atom { atom: Atom::Marker, .. }))
            .flat_map(|r| r.text.chars())
            .filter(|c| !c.is_whitespace())
            .collect()
    };
    let mut rng = Rng::new(0x5eed_4e7e_0001);
    let mut checked = 0;
    let mut refused = 0;
    for _ in 0..rounds(400) {
        let text = ordinary_document(&mut rng);
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        let mut p = 1 + rng.below(shown.len() - 2);
        while (0xDC00..0xE000).contains(&shown[p]) {
            p -= 1;
        }
        let alphabetic = |u: u16| char::from_u32(u32::from(u)).is_some_and(|c| c.is_alphabetic());
        if p == 0 || !alphabetic(shown[p - 1]) || !alphabetic(shown[p]) {
            continue;
        }
        let Some(edit) = bridge.newline(Shown::new(p)) else {
            refused += 1;
            continue;
        };
        let out = check(&text, &edit, "return");
        assert_eq!(visible(&out), visible(&text), "return at {p} of {text:?} -> {out:?}");
        checked += 1;
    }
    assert!(checked > rounds(400) / 4, "only {checked} checks ran");
    assert!(refused * 50 <= checked, "{refused} edits refused, {checked} made");
}

/// Joining two paragraphs shows both their texts as they were, and what lay hidden between them stays in the source.
#[test]
fn joining_paragraphs_keeps_what_they_show_and_what_lies_between_them() {
    const TEXTS: [&str; 12] = [
        "Wort",
        "**fett**",
        "*schräg*",
        "`code`",
        "[Link](#a)",
        "~~weg~~",
        "<u>unter</u>",
        "Caf&eacute;",
        "😀 Wort",
        "# Titel",
        "- Punkt",
        "> Zitat",
    ];
    const HIDDEN: [&str; 3] = ["<!-- Notiz -->", "[r]: http://x.y", "%% Kommentar %%"];
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_101e_0001);
    let (mut joined, mut refused) = (0, 0);
    use splat_preview::layout::{Atom, RunMap};
    let shown = |text: &str| -> String {
        let l = layout(&parse(text), &NoImages, None);
        l.blocks
            .iter()
            .flat_map(|b| &b.paras)
            .flat_map(|p| &p.runs)
            .filter(|r| !matches!(r.map, RunMap::Atom { atom: Atom::Marker, .. }))
            .flat_map(|r| r.text.chars())
            .filter(|c| !c.is_whitespace())
            .collect()
    };
    for _ in 0..rounds(400) {
        let (a, b) = (*rng.pick(&TEXTS), *rng.pick(&TEXTS));
        let hidden = (rng.below(2) == 0).then(|| *rng.pick(&HIDDEN));
        let text = match hidden {
            Some(hidden) => format!("{a}\n\n{hidden}\n\n{b}\n"),
            None => format!("{a}\n\n{b}\n"),
        };
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown_paras: Vec<_> = index.paras.iter().filter(|p| p.len > 0).collect();
        let [first, second] = shown_paras[..] else {
            continue;
        };
        let at = second.start.get();
        // Empty paragraphs between them are breaks of their own.
        if at != first.start.get() + first.len + 1 {
            continue;
        }
        let Some(edit) = bridge.replace(Shown::new(at - 1)..Shown::new(at), "") else {
            refused += 1;
            continue;
        };
        let out = check(&text, &edit, "joining");
        assert_eq!(shown(&out), shown(&text), "{text:?} -> {out:?}");
        if let Some(hidden) = hidden {
            assert!(out.contains(hidden), "{text:?} -> {out:?}");
        }
        joined += 1;
    }
    assert!(joined > rounds(400) / 2, "only {joined} checks ran");
    assert!(refused * 10 <= joined, "{refused} joins refused, {joined} made");
}

/// Word refuses a document that is not well-formed XML, whatever stands in the text.
#[test]
fn word_documents_are_well_formed_xml() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_0d0c_0001);
    for _ in 0..rounds(300) {
        let text = document(&mut rng);
        let word = to_docx(
            &parse(&text),
            &WordExport {
                figure: &|_| None,
                image: &|_| None,
                quotes: None,
                reference: &|_| None,
                citation: &|_| None,
            },
        )
        .expect("a small document");
        let parts = stored_entries(&word);
        assert!(parts.iter().any(|(name, _)| name == "word/document.xml"), "{text:?}");
        for (name, part) in parts {
            if name.ends_with(".xml") || name.ends_with(".rels") {
                assert_eq!(xml_problem(&part), None, "{name} of {text:?}");
            }
        }
    }
}

/// What the preview shows of a position and what the source holds there lead back to each other.
#[test]
fn positions_map_back_and_forth_between_preview_and_source() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_a119_000d);
    let mut checked = 0;
    for _ in 0..rounds(400) {
        let text = ordinary_document(&mut rng);
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        for _ in 0..8 {
            let p = rng.below(shown.len() - 1);
            let letter = char::from_u32(u32::from(shown[p])).filter(|c| c.is_ascii_alphabetic());
            let Some(letter) = letter else { continue };
            if p == 0 || !char::from_u32(u32::from(shown[p - 1])).is_some_and(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            let selected = bridge.source_selection(Shown::new(p)..Shown::new(p + 1));
            assert_eq!(text.at(selected.clone()), letter.to_string(), "{p} of {text:?}");
            let at = bridge.source_at(Shown::new(p)).expect("a place");
            assert_eq!(at, selected.start, "{p} of {text:?}");
            assert_eq!(preview_offset(&l, &index, at), Shown::new(p), "{at:?} of {text:?}");
            checked += 1;
        }
    }
    assert!(checked > rounds(400), "only {checked} checks ran");
}

/// Typing over a selection in a paragraph shows the typed text in its place and leaves the rest of what is shown.
#[test]
fn typing_over_a_selection_replaces_exactly_it() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_de1e_000e);
    let mut checked = 0;
    let lines = |shown: &[u16]| -> Vec<String> {
        String::from_utf16_lossy(shown)
            .split('\n')
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|l| !l.is_empty())
            .collect()
    };
    for round in 0..rounds(400) {
        let mut text = plain_document(&mut rng);
        if round % 2 == 1 {
            let at = text.find(' ').unwrap_or(text.len());
            text.insert_str(at, MARKED[rng.below(MARKED.len())]);
        }
        if round % 3 == 0 {
            text.push_str("\n\n");
            text.push_str(rng.pick(&BLOCKS));
        }
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        let first = rng.below(shown.len());
        let last = (first + 1 + rng.below(6)).min(shown.len());
        let boundary = |p: usize| p >= shown.len() || !(0xDC00..0xE000).contains(&shown[p]);
        if !boundary(first) || !boundary(last) || shown[first..last].contains(&u16::from(b'\n')) {
            continue;
        }
        let edit = bridge
            .replace(Shown::new(first)..Shown::new(last), "q")
            .expect("typing over text");
        let new_text = check(&text, &edit, "typing over");
        let mut expected = shown.clone();
        expected.splice(first..last, [u16::from(b'q')]);
        assert_eq!(
            lines(&preview(&new_text)),
            lines(&expected),
            "typed over {first}..{last} of {text:?} -> {new_text:?}"
        );
        checked += 1;
    }
    assert!(checked > rounds(400) / 2, "only {checked} checks ran");
}

/// A caret that moves changes a layout only where `caret_shapes_layout` says, and only the block it left has to go.
#[test]
fn a_caret_that_moves_leaves_the_layout_where_it_does_not_shape_it() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_ca4e_0001);
    let (mut kept, mut laid) = (0, 0);
    for round in 0..rounds(1500) {
        let text = if round % 2 == 0 {
            document(&mut rng)
        } else {
            ordinary_document(&mut rng) + "\n\n\n\nEnde  \n\nmehr\n"
        };
        let doc = parse(&text);
        let carets = [
            (rng.below(4) > 0).then(|| rng.boundary(&text)),
            (rng.below(4) > 0).then(|| rng.boundary(&text)),
        ];
        let old = layout(&doc, &NoImages, carets[0]);
        let fresh = layout(&doc, &NoImages, carets[1]);
        let shapes = carets.iter().flatten().any(|&c| caret_shapes_layout(&text, c));
        let what = format!("{carets:?} of {text:?}");
        if shapes {
            laid += 1;
        } else {
            kept += 1;
            assert_eq!(old.blocks, fresh.blocks, "{what}");
        }
        let relaid = relayout(&doc, &NoImages, carets[1], old, &reusable_behind_caret(&doc, carets[0]));
        assert_eq!(relaid.blocks, fresh.blocks, "{what}");
    }
    assert!(
        kept > rounds(1500) / 10 && laid > rounds(1500) / 10,
        "{kept} kept, {laid} laid out"
    );
}

/// Parsing only around an edit must give exactly the tree a full parse gives, offsets included.
#[test]
fn reparsing_around_an_edit_matches_a_full_parse() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_1ac2_0005);
    let mut spliced = 0;
    for round in 0..rounds(3000) {
        // Several paragraphs, so most edits leave blocks on both sides untouched.
        // Footnotes mostly take a full parse here, so most documents leave them out; the others hold a reference.
        let with_footnotes = round % 4 == 0;
        let mut old: String = (0..rng.below(6) + 2)
            .map(|_| document(&mut rng))
            .map(|d| {
                if with_footnotes {
                    d
                } else {
                    d.replace("[^1]", "x").replace("^[", "^").replace("]:", "]")
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        if rng.below(2) == 0 {
            old.push_str("\n\n[ref]: https://example.com\n\nSiehe [ref] und [Text][ref].\n");
        }
        let _input = Reporting(&old);
        let doc = parse(&old);
        let range = rng.range(&old);
        let chosen = PIECES[rng.below(PIECES.len())];
        let plain = chosen
            .replace("[^1]", "y")
            .replace("[^", "[")
            .replace("^[", "^")
            .replace("]:", "]");
        let piece = if with_footnotes { chosen } else { plain.as_str() };
        let mut new = old.clone();
        new.replace_range(offsets(&range), if rng.below(4) == 0 { "" } else { piece });
        let old_spans = highlight(&doc);
        let old_caret = (rng.below(2) == 0).then(|| rng.boundary(&old));
        let old_layout = layout(&doc, &NoImages, old_caret);
        let parsed = reparse_and_color(doc, &old_spans, &new);
        let what = format!("edit {range:?} -> {piece:?} of {old:?} -> {new:?}, {:?}", parsed.splice);
        let full = parse_and_color(&new);
        assert_eq!(parsed.doc.blocks, full.doc.blocks, "{what}");
        assert_eq!(parsed.spans, full.spans, "colors: {what}");
        let Some(splice) = &parsed.splice else {
            continue;
        };
        spliced += 1;
        let new_caret = (rng.below(2) == 0).then(|| rng.boundary(&new));
        let reuse = reusable(splice, &parsed.doc, old_caret);
        let relaid = relayout(&parsed.doc, &NoImages, new_caret, old_layout, &reuse);
        let fresh = layout(&full.doc, &NoImages, new_caret);
        assert_eq!(relaid.blocks, fresh.blocks, "layout: {what}");
    }
    assert!(spliced > rounds(3000) / 5, "only {spliced} edits were parsed in part");
}

const TYPED_INTO: [&str; 45] = [
    "Wort und mehr Wörter",
    "**fett** *schief* `code` ~~weg~~",
    "# Titel\n",
    "Titel\n===",
    "- eins\n- zwei\n  - drei",
    "1. a\n2. b",
    "> Zitat\n> weiter",
    "> [!NOTE]\n> Hinweis",
    "```rust\nlet a = 1;\n```",
    "~~~\ncode\n~~~",
    "    indented code",
    "| a | b |\n|---|---|\n| 1 | 2 |",
    "[a]: https://x.de",
    "[a] und [Text][a]",
    "[^1]: Note",
    "Text[^1] mehr",
    "^[inline]",
    "<!-- comment -->",
    "<div>\nhtml\n</div>",
    "<pre>\nx\n\ny\n</pre>",
    "<script>\nlet x;\n\nlet y;\n</script>",
    "<style>\np{}\n\nq{}\n</style>",
    "<textarea>\na\n\nb\n</textarea>",
    "<?php x\n\ny ?>",
    "<![CDATA[\na\n\nb\n]]>",
    "<!DOCTYPE html>",
    "$$\nx^2\n$$",
    "\\(a\\) und \\[b\\]",
    "$`a`$",
    "%%\nversteckt\n%%",
    "```mermaid\ngraph TD; A-->B; %% c\n```",
    "Term\n: Definition",
    "---",
    "***",
    "- [ ] todo\n- [x] done",
    "![Bild](a.png)",
    "![[Einbettung]]",
    "==mark== ^sup^ ~sub~",
    "{: .class}",
    "---\ntitle: x\n---",
    "a  \nb",
    "&amp; &#123; \\* \\_",
    "日本語の文章 🧑‍🚀 é",
    "   ",
    "\t- tab",
];
const KEYSTROKES: [&str; 56] = [
    "a",
    "e",
    " ",
    "\n",
    "\n\n",
    "*",
    "**",
    "`",
    "```",
    "[",
    "]",
    "(",
    ")",
    "]:",
    "^[",
    "[^1]",
    "<",
    ">",
    "<!--",
    "-->",
    "%%",
    "$",
    "$$",
    "\\(",
    "\\)",
    "\\[",
    "\\]",
    "#",
    "# ",
    "- ",
    "> ",
    "1. ",
    "|",
    "---",
    "~~~",
    "<pre>",
    "</pre>",
    "<script>",
    "</script>",
    "<div>",
    "</div>",
    "&",
    ";",
    "\\",
    "\t",
    "    ",
    "\r\n",
    "ä",
    "🧑",
    "é",
    "=",
    ":",
    "!",
    "[!NOTE]",
    "^",
    "~",
];
fn on_a_letter(text: &str, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    // A view treats a line end of two characters as one.
    if at > 0 && at < text.len() && text.as_bytes()[at - 1] == b'\r' && text.as_bytes()[at] == b'\n' {
        at += 1;
    }
    at
}

/// Keystroke after keystroke, as typing goes: blocks, colors and layout stay those of a full parse, and what a partial parse
/// carries on stays what all of the text says. A single step from a fresh parse would not see what wears off over many.
#[test]
fn typing_on_and_on_reads_the_same_as_parsing_all_of_it() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_11c4_0002);
    let (mut steps, mut partial) = (0, 0);
    for round in 0..rounds(1500) {
        let picked: Vec<&str> = (0..rng.below(14) + 1).map(|_| *rng.pick(&TYPED_INTO)).collect();
        let mut text = picked.join("\n\n");
        if round % 5 == 0 {
            text = text.replace('\n', "\r\n");
        }
        let mut parsed = parse_and_color(&text);
        let mut caret = rng.below(text.len() + 1);
        for _ in 0..rng.below(40) + 2 {
            caret = on_a_letter(&text, caret);
            let mut new = text.clone();
            let key = *rng.pick(&KEYSTROKES);
            match rng.below(10) {
                0..=5 => {
                    new.insert_str(caret, key);
                    caret += key.len();
                }
                6 | 7 => {
                    let start = text[..caret].char_indices().next_back().map_or(0, |(at, _)| at);
                    new.replace_range(start..caret, "");
                    caret = start;
                }
                8 => {
                    caret = rng.below(text.len() + 1);
                    continue;
                }
                _ => {
                    let end = on_a_letter(&text, caret + rng.below(30));
                    new.replace_range(caret..end, key);
                    caret += key.len();
                }
            }
            steps += 1;
            let what = format!("{text:?} -> {new:?}");
            let spans = std::mem::take(&mut parsed.spans);
            let next = reparse_and_color(parsed.doc, &spans, &new);
            let full = parse_and_color(&new);
            assert_eq!(next.doc.blocks, full.doc.blocks, "{what}");
            assert_eq!(next.spans, full.spans, "colors: {what}");
            assert_eq!(next.doc.facts(), full.doc.facts(), "facts: {what}");
            assert_eq!(
                layout(&next.doc, &NoImages, None).blocks,
                layout(&full.doc, &NoImages, None).blocks,
                "layout: {what}"
            );
            partial += usize::from(next.splice.is_some());
            parsed = next;
            text = new;
        }
    }
    assert!(
        partial > steps / 4,
        "only {partial} of {steps} keystrokes were parsed in part"
    );
}

/// The same for ordinary documents with links defined anywhere: this is where typing has to stay partial.
#[test]
fn reparsing_around_links_with_definitions_matches_a_full_parse() {
    const BLOCKS: [&str; 16] = [
        "Wort und mehr",
        "Ein [a] und [Text][b] und [c]",
        "**fett** [a] *schief*",
        "[Direkt](https://x.de) [a]",
        "# Titel [b]",
        "- eins [a]\n- zwei\n  - drei [c]",
        "> Zitat [a]\n> weiter",
        "```\nx[0]: y\n[a]\n```",
        "```mermaid\n%%{init}%%\ngraph\n```",
        "| a | [a] |\n|---|---|\n| 1 | 2 |",
        "[a]: http://a.de",
        "[b]: <http://b.de> \"Titel\"",
        "[c]:\n  http://c.de\n  'Titel'",
        "> [d]: http://d.de",
        "- [e]: http://e.de",
        "Text a[0]: b",
    ];
    const TYPED: [&str; 12] = [
        "x",
        " ",
        "[",
        "]",
        "]:",
        "[a]",
        "[d]",
        "\n",
        "\n\n",
        "[neu]: http://n.de\n\n",
        "*",
        "`",
    ];
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_11c4_0001);
    let (mut spliced, mut whole) = (0, 0);
    for _ in 0..rounds(3000) {
        let mut picked: Vec<&str> = (0..rng.below(14) + 3).map(|_| *rng.pick(&BLOCKS)).collect();
        // A label defined twice takes a full parse.
        let mut seen = std::collections::HashSet::new();
        picked.retain(|b| !(b.contains("]:") && !b.contains("x[0]") && !b.contains("a[0]")) || seen.insert(*b));
        let old = picked.join("\n\n");
        let _input = Reporting(&old);
        let doc = parse(&old);
        let at = rng.boundary(&old);
        let mut new = old.clone();
        new.insert_str(at.get(), rng.pick(&TYPED));
        let spans = highlight(&doc);
        let parsed = reparse_and_color(doc, &spans, &new);
        let full = parse_and_color(&new);
        let what = format!("{at:?} of {old:?} -> {new:?}, {:?}", parsed.splice);
        assert_eq!(parsed.doc.blocks, full.doc.blocks, "{what}");
        assert_eq!(parsed.spans, full.spans, "colors: {what}");
        // A definition inside a quote or list takes a full parse.
        if parsed.splice.is_some() {
            spliced += 1;
        } else if !old.contains("> [d]") && !old.contains("- [e]") {
            whole += 1;
        }
    }
    assert!(
        spliced > whole,
        "{spliced} edits were parsed in part, {whole} as a whole"
    );
}

/// A document whose references all have definitions, written before, between or behind the rest of the text.
fn document_with_footnotes(rng: &mut Rng) -> String {
    const PLAIN: &[&str] = &[
        "Wort und mehr",
        "Fuß **fett** und *schief*",
        "# Titel",
        "- eins\n- zwei",
        "> Zitat",
        "```\ncode [^a-z]\n```",
        "| a | b |\n|---|---|\n| 1 | 2 |",
        "[Link](https://x.de)",
        "$$\nx\n$$",
    ];
    let notes = rng.below(5) + 1;
    let place = rng.below(3);
    let (mut text, mut behind) = (Vec::new(), Vec::new());
    for i in 0..notes {
        for _ in 0..rng.below(4) + 1 {
            text.push(PLAIN[rng.below(PLAIN.len())].to_string());
        }
        text.push(match rng.below(3) {
            0 => format!("Text[^n{i}] weiter"),
            1 => format!("Mit [^n{i}] und **fett[^n{i}]**"),
            _ => format!("- Punkt[^n{i}]"),
        });
        let definition = match rng.below(4) {
            0 => format!("[^n{i}]: Note\n    und mehr"),
            1 => format!("[^n{i}]: Note\n\n    Absatz"),
            2 if i + 1 < notes => format!("[^n{i}]: Note mit[^n{}]", i + 1),
            _ => format!("[^n{i}]: Note"),
        };
        if place == 0 || (place == 2 && rng.below(2) == 0) {
            behind.push(definition);
        } else {
            text.push(definition);
        }
    }
    for _ in 0..rng.below(3) {
        text.push(PLAIN[rng.below(PLAIN.len())].to_string());
    }
    text.extend(behind);
    text.join("\n\n") + if rng.below(4) == 0 { "" } else { "\n" }
}

/// Footnotes keep their numbers, colors and places in the preview when only what lies around them is parsed again.
#[test]
fn reparsing_around_footnotes_matches_a_full_parse() {
    let _limit = limited();
    const PIECES: &[&str] = &[
        "x",
        " ",
        "ß",
        "😀",
        "*",
        "`",
        "\n",
        "\n\n",
        "[",
        "]",
        "^",
        ":",
        "    ",
        "> ",
        "[^n0]",
        "[^n1]",
        "[^neu]",
        "[^n1]:",
        "\n[^neu]: Neu\n",
        "^[i]",
        "```",
    ];
    let mut rng = Rng::new(0x5eed_1ac2_0007);
    let mut spliced = 0;
    for _ in 0..rounds(3000) {
        let old = document_with_footnotes(&mut rng);
        let typing = rng.below(2) == 0;
        let range = if typing {
            let at = rng.boundary(&old);
            at..at
        } else {
            rng.range(&old)
        };
        let piece = if typing { "x" } else { PIECES[rng.below(PIECES.len())] };
        let mut new = old.clone();
        new.replace_range(offsets(&range), if !typing && rng.below(5) == 0 { "" } else { piece });
        let doc = parse(&old);
        let old_spans = highlight(&doc);
        let old_caret = (rng.below(2) == 0).then(|| rng.boundary(&old));
        let old_layout = layout(&doc, &NoImages, old_caret);
        let parsed = reparse_and_color(doc, &old_spans, &new);
        let what = format!("edit {range:?} -> {piece:?} of {old:?} -> {new:?}, {:?}", parsed.splice);
        let full = parse_and_color(&new);
        assert_eq!(parsed.doc.blocks, full.doc.blocks, "{what}");
        assert_eq!(parsed.spans, full.spans, "colors: {what}");
        let Some(splice) = &parsed.splice else {
            continue;
        };
        spliced += 1;
        let new_caret = (rng.below(2) == 0).then(|| rng.boundary(&new));
        let reuse = reusable(splice, &parsed.doc, old_caret);
        let relaid = relayout(&parsed.doc, &NoImages, new_caret, old_layout, &reuse);
        let fresh = layout(&full.doc, &NoImages, new_caret);
        assert_eq!(relaid.blocks, fresh.blocks, "layout: {what}");
    }
    assert!(spliced > rounds(3000) / 5, "only {spliced} edits were parsed in part");
}

const PENDING: [InlineStyle; 6] = [
    InlineStyle::Bold,
    InlineStyle::Italic,
    InlineStyle::Code,
    InlineStyle::Highlight,
    InlineStyle::Superscript,
    InlineStyle::Strikethrough,
];

/// Text typed in the source at a caret with pending styles moves into their markers and leaves the rest alone.
#[test]
fn typing_at_pending_styles_in_the_source_never_breaks_the_text() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_9e4d_0006);
    let mut checked = 0;
    for _ in 0..rounds(400) {
        let base = document(&mut rng);
        let at = rng.boundary(&base);
        let style = *rng.pick(&PENDING);
        let Toggle::Pending(Some(pending)) = commands::toggle(&parse(&base), at..at, style, None) else {
            continue;
        };
        let typed = *rng.pick(&["x", "neu", " ", " a", "ä😀"]);
        let mut text = base.clone();
        text.insert_str(pending.at.get(), typed);
        let Some((edit, keep_pending)) = commands::typed_at_pending(&parse(&base), &text, &pending) else {
            panic!("typing {typed:?} at {pending:?} of {base:?} was not recognised");
        };
        let out = check(&text, &edit, "typed at pending");
        assert!(out.contains(typed.trim()), "{typed:?} lost in {out:?}");
        assert_eq!(keep_pending, typed.trim().is_empty(), "{typed:?} -> {out:?}");
        checked += 1;
    }
    assert!(checked > rounds(400) / 10, "only {checked} checks ran");
}

/// The same in the preview: the edit fits the text, and whitespace keeps the styles waiting at the new caret.
#[test]
fn typing_at_pending_styles_in_the_preview_never_breaks_the_text() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_9e4d_0007);
    let mut checked = 0;
    for _ in 0..rounds(400) {
        let text = document(&mut rng);
        let doc = parse(&text);
        let at = rng.boundary(&text);
        let Toggle::Pending(Some(pending)) = commands::toggle(&doc, at..at, *rng.pick(&PENDING), None) else {
            continue;
        };
        let l = layout(&doc, &NoImages, Some(pending.at));
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let typed = *rng.pick(&["x", " ", "ä", "*"]);
        let p = preview_offset(&l, &index, pending.at);
        let Some((edit, next)) = bridge.type_with_pending(&pending, p, typed) else {
            continue;
        };
        let out = check(&text, &edit, "preview typed at pending");
        assert_eq!(next.is_some(), typed.trim().is_empty(), "{typed:?} -> {out:?}");
        if let Some(next) = next {
            assert_eq!(next.at, edit.selection.start, "{typed:?} -> {out:?}");
        }
        checked += 1;
    }
    assert!(checked > rounds(400) / 10, "only {checked} checks ran");
}

fn quick_reference_without_footnotes() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../resources/de.lproj/QuickReference.md"
    ))
    .unwrap()
    .replace("[^", "(")
    .replace("^[", "^(")
    .replace("]:", "] :")
}

/// The partial update of a 10 MB document after one keystroke, against a full one: same result, in a fraction of the time.
/// Run with `cargo test --release -- --ignored partial_updates`.
#[test]
#[ignore]
fn partial_updates_at_ten_megabytes() {
    let one = quick_reference_without_footnotes();
    let mut old = String::new();
    while old.len() < 10 << 20 {
        old.push_str(&one);
        old.push_str("\n\n");
    }
    keystroke_is_partial(&old, "Fett", "one keystroke at 10 MB", 8.0);
}

/// The quick reference with its footnotes numbered apart, so that every copy keeps its own, and without its inline note:
/// a thousand of those in one text switch them off, which only all of the text can tell.
#[test]
#[ignore]
fn partial_updates_of_the_quick_reference_as_it_ships() {
    let one = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../resources/de.lproj/QuickReference.md"
    ))
    .expect("the quick reference");
    let mut old = String::new();
    let mut copy = 0;
    while old.len() < 10 << 20 {
        old.push_str(&one.replace("[^", &format!("[^{copy}-")).replace("^[", "^("));
        old.push_str("\n\n");
        copy += 1;
    }
    keystroke_is_partial(
        &old,
        "## Listen",
        "one keystroke in the quick reference as it ships at 10 MB",
        8.0,
    );
}

/// The same with footnotes, half of them defined next to their references and half behind the text.
#[test]
#[ignore]
fn partial_updates_around_footnotes_at_ten_megabytes() {
    let one = quick_reference_without_footnotes();
    let (mut old, mut behind) = (String::new(), String::new());
    let mut note = 0;
    while old.len() + behind.len() < 10 << 20 {
        old.push_str(&one);
        old.push_str(&format!("\n\nNoted.[^n{note}]\n\n"));
        let definition = format!("[^n{note}]: A note.\n\n");
        if note % 2 == 0 {
            old.push_str(&definition);
        } else {
            behind.push_str(&definition);
        }
        note += 1;
    }
    old.push_str(&behind);
    keystroke_is_partial(&old, "Fett", "one keystroke around footnotes at 10 MB", 8.0);
}

/// The budget of a keystroke against what parsing all of the text takes, so that it does not depend on the machine.
#[test]
fn a_keystroke_in_a_megabyte_stays_far_below_a_full_update() {
    let one = quick_reference_without_footnotes();
    let mut old = String::new();
    while old.len() < 1 << 20 {
        old.push_str(&one);
        old.push_str("\n\n");
    }
    let _limit = limited();
    keystroke_is_partial(&old, "Fett", "one keystroke at 1 MB", 3.0);
}

/// `times_faster` than a full update, on the shortest of some runs.
fn keystroke_is_partial(old: &str, word: &str, what: &str, times_faster: f64) {
    let middle = old.floor_char_boundary(old.len() / 2);
    let at = old[middle..].find(word).unwrap() + middle + word.len();
    let mut new = old.to_string();
    new.insert(at, 'x');
    // Other tests run at the same time: the fastest of some runs says what the code costs, a single one what they do.
    let (mut full_ms, mut partial_ms) = (f64::MAX, f64::MAX);
    for _ in 0..3 {
        let doc = parse(old);
        let spans = highlight(&doc);
        let old_layout = layout(&doc, &NoImages, None);
        // The app works these out when it loads a text, not at the keystroke.
        doc.facts();
        let started = std::time::Instant::now();
        let full = parse(&new);
        let full_spans = highlight(&full);
        let full_layout = layout(&full, &NoImages, None);
        full_ms = full_ms.min(started.elapsed().as_secs_f64() * 1e3);
        let started = std::time::Instant::now();
        let parsed = reparse_and_color(doc, &spans, &new);
        let splice = parsed.splice.as_ref().expect("a partial parse");
        let new_layout = relayout(
            &parsed.doc,
            &NoImages,
            None,
            old_layout,
            &reusable(splice, &parsed.doc, None),
        );
        partial_ms = partial_ms.min(started.elapsed().as_secs_f64() * 1e3);
        assert_eq!(parsed.doc.blocks, full.blocks);
        assert_eq!(parsed.spans, full_spans);
        assert_eq!(new_layout.blocks, full_layout.blocks);
    }
    eprintln!("{what}: full {full_ms:.0} ms, partial {partial_ms:.0} ms");
    assert!(
        partial_ms * times_faster < full_ms,
        "a keystroke got slower: partial {partial_ms:.0} ms, full {full_ms:.0} ms"
    );
}

#[test]
fn tokens_of_any_text_are_in_order_apart_and_on_character_boundaries() {
    const PIECES: [&str; 18] = [
        "def ", "fn ", "'", "\"", "\"\"\"", "#", "//", "/*", "*/", "<", ">", "--", "%%", "1", "ä", "😀", "\n", "\\",
    ];
    let mut rng = Rng::new(0x5eed_c0de_0001);
    for _ in 0..rounds(300) {
        let code: String = (0..rng.below(30)).map(|_| *rng.pick(&PIECES)).collect();
        for lang in splat_markdown::code::language_names() {
            let found = splat_markdown::code::tokens(lang, &code);
            let mut at = 0;
            for (range, _) in found {
                assert!(
                    range.start >= at && range.start < range.end,
                    "{}: {range:?} of {code:?}",
                    lang
                );
                assert!(
                    code.is_char_boundary(range.start) && code.is_char_boundary(range.end),
                    "{code:?}"
                );
                at = range.end;
            }
            assert!(at <= code.len());
        }
    }
}
