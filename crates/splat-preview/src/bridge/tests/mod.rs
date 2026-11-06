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
