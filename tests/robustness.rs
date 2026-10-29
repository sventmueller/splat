//! Randomised documents and edits: every command must return an edit inside the text and on char boundaries.

use std::ops::Range;

use splat::fuzz::{Rng, rounds};
use splat::markdown::commands::{self, Edit, InlineStyle, ListStyle, Pending, Toggle};
use splat::markdown::doc::{AlertKind, parse};
use splat::markdown::highlight::highlight;
use splat::markdown::incremental::{parse_and_color, reparse_and_color};
use splat::markdown::table::{TableOp, table_edit};
use splat::preview::bridge::Bridge;
use splat::preview::layout::{NoImages, layout, relayout, reusable};
use splat::preview::map::{Index, preview_offset};

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
    let mut rng = Rng::new(0x5eed_1234_abcd_0001);
    for _ in 0..rounds(400) {
        let text = document(&mut rng);
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
    let mut rng = Rng::new(0x0dd_ba11_c0ff_ee42 ^ 0x9e37_79b9);
    for _ in 0..rounds(300) {
        let text = document(&mut rng);
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
        if let Some(e) = bridge.replace(p..q, typed) {
            check(&text, &e, "replace");
        }
        if let Some(e) = bridge.newline(p) {
            check(&text, &e, "newline");
        }
        if let Some(e) = bridge.delete_at_block_start(p) {
            check(&text, &e, "backspace");
        }
        if let Some(e) = bridge.tab(p, rng.below(2) == 0) {
            check(&text, &e, "tab");
        }
        let s = bridge.source_selection(p..q);
        assert!(s.end <= text.len() && text.is_char_boundary(s.start) && text.is_char_boundary(s.end));
        let back = preview_offset(&l, &index, rng.boundary(&text));
        assert!(back <= len, "{back} > {len}");
    }
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
        let level = 1 + rng.below(6) as u8;
        let heading = commands::set_heading(&text, sel.clone(), level);
        let titled = applied(&text, &heading);
        let body = commands::set_heading(&titled, heading.selection.clone(), 0);
        assert_eq!(applied(&titled, &body), text, "heading {level} via {titled:?}");
    }
}

#[test]
fn inserting_and_deleting_rows_and_columns_gives_back_the_table() {
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

/// Typing a character in the preview must show exactly that character there once the source is laid out again.
#[test]
fn typing_in_the_preview_shows_what_was_typed() {
    let mut rng = Rng::new(0x5eed_7e57_0004);
    let mut checked = 0;
    let preview = |text: &str| -> Vec<u16> {
        let l = layout(text, &parse(text), &NoImages, None);
        l.blocks
            .iter()
            .flat_map(|b| &b.paras)
            .flat_map(|p| p.runs.iter().map(|r| r.text.as_str()).chain(["\n"]))
            .flat_map(|s| s.encode_utf16().collect::<Vec<_>>())
            .collect()
    };
    for round in 0..rounds(400) {
        let mut text = plain_document(&mut rng);
        if round % 2 == 1 {
            const MARKED: [&str; 6] = [
                " **fett**",
                " *kursiv*",
                " `code`",
                " [Link](#a)",
                " ~~weg~~",
                " <mark>hell</mark>",
            ];
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
        let typed = ["a", "ä", "*", "#", "_"][rng.below(5)];
        let edit = bridge.replace(p..p, typed).expect("typing in a paragraph");
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

/// Parsing only around an edit must give exactly the tree a full parse gives, offsets included.
#[test]
fn reparsing_around_an_edit_matches_a_full_parse() {
    let mut rng = Rng::new(0x5eed_1ac2_0005);
    let mut spliced = 0;
    for _ in 0..rounds(3000) {
        // Several paragraphs, so most edits leave blocks on both sides untouched.
        // Footnotes always take a full parse, so they are left out; every other document holds a reference.
        let mut old: String = (0..rng.below(6) + 2)
            .map(|_| document(&mut rng).replace("[^1]", "x").replace("^[", "^"))
            .collect::<Vec<_>>()
            .join("\n\n");
        if rng.below(2) == 0 {
            old.push_str("\n\n[ref]: https://example.com\n\nSiehe [ref] und [Text][ref].\n");
        }
        let mut doc = parse(&old);
        let range = rng.range(&old);
        let piece = PIECES[rng.below(PIECES.len())]
            .replace("[^1]", "y")
            .replace("[^", "[")
            .replace("^[", "^");
        let piece = piece.as_str();
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
    assert!(spliced > rounds(3000) / 4, "only {spliced} edits were parsed in part");
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

/// The partial update of a 10 MB document after one keystroke, against a full one: same result, and the time of
/// both. Run with `cargo test --release -- --ignored partial_updates`.
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
}
