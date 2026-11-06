//! Tests of the bridge by topic, and the helpers they share.

use super::escape::*;
use super::keys::*;
use super::*;
use crate::layout::RunMap;
use crate::layout::{NoImages, layout};
use splat_markdown::commands::Pending;
use splat_markdown::reader::parse;
use splat_markdown::text::{Direction, bytes};

/// The document of a text, its layout, its index and the bridge between them.
macro_rules! bridge_of {
    ($text:expr => $doc:ident, $layout:ident, $index:ident, $bridge:ident) => {
        let $doc = splat_markdown::reader::parse($text);
        let $layout = crate::layout::layout(&$doc, &crate::layout::NoImages, None);
        let $index = Index::new(&$layout);
        let $bridge = Bridge::new(&$doc, &$layout, &$index).unwrap();
    };
}

mod clicks;
mod code;
mod deleting;
mod returns;
mod selection;
mod tab_and_backspace;
mod typing;

/// The source after `edit` of the preview of `t`.
fn edited(t: &str, edit: impl Fn(&Bridge) -> Option<Edit>) -> String {
    bridge_of!(t => d, l, index, bridge);
    let mut out = t.to_string();
    edit(&bridge).expect("an edit").apply_to(&mut out);
    out
}

/// What the preview shows of `t`, its paragraphs in lines.
fn shown_of(t: &str) -> String {
    let d = splat_markdown::reader::parse(t);
    let l = crate::layout::layout(&d, &crate::layout::NoImages, None);
    l.blocks
        .iter()
        .flat_map(|b| &b.paras)
        .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn edit(t: &str, f: impl Fn(&Bridge) -> Option<Edit>) -> (String, usize) {
    let d = parse(t);
    let l = layout(&d, &NoImages, None);
    let i = Index::new(&l);
    let b = Bridge::new(&d, &l, &i).unwrap();
    let e = f(&b).expect("edit");
    let mut s = t.to_string();
    e.apply_to(&mut s);
    (s, e.selection.start.get())
}

fn edit_of(t: &str, edit: impl Fn(&Bridge) -> Option<Edit>) -> Option<Edit> {
    let d = splat_markdown::reader::parse(t);
    let l = crate::layout::layout(&d, &crate::layout::NoImages, None);
    let index = Index::new(&l);
    edit(&Bridge::new(&d, &l, &index).unwrap())
}

/// The source after `edit` of the preview of `t`, with `|` for the caret.
fn with_caret(t: &str, edit: impl Fn(&Bridge) -> Option<Edit>) -> String {
    let edit = edit_of(t, edit).expect("an edit");
    let mut out = t.to_string();
    edit.apply_to(&mut out);
    out.insert(edit.selection.start.get(), '|');
    out
}

/// A position behind the end of the preview, inside a character of it, or a pending caret from an older text, gives no
/// edit or one that fits the source, and never a panic.
#[test]
fn a_stale_position_gives_no_edit_or_one_that_fits() {
    use splat_markdown::commands::InlineStyle;
    let t = "Füße 😀 **ä**\n\n- [ ] ü\n\n> b\n\n| ü | b |\n|---|---|\n| c | d |\n";
    bridge_of!(t => d, l, index, b);
    let fits = |edit: Option<Edit>, what: &str| {
        if let Some(edit) = edit {
            assert!(
                edit.range.end <= t.end() && t.boundary(edit.range.start) && t.boundary(edit.range.end),
                "{what}"
            );
        }
    };
    let inside_the_emoji = Shown::new("Füße ".chars().count() + 1);
    for p in [inside_the_emoji, Shown::new(t.len() * 2), Shown::new(usize::MAX / 2)] {
        let range = Shown::new(1)..p;
        let _ = (
            b.source_selection(range.clone()),
            b.source_selection(p..p),
            b.source_at(p),
            b.escaped(p, "*", false),
        );
        fits(b.replace(range.clone(), "x"), "replace");
        fits(b.replace(p..p, "x"), "replace");
        fits(
            b.replace_keeping_styles(range, "x").map(|(e, _)| e),
            "replace_keeping_styles",
        );
        fits(b.newline(p), "newline");
        fits(b.line_break(p), "line_break");
        fits(b.tab(p, Direction::Forward), "tab");
        fits(b.tab(p, Direction::Backward), "tab");
        fits(b.delete_at_block_start(p), "delete_at_block_start");
        fits(b.toggle_fold(p), "toggle_fold");
        fits(b.toggle_task(p), "toggle_task");
        for at in [2, 999] {
            let pending = Pending {
                at: Byte::new(at),
                styles: vec![InlineStyle::Bold],
            };
            fits(
                b.type_with_pending(&pending, p, "x").map(|(e, _)| e),
                "type_with_pending",
            );
        }
    }
}
