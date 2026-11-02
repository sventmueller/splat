//! Randomised documents and edits: every command must return an edit inside the text and on char boundaries.

use std::ops::Range;

use splat::fuzz::{Reporting, Rng, TimeLimit, rounds, time_limit, with_smallest_failure};
use splat::markdown::commands::{self, Edit, InlineStyle, ListStyle, Pending, Toggle};
use splat::markdown::doc::{AlertKind, parse};
use splat::markdown::docx::{WordExport, to_docx};
use splat::markdown::highlight::highlight;
use splat::markdown::incremental::{parse_and_color, reparse_and_color};
use splat::markdown::table::{TableOp, table_edit};
use splat::preview::bridge::Bridge;
use splat::preview::layout::{NoImages, layout, relayout, reusable};
use splat::preview::map::{Index, Shown, preview_offset};

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
];

fn document(rng: &mut Rng) -> String {
    (0..rng.below(40) + 1).map(|_| *rng.pick(PIECES)).collect()
}

fn check(text: &str, edit: &Edit, what: &str) -> String {
    assert!(
        edit.range.start <= edit.range.end && edit.range.end <= text.len(),
        "{what}: {edit:?} in {text:?}"
    );
    assert!(
        text.is_char_boundary(edit.range.start) && text.is_char_boundary(edit.range.end),
        "{what}: {edit:?} in {text:?}"
    );
    let mut out = text.to_string();
    edit.apply_to(&mut out);
    assert!(
        edit.selection.start <= edit.selection.end && edit.selection.end <= out.len(),
        "{what}: {edit:?} -> {out:?}"
    );
    assert!(
        out.is_char_boundary(edit.selection.start) && out.is_char_boundary(edit.selection.end),
        "{what}: {edit:?} -> {out:?}"
    );
    out
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
        highlight(&text, &doc);
        let sel = rng.range(&text);
        let styles = [
            InlineStyle::Bold,
            InlineStyle::Italic,
            InlineStyle::Code,
            InlineStyle::Highlight,
            InlineStyle::Superscript,
        ];
        let style = styles[rng.below(styles.len())];
        match commands::toggle(&text, &doc, sel.clone(), style, None) {
            Toggle::Edit(e) => {
                check(&text, &e, "toggle");
            }
            Toggle::Pending(Some(p)) => {
                if let Some(plan) = commands::typing_plan(&text, &doc, &p) {
                    let placed = commands::place(&plan, "neu");
                    let mut typed = text.clone();
                    typed.insert_str(p.at, "neu");
                    check(&typed, &commands::move_typed(&typed, p.at, 3, &placed), "move_typed");
                }
            }
            Toggle::Pending(None) => {}
        }
        check(&text, &commands::set_heading(&text, sel.clone(), 2), "heading");
        check(
            &text,
            &commands::toggle_list(&text, sel.clone(), ListStyle::Task),
            "list",
        );
        check(&text, &commands::toggle_quote(&text, sel.clone()), "quote");
        check(
            &text,
            &commands::insert_alert(&text, &doc, sel.clone(), AlertKind::Warning),
            "alert",
        );
        check(
            &text,
            &commands::code_block_language(&text, &doc, sel.clone(), "rust"),
            "code",
        );
        check(&text, &commands::remove_formatting(&text, &doc, sel.clone()), "clear");
        if let Some(e) = commands::insert_toc(&text, &doc, sel.clone()) {
            check(&text, &e, "toc");
        }
        let _ = commands::typing_plan(
            &text,
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
        let l = layout(&text, &doc, &NoImages, caret);
        let index = Index::new(&l);
        let len: usize = l.blocks.iter().map(|b| b.len_u16).sum();
        let bridge = Bridge {
            text: &text,
            doc: &doc,
            layout: &l,
            index: &index,
        };
        let p = rng.below(len + 1);
        let q = (p + rng.below(6)).min(len);
        let typed = ["a", "*", " ", "ä", "\n", "#"][rng.below(6)];
        if let Some(e) = bridge.replace(Shown(p)..Shown(q), typed) {
            check(&text, &e, "replace");
        }
        if let Some(e) = bridge.newline(Shown(p)) {
            check(&text, &e, "newline");
        }
        if let Some(e) = bridge.delete_at_block_start(Shown(p)) {
            check(&text, &e, "backspace");
        }
        if let Some(e) = bridge.tab(Shown(p), rng.below(2) == 0) {
            check(&text, &e, "tab");
        }
        let s = bridge.source_selection(Shown(p)..Shown(q));
        assert!(s.end <= text.len() && text.is_char_boundary(s.start) && text.is_char_boundary(s.end));
        let back = preview_offset(&l, &index, rng.boundary(&text));
        assert!(back <= Shown(len), "{back:?} > {len}");
    }
}

/// A text too large to parse, or one whose parsing failed, has no blocks at all; commands must cope with that.
#[test]
fn commands_on_a_text_without_blocks_never_break_it() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_0b1a_0008);
    let none = splat::markdown::doc::Doc::default();
    for _ in 0..rounds(400) {
        let text = document(&mut rng);
        let sel = rng.range(&text);
        if let Toggle::Edit(e) = commands::toggle(&text, &none, sel.clone(), InlineStyle::Bold, None) {
            check(&text, &e, "toggle");
        }
        check(&text, &commands::set_heading(&text, sel.clone(), 2), "heading");
        check(
            &text,
            &commands::toggle_list(&text, sel.clone(), ListStyle::Task),
            "list",
        );
        check(&text, &commands::toggle_quote(&text, sel.clone()), "quote");
        check(
            &text,
            &commands::insert_alert(&text, &none, sel.clone(), AlertKind::Warning),
            "alert",
        );
        check(
            &text,
            &commands::code_block_language(&text, &none, sel.clone(), "rust"),
            "code",
        );
        check(&text, &commands::remove_formatting(&text, &none, sel.clone()), "clear");
        let empty = layout("", &none, &NoImages, None);
        let index = Index::new(&empty);
        let bridge = Bridge {
            text: &text,
            doc: &none,
            layout: &empty,
            index: &index,
        };
        assert!(
            bridge
                .replace(Shown(0)..Shown(0), "x")
                .is_none_or(|e| e.range.end <= text.len())
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
    use splat::preview::layout::{Atom, ParaKind, RunMap};
    let mut rng = Rng::new(0x5eed_a119_000b);
    for _ in 0..rounds(2000) {
        let text = ordinary_document(&mut rng);
        let l = layout(&text, &parse(&text), &NoImages, None);
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
                    &text[src.clone()]
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
        let l = layout(&text, &doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge {
            text: &text,
            doc: &doc,
            layout: &l,
            index: &index,
        };
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
        let edit = bridge.replace(Shown(p)..Shown(p), "q").expect("typing in a word");
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
        let l = layout(&text, &doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge {
            text: &text,
            doc: &doc,
            layout: &l,
            index: &index,
        };
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
        let Some(edit) = bridge.replace(Shown(p)..Shown(p), mark) else {
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
            let Toggle::Edit(on) = commands::toggle(&text, &parse(&text), sel.clone(), style, None) else {
                panic!("{style:?} on {sel:?} of {text:?} made no edit");
            };
            let styled = check(&text, &on, "on");
            assert_ne!(styled, text, "{style:?} changed nothing");
            let Toggle::Edit(off) = commands::toggle(&styled, &parse(&styled), on.selection.clone(), style, None)
            else {
                panic!("{style:?} off made no edit in {styled:?}");
            };
            assert_eq!(check(&styled, &off, "off"), text, "{style:?} on {sel:?} via {styled:?}");
        }
        for list in [ListStyle::Bullet, ListStyle::Numbered, ListStyle::Task] {
            let on = commands::toggle_list(&text, sel.clone(), list);
            let listed = check(&text, &on, "list");
            let off = commands::toggle_list(&listed, on.selection.clone(), list);
            assert_eq!(applied(&listed, &off), text, "{list:?} via {listed:?}");
        }
        let quoted = commands::toggle_quote(&text, sel.clone());
        let back = commands::toggle_quote(&applied(&text, &quoted), quoted.selection.clone());
        assert_eq!(applied(&applied(&text, &quoted), &back), text, "quote");
        let level = 1 + u8::try_from(rng.below(6)).unwrap();
        let heading = commands::set_heading(&text, sel.clone(), level);
        let titled = applied(&text, &heading);
        let body = commands::set_heading(&titled, heading.selection.clone(), 0);
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
        let format = table_edit(&text, &parse(&text), at, TableOp::Format).expect("a table");
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
            let Some(grown) = table_edit(&table, &parse(&table), at, insert) else {
                continue;
            };
            let bigger = check(&table, &grown, "insert");
            let shrunk = table_edit(&bigger, &parse(&bigger), grown.selection.start, delete)
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
    let l = layout(text, &parse(text), &NoImages, None);
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
        let l = layout(&text, &doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge {
            text: &text,
            doc: &doc,
            layout: &l,
            index: &index,
        };
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
            .replace(Shown(p)..Shown(p), typed)
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
    for _ in 0..rounds(300) {
        let text = ordinary_document(&mut rng);
        let doc = parse(&text);
        let l = layout(&text, &doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge {
            text: &text,
            doc: &doc,
            layout: &l,
            index: &index,
        };
        let shown = preview(&text);
        for pp in &index.paras {
            let p = pp.start.0 + pp.len;
            let Some(edit) = bridge.replace(Shown(p)..Shown(p), "a") else {
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
        let l = layout(&text, &doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge {
            text: &text,
            doc: &doc,
            layout: &l,
            index: &index,
        };
        let shown = preview(&text);
        let first = rng.below(shown.len());
        let mut last = first + rng.below(8);
        last = last.min(shown.len());
        let boundary = |p: usize| p >= shown.len() || !(0xDC00..0xE000).contains(&shown[p]);
        if !boundary(first) || !boundary(last) || shown[first..last].contains(&u16::from(b'\n')) || first == last {
            continue;
        }
        let edit = bridge
            .replace(Shown(first)..Shown(last), "")
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
            let edit = commands::remove_formatting(text, &parse(text), 0..text.len());
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
            ("heading", commands::set_heading(&text, sel.clone(), 2), true),
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
            let lines_behind = text[span.end..edit.range.end].chars().filter(|c| *c == '\n').count();
            assert!(lines_behind <= 1, "{name} of {sel:?} in {text:?}: {edit:?}");
        }
    }
}

/// Return splits what is there and adds no text and loses none, whatever the block.
#[test]
fn return_keeps_every_visible_character() {
    use splat::preview::layout::{Atom, RunMap};
    let _limit = limited();
    let visible = |text: &str| -> String {
        layout(text, &parse(text), &NoImages, None)
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
    for _ in 0..rounds(400) {
        let text = ordinary_document(&mut rng);
        let doc = parse(&text);
        let l = layout(&text, &doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge {
            text: &text,
            doc: &doc,
            layout: &l,
            index: &index,
        };
        let shown = preview(&text);
        let mut p = 1 + rng.below(shown.len() - 2);
        while (0xDC00..0xE000).contains(&shown[p]) {
            p -= 1;
        }
        let alphabetic = |u: u16| char::from_u32(u32::from(u)).is_some_and(|c| c.is_alphabetic());
        if p == 0 || !alphabetic(shown[p - 1]) || !alphabetic(shown[p]) {
            continue;
        }
        let Some(edit) = bridge.newline(Shown(p)) else {
            continue;
        };
        let out = check(&text, &edit, "return");
        assert_eq!(visible(&out), visible(&text), "return at {p} of {text:?} -> {out:?}");
        checked += 1;
    }
    assert!(checked > rounds(400) / 4, "only {checked} checks ran");
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
                source: &text,
                figure: &|_| None,
                image: &|_| None,
                quotes: None,
                reference: &|_| None,
                citation: &|_| None,
            },
        );
        for (name, part) in stored_parts(&word) {
            if name.ends_with(".xml") || name.ends_with(".rels") {
                let xml = std::str::from_utf8(part).unwrap_or_else(|e| panic!("{name} of {text:?}: {e}"));
                if let Err(problem) = well_formed(xml) {
                    panic!("{name} of {text:?}: {problem} in {xml}");
                }
            }
        }
    }
}

/// The parts of a ZIP file written without compression.
fn stored_parts(zip: &[u8]) -> Vec<(String, &[u8])> {
    let mut parts = Vec::new();
    let mut at = 0;
    while zip.len() >= at + 30 && zip[at..at + 4] == [0x50, 0x4B, 0x03, 0x04] {
        let number = |from: usize, len: usize| {
            zip[from..from + len]
                .iter()
                .rev()
                .fold(0usize, |n, b| n << 8 | *b as usize)
        };
        let (size, name_len, extra) = (number(at + 18, 4), number(at + 26, 2), number(at + 28, 2));
        let body = at + 30 + name_len + extra;
        parts.push((
            String::from_utf8_lossy(&zip[at + 30..at + 30 + name_len]).into_owned(),
            &zip[body..body + size],
        ));
        at = body + size;
    }
    parts
}

/// Tags that nest, quoted attributes, references that exist, and no character XML forbids.
fn well_formed(xml: &str) -> Result<(), String> {
    let mut open: Vec<&str> = Vec::new();
    let mut rest = xml;
    if let Some(c) = xml
        .chars()
        .find(|c| (*c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r'))
    {
        return Err(format!("control character {c:?}"));
    }
    while let Some(at) = rest.find('<') {
        check_references(&rest[..at])?;
        rest = &rest[at..];
        if rest.starts_with("<?") {
            rest = &rest[rest.find("?>").ok_or("open declaration")? + 2..];
            continue;
        }
        let end = tag_end(rest).ok_or_else(|| format!("open tag {:?}", &rest[..rest.len().min(40)]))?;
        let tag = &rest[1..end];
        if let Some(name) = tag.strip_prefix('/') {
            match open.pop() {
                Some(top) if top == name.trim() => {}
                other => return Err(format!("</{name}> closes {other:?}")),
            }
        } else {
            let name = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
            if name.is_empty() {
                return Err(format!("nameless tag {tag:?}"));
            }
            check_references(tag)?;
            if !tag.ends_with('/') {
                open.push(name);
            }
        }
        rest = &rest[end + 1..];
    }
    check_references(rest)?;
    if open.is_empty() {
        Ok(())
    } else {
        Err(format!("{open:?} stay open"))
    }
}

/// Where a tag starting at the start of `xml` ends, outside of its quoted values.
fn tag_end(xml: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in xml.char_indices().skip(1) {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), '<') | (None, '<') => return None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

fn check_references(text: &str) -> Result<(), String> {
    for (at, _) in text.match_indices('&') {
        let rest = &text[at + 1..];
        let name = &rest[..rest.find(';').ok_or_else(|| format!("bare & in {text:?}"))?];
        let known = ["amp", "lt", "gt", "quot", "apos"].contains(&name) || name.starts_with('#');
        if !known {
            return Err(format!("unknown reference &{name}; in {text:?}"));
        }
    }
    Ok(())
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
        let l = layout(&text, &doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge {
            text: &text,
            doc: &doc,
            layout: &l,
            index: &index,
        };
        let shown = preview(&text);
        for _ in 0..8 {
            let p = rng.below(shown.len() - 1);
            let letter = char::from_u32(u32::from(shown[p])).filter(|c| c.is_ascii_alphabetic());
            let Some(letter) = letter else { continue };
            if p == 0 || !char::from_u32(u32::from(shown[p - 1])).is_some_and(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            let selected = bridge.source_selection(Shown(p)..Shown(p + 1));
            assert_eq!(&text[selected.clone()], letter.to_string(), "{p} of {text:?}");
            let at = bridge.source_at(Shown(p)).expect("a place");
            assert_eq!(at, selected.start, "{p} of {text:?}");
            assert_eq!(preview_offset(&l, &index, at), Shown(p), "{at} of {text:?}");
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
        let l = layout(&text, &doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge {
            text: &text,
            doc: &doc,
            layout: &l,
            index: &index,
        };
        let shown = preview(&text);
        let first = rng.below(shown.len());
        let last = (first + 1 + rng.below(6)).min(shown.len());
        let boundary = |p: usize| p >= shown.len() || !(0xDC00..0xE000).contains(&shown[p]);
        if !boundary(first) || !boundary(last) || shown[first..last].contains(&u16::from(b'\n')) {
            continue;
        }
        let edit = bridge
            .replace(Shown(first)..Shown(last), "q")
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

/// Parsing only around an edit must give exactly the tree a full parse gives, offsets included.
#[test]
fn reparsing_around_an_edit_matches_a_full_parse() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_1ac2_0005);
    let mut spliced = 0;
    for round in 0..rounds(3000) {
        // Several paragraphs, so most edits leave blocks on both sides untouched.
        // Real footnotes take a full parse, so most documents leave them out; the others hold a reference.
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
        let mut doc = parse(&old);
        let range = rng.range(&old);
        let chosen = PIECES[rng.below(PIECES.len())];
        let plain = chosen
            .replace("[^1]", "y")
            .replace("[^", "[")
            .replace("^[", "^")
            .replace("]:", "]");
        let piece = if with_footnotes { chosen } else { plain.as_str() };
        let mut new = old.clone();
        new.replace_range(range.clone(), if rng.below(4) == 0 { "" } else { piece });
        let old_spans = highlight(&old, &doc);
        let old_caret = (rng.below(2) == 0).then(|| rng.boundary(&old));
        let old_layout = layout(&old, &doc, &NoImages, old_caret);
        let parsed = reparse_and_color(&mut doc, &old, &old_spans, &new);
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
        let relaid = relayout(&new, &parsed.doc, &NoImages, new_caret, old_layout, &reuse);
        let fresh = layout(&new, &full.doc, &NoImages, new_caret);
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
        let Toggle::Pending(Some(pending)) = commands::toggle(&base, &parse(&base), at..at, style, None) else {
            continue;
        };
        let typed = *rng.pick(&["x", "neu", " ", " a", "ä😀"]);
        let mut text = base.clone();
        text.insert_str(pending.at, typed);
        let Some((edit, keep_pending)) = commands::typed_at_pending(&base, &text, &pending) else {
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
        let Toggle::Pending(Some(pending)) = commands::toggle(&text, &doc, at..at, *rng.pick(&PENDING), None) else {
            continue;
        };
        let l = layout(&text, &doc, &NoImages, Some(pending.at));
        let index = Index::new(&l);
        let bridge = Bridge {
            text: &text,
            doc: &doc,
            layout: &l,
            index: &index,
        };
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

/// The partial update of a 10 MB document after one keystroke, against a full one: same result, and within a time
/// limit. Run with `cargo test --release -- --ignored partial_updates`.
#[test]
#[ignore]
fn partial_updates_at_ten_megabytes() {
    let one = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/de.lproj/QuickReference.md"
    ))
    .unwrap()
    .replace("[^", "(")
    .replace("^[", "^(")
    .replace("]:", "] :");
    let mut old = String::new();
    while old.len() < 10 << 20 {
        old.push_str(&one);
        old.push_str("\n\n");
    }
    let at = old[old.len() / 2..].find("Fett").unwrap() + old.len() / 2;
    let mut new = old.clone();
    new.insert(at, 'x');
    let mut doc = parse(&old);
    let spans = highlight(&old, &doc);
    let old_layout = layout(&old, &doc, &NoImages, None);
    let started = std::time::Instant::now();
    let full = parse(&new);
    let full_spans = highlight(&new, &full);
    let full_layout = layout(&new, &full, &NoImages, None);
    let full_ms = started.elapsed().as_secs_f64() * 1e3;
    let started = std::time::Instant::now();
    let parsed = reparse_and_color(&mut doc, &old, &spans, &new);
    let splice = parsed.splice.as_ref().expect("a partial parse");
    let new_layout = relayout(
        &new,
        &parsed.doc,
        &NoImages,
        None,
        old_layout,
        &reusable(splice, &parsed.doc, None),
    );
    let partial_ms = started.elapsed().as_secs_f64() * 1e3;
    assert_eq!(parsed.doc.blocks, full.blocks);
    assert_eq!(parsed.spans, full_spans);
    assert_eq!(new_layout.blocks, full_layout.blocks);
    eprintln!("one keystroke at 10 MB: full {full_ms:.0} ms, partial {partial_ms:.0} ms");
    assert!(
        partial_ms < 150.0 && partial_ms * 4.0 < full_ms,
        "a keystroke got slower: partial {partial_ms:.0} ms, full {full_ms:.0} ms"
    );
}
