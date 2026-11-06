//! Commands on the source: every edit fits the text, and formatting twice gives it back.

use std::ops::Range;

use splat_markdown::commands::{self, Edit, InlineStyle, ListStyle, Pending, Toggle};
use splat_markdown::doc::{AlertKind, Doc};
use splat_markdown::highlight::highlight;
use splat_markdown::incremental::Parsed;
use splat_markdown::reader::parse;
use splat_markdown::table::{TableOp, table_edit};
use splat_markdown::text::{Byte, TextAt, bytes};
use splat_preview::bridge::Bridge;
use splat_preview::layout::{NoImages, layout};
use splat_preview::map::{Index, Shown};
use splat_testkit::{Rng, rounds, with_smallest_failure};

use super::common::{check, document, limited, plain_document};

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
