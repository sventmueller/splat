//! What each target in `fuzz/` asks of its input. They live here so that `cargo test` can run the seeds in `fuzz/seeds`
//! through the same checks on every run, where the fuzzers need a nightly compiler and minutes.

use splat_file_state::{Encoding, FileFormat};
use splat_markdown::commands::{self, Edit, InlineStyle, ListStyle};
use splat_markdown::doc::Align;
use splat_markdown::docx::{WordExport, stored_entries, to_docx};
use splat_markdown::html::{Export, to_html};
use splat_markdown::incremental::{parse_and_color, reparse_and_color};
use splat_markdown::reader::parse;
use splat_markdown::table::{TableOp, table_edit};
use splat_markdown::text::{Byte, Direction, bytes};
use splat_preview::bridge::Bridge;
use splat_preview::layout::{NoImages, layout};
use splat_preview::map::{Index, Shown};

/// Ends the text and each step of an input: no byte of UTF-8.
const SEPARATOR: u8 = 0xFF;

/// Long enough for what crosses blocks, short enough for many rounds per second.
const MOST_STEPS: usize = 16;

/// The largest position of `text` not beyond `at` on a char boundary.
fn on_a_letter(text: &str, at: usize) -> usize {
    text.floor_char_boundary(at.min(text.len()))
}

/// The text of an input and the steps behind it.
fn text_and_steps(data: &[u8]) -> Option<(String, impl Iterator<Item = &[u8]>)> {
    let mut parts = data.split(|&b| b == SEPARATOR);
    let text = std::str::from_utf8(parts.next()?).ok()?.to_string();
    Some((text, parts.take(MOST_STEPS)))
}

/// A step is two bytes for its place, one for how much it replaces, and the text it leaves there.
fn typed(text: &str, step: &[u8]) -> Option<String> {
    let [high, low, replaced, typed @ ..] = step else {
        return None;
    };
    let start = on_a_letter(text, usize::from(*high) << 8 | usize::from(*low));
    let end = on_a_letter(text, start + usize::from(*replaced));
    let mut new = text.to_string();
    new.replace_range(start..end, std::str::from_utf8(typed).ok()?);
    Some(new)
}

/// The text after `edit`, which has to lie inside `text` and on its letters, with a selection that lies inside the result.
pub fn edit_fits(text: &str, edit: &Edit, what: &str) -> String {
    assert!(
        edit.range.start <= edit.range.end && edit.range.end <= Byte::new(text.len()),
        "{what}: {edit:?} in {text:?}"
    );
    assert!(
        text.is_char_boundary(edit.range.start.get()) && text.is_char_boundary(edit.range.end.get()),
        "{what}: {edit:?} in {text:?}"
    );
    let mut out = text.to_string();
    edit.apply_to(&mut out);
    assert!(
        edit.selection.start <= edit.selection.end && edit.selection.end <= Byte::new(out.len()),
        "{what}: {edit:?} -> {out:?}"
    );
    assert!(
        out.is_char_boundary(edit.selection.start.get()) && out.is_char_boundary(edit.selection.end.get()),
        "{what}: {edit:?} -> {out:?}"
    );
    out
}

/// What is wrong with a part of a Word file, if anything: it has to be XML that a word processor can read.
pub fn xml_problem(xml: &[u8]) -> Option<String> {
    let text = match std::str::from_utf8(xml) {
        Ok(text) => text,
        Err(error) => return Some(error.to_string()),
    };
    roxmltree::Document::parse(text).err().map(|error| error.to_string())
}

/// After each keystroke on a text, blocks, colors, what the document knows about its text and the layout are what a full
/// parse finds.
pub fn reparse(data: &[u8]) {
    let Some((mut text, steps)) = text_and_steps(data) else {
        return;
    };
    let mut parsed = parse_and_color(&text);
    for step in steps {
        let Some(new) = typed(&text, step) else {
            continue;
        };
        let spans = std::mem::take(&mut parsed.spans);
        let next = reparse_and_color(parsed.doc, &spans, &new);
        let full = parse_and_color(&new);
        let what = || format!("{text:?} -> {new:?}");
        assert!(next.doc.blocks == full.doc.blocks, "blocks: {}", what());
        assert!(next.spans == full.spans, "colors: {}", what());
        assert!(next.doc.facts() == full.doc.facts(), "facts: {}", what());
        assert!(
            layout(&next.doc, &NoImages, None).blocks == layout(&full.doc, &NoImages, None).blocks,
            "layout: {}",
            what()
        );
        parsed = next;
        text = new;
    }
}

/// Any text makes a preview, a page without scripts and a Word file whose every part reads as XML.
pub fn outputs(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let doc = parse(text);
    let shown = layout(&doc, &NoImages, None);
    let _ = Index::new(&shown);
    let page = to_html(
        text,
        &Export {
            title: "",
            figure: &|_| None,
            embed: &|_| None,
            rebase: &|url| url.to_string(),
            quotes: None,
            reference: &|_| None,
            citation: &|_| None,
        },
    );
    assert!(!page.to_ascii_lowercase().contains("<script"), "{text:?}: {page}");
    let word = to_docx(
        &doc,
        &WordExport {
            figure: &|_| None,
            image: &|_| None,
            quotes: None,
            reference: &|_| None,
            citation: &|_| None,
        },
    );
    if let Ok(word) = word {
        for (name, part) in stored_entries(&word) {
            if name.ends_with(".xml") || name.ends_with(".rels") {
                assert_eq!(xml_problem(&part), None, "{name} of {text:?}");
            }
        }
    }
}

/// What a step does in the preview: its first bytes say what and where, the rest what is typed. Every edit that comes of it
/// fits the text, and what it leaves goes on to the next step.
pub fn bridge(data: &[u8]) {
    let Some((mut text, steps)) = text_and_steps(data) else {
        return;
    };
    for step in steps {
        let [kind, high, low, length, typed @ ..] = step else {
            continue;
        };
        let Ok(typed) = std::str::from_utf8(typed) else {
            continue;
        };
        let doc = parse(&text);
        let shown = layout(&doc, &NoImages, None);
        let index = Index::new(&shown);
        let bridge = Bridge::new(&doc, &shown, &index).expect("a layout of this document");
        let at = usize::from(*high) << 8 | usize::from(*low);
        let p = Shown::new(at);
        let edit = match kind % 8 {
            0 => bridge.replace(p..Shown::new(at + usize::from(*length)), typed),
            1 => bridge
                .replace_keeping_styles(p..Shown::new(at + usize::from(*length)), typed)
                .map(|(edit, _)| edit),
            2 => bridge.newline(p),
            3 => bridge.line_break(p),
            4 => bridge.tab(
                p,
                if length % 2 == 0 {
                    Direction::Forward
                } else {
                    Direction::Backward
                },
            ),
            5 => bridge.delete_at_block_start(p),
            6 => bridge.toggle_fold(p),
            _ => bridge.toggle_task(p),
        };
        if let Some(edit) = edit {
            text = edit_fits(&text, &edit, "preview");
        }
    }
}

/// Every command on any selection gives an edit that fits the text, and what it makes of the text can be read again.
pub fn commands(data: &[u8]) {
    let Some((text, steps)) = text_and_steps(data) else {
        return;
    };
    let doc = parse(&text);
    for step in steps {
        let [a, b, c, d, ..] = step else {
            continue;
        };
        let start = on_a_letter(&text, usize::from(*a) << 8 | usize::from(*b));
        let end = on_a_letter(&text, start + (usize::from(*c) << 8 | usize::from(*d)));
        let sel = bytes(start..end);
        let edits = [
            commands::toggle_inline(&doc, sel.clone(), InlineStyle::Bold),
            commands::toggle_inline(&doc, sel.clone(), InlineStyle::Italic),
            commands::remove_formatting(&doc, sel.clone()),
            commands::set_heading(&doc, sel.clone(), 2),
            commands::set_heading(&doc, sel.clone(), 0),
            commands::toggle_list(&text, sel.clone(), ListStyle::Bullet),
            commands::toggle_list(&text, sel.clone(), ListStyle::Task),
            commands::toggle_quote(&text, sel.clone()),
        ];
        for edit in &edits {
            let _ = parse(&edit_fits(&text, edit, "command"));
        }
        for op in [
            TableOp::InsertRowBelow,
            TableOp::InsertColumnRight,
            TableOp::DeleteRow,
            TableOp::DeleteColumn,
            TableOp::Align(Align::Center),
            TableOp::Format,
        ] {
            if let Some(edit) = table_edit(&doc, sel.start, op) {
                let _ = parse(&edit_fits(&text, &edit, "table"));
            }
        }
    }
}

/// A file that opens and saves unchanged stays what it was, byte for byte.
pub fn encoding(data: &[u8]) {
    let Some((encoding, text)) = Encoding::read(data.to_vec()) else {
        return;
    };
    let (format, body) = FileFormat::of(&text);
    let written = FileFormat { encoding, ..format }.encode(body);
    assert_eq!(written, data, "{encoding:?} {format:?}");
}

/// Runs a check on every seed of `folder`, naming the file it fails on.
pub fn replay(folder: &std::path::Path, check: fn(&[u8])) -> usize {
    let mut seeds = 0;
    for entry in std::fs::read_dir(folder).expect("a folder of seeds").flatten() {
        let path = entry.path();
        let bytes = std::fs::read(&path).expect("a seed");
        let _name = crate::Reporting(path.to_str().unwrap_or_default());
        check(&bytes);
        seeds += 1;
    }
    seeds
}
