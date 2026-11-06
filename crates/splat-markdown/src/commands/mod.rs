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
pub use insert::{
    code_block_language, insert_alert, insert_block, insert_code_block, insert_diagram, insert_footnote,
    insert_formula, insert_image, insert_link, insert_rule, insert_table, insert_toc, link_destination,
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
}
