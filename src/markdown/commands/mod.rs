use std::ops::Range;

use super::text::{Byte, offsets};

mod anchors;
mod inline;
mod insert;
mod lines;

pub use anchors::{HeadingAnchor, heading_anchors};
pub use inline::{
    Active, InlineStyle, Pending, Placed, Toggle, Typing, active_at, code_markers, leaf_at, move_typed, place,
    remove_formatting, styles_at, toggle, toggle_inline, typed_at_pending, typing_plan,
};
pub use insert::{
    code_block_language, insert_alert, insert_block, insert_code_block, insert_diagram, insert_footnote,
    insert_formula, insert_image, insert_link, insert_rule, insert_table, insert_toc, link_destination,
};
pub use lines::{LinePrefix, ListStyle, continuation_of, line_span, set_heading, toggle_list, toggle_quote};

/// A single replacement in the Markdown source; `selection` is expressed in the edited text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<Byte>,
    pub replacement: String,
    pub selection: Range<Byte>,
}

impl Edit {
    pub fn apply_to(&self, text: &mut String) {
        text.replace_range(offsets(&self.range), &self.replacement);
    }

    /// Shifts the unchanged selection by the length change of an edit made before it.
    fn keeping_selection_after(mut self, at: Byte, removed: usize, inserted: usize) -> Edit {
        let shift = |o: Byte| {
            if o <= at {
                o
            } else if o < at + removed {
                at + inserted
            } else {
                o + inserted - removed
            }
        };
        self.selection = shift(self.selection.start)..shift(self.selection.end);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::doc::Doc;
    use crate::markdown::reader::parse;
    use crate::markdown::text::{TextAt, bytes};

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
