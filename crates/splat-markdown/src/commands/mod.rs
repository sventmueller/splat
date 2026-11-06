//! The editing commands as pure text edits on the Markdown source: formatting, list, quote and heading lines, blocks to
//! insert, and the anchors of headings.

mod anchors;
mod inline;
mod insert;
mod lines;

pub use super::edit::Edit;
pub use anchors::{HeadingAnchor, heading_anchors};
pub use inline::{
    Active, InlineStyle, Pending, Placed, Toggle, Typing, active_at, code_markers, leaf_at, move_typed, place,
    remove_formatting, styles_at, toggle, toggle_inline, typed_at_pending, typing_plan,
};
#[cfg(test)]
use insert::insert_code_block;
pub use insert::{
    code_block_language, insert_alert, insert_block, insert_diagram, insert_footnote, insert_formula, insert_image,
    insert_link, insert_rule, insert_table, insert_toc, link_destination,
};
pub use lines::{LinePrefix, ListStyle, line_span, set_heading, toggle_list, toggle_quote};

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::*;
    use crate::doc::Doc;
    use crate::reader::parse;
    use crate::text::{Byte, TextAt, bytes};

    pub(super) fn run(text: &str, sel: Range<usize>, f: impl Fn(&Doc, Range<Byte>) -> Edit) -> (String, String) {
        let d = parse(text);
        let e = f(&d, bytes(sel));
        let mut s = text.to_string();
        e.apply_to(&mut s);
        let selected = s.at(e.selection.clone()).to_string();
        (s, selected)
    }

    pub(super) fn applied(t: &str, e: &Edit) -> String {
        let mut out = t.to_string();
        e.apply_to(&mut out);
        out
    }

    /// What every command that takes a selection or an offset makes of `sel` in `doc`, by name.
    fn edits_of_every_command(doc: &Doc, sel: &Range<Byte>) -> Vec<(&'static str, Option<Edit>)> {
        use crate::doc::AlertKind;
        use crate::front_matter::{self, Value};
        use crate::table::{TableOp, move_down, navigate, table_edit};
        use crate::text::Direction;
        let (text, at) = (doc.text(), sel.start);
        let placed = Placed {
            at: sel.end,
            text: "x".into(),
            caret: 1,
            keep_pending: false,
        };
        let toggled = match toggle(doc, sel.clone(), InlineStyle::Italic, None) {
            Toggle::Edit(edit) => Some(edit),
            Toggle::Pending(_) => None,
        };
        vec![
            (
                "toggle_inline",
                Some(toggle_inline(doc, sel.clone(), InlineStyle::Bold)),
            ),
            ("toggle", toggled),
            ("remove_formatting", Some(remove_formatting(doc, sel.clone()))),
            ("set_heading", Some(set_heading(doc, sel.clone(), 2))),
            ("toggle_list", Some(toggle_list(text, sel.clone(), ListStyle::Task))),
            ("toggle_quote", Some(toggle_quote(text, sel.clone()))),
            ("insert_block", Some(insert_block(text, sel.clone(), "x", 1))),
            ("insert_code_block", Some(insert_code_block(text, sel.clone(), "rust"))),
            (
                "code_block_language",
                Some(code_block_language(doc, sel.clone(), "rust")),
            ),
            ("insert_alert", Some(insert_alert(doc, sel.clone(), AlertKind::Tip))),
            ("insert_toc", insert_toc(doc, sel.clone())),
            ("insert_rule", Some(insert_rule(text, sel.clone()))),
            ("insert_table", Some(insert_table(text, sel.clone(), 2, 2))),
            ("insert_link", Some(insert_link(text, sel.clone(), "u", "l"))),
            ("insert_image", Some(insert_image(text, sel.clone(), "a", "p.png"))),
            ("insert_formula", Some(insert_formula(text, sel.clone()))),
            ("insert_diagram", Some(insert_diagram(text, sel.clone(), "graph TD"))),
            ("insert_footnote", Some(insert_footnote(text, sel.clone()))),
            ("table_edit", table_edit(doc, at, TableOp::InsertColumnRight)),
            ("navigate", navigate(doc, at, Direction::Forward)),
            ("move_down", move_down(doc, at)),
            (
                "front_matter",
                front_matter::edit(doc, &[("a".into(), Value::Text("b".into()))], sel),
            ),
            ("move_typed", Some(move_typed(text, at, 3, &placed))),
        ]
    }

    /// Each command and query with a selection or offset that may be stale: beyond the text, inside a character or
    /// turned round. None may panic, and every edit must fit the text it is for.
    #[test]
    fn a_stale_selection_is_brought_into_the_text_by_every_command_and_query() {
        use crate::snippets::{DiagramAction, diagram_snippet};
        let text = "Füße **ä** `ö` $x$\n\n| ü | b |\n|---|---|\n| c | d |\n";
        let doc = parse(text);
        let len = text.len();
        let middle = text.find('ü').unwrap() + 1;
        let fits = |edit: &Edit, what: &str| {
            assert!(
                edit.range.end <= text.end() && text.boundary(edit.range.start) && text.boundary(edit.range.end),
                "{what}: {edit:?}"
            );
            let new = edit.applied_to(text);
            assert!(
                edit.selection.end <= new.end()
                    && new.boundary(edit.selection.start)
                    && new.boundary(edit.selection.end),
                "{what}: {edit:?}"
            );
        };
        for sel in [
            bytes(2..2),
            bytes(middle..middle),
            bytes(2..middle),
            bytes(middle..len + 9),
            bytes(len + 9..len + 9),
            bytes(len + 3..1),
        ] {
            for (what, edit) in edits_of_every_command(&doc, &sel) {
                if let Some(edit) = edit {
                    fits(&edit, &format!("{what} at {sel:?}"));
                }
            }
            let at = sel.start;
            let span = line_span(text, &sel);
            assert!(span.end <= text.end() && text.boundary(span.start), "{sel:?}");
            let _ = (leaf_at(&doc, at), active_at(&doc, at), styles_at(&doc, at));
            let _ = (
                crate::counts::of_range(&doc, sel.clone()),
                crate::doc::figure_at(&doc, at),
            );
            let pending = Pending {
                at,
                styles: vec![InlineStyle::Bold],
            };
            assert!(typing_plan(&doc, &pending).is_none() || doc.fit_at(at) == at);
            let typed = format!("{text}x");
            assert!(typed_at_pending(&doc, &typed, &pending).is_none() || doc.fit_at(at) == at);
        }
        let code = "graph TD\n  Ä --> B\n";
        let inside_the_letter = code.find('Ä').unwrap() + 1;
        for caret in [inside_the_letter, code.len() + 9] {
            let _ = diagram_snippet(code, caret, DiagramAction::Edge, "2026-11-06");
        }
    }

    #[test]
    #[expect(clippy::reversed_empty_ranges, reason = "a reversed selection is one of the inputs")]
    fn a_selection_is_fitted_to_the_characters_of_the_text() {
        let doc = parse("aä😀\n");
        for (sel, wanted) in [(0..1, 0..1), (2..3, 1..3), (4..9, 3..8), (99..99, 8..8), (5..2, 1..1)] {
            assert_eq!(doc.fit(bytes(sel.clone())), bytes(wanted), "{sel:?}");
        }
        assert_eq!(doc.fit_at(Byte::new(6)), Byte::new(3));
    }
}
