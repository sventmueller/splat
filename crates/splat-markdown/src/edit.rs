use std::ops::Range;

use super::text::{Byte, TextAt, offsets};

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

    pub fn applied_to(&self, text: &str) -> String {
        format!(
            "{}{}{}",
            text.upto(self.range.start),
            self.replacement,
            text.from(self.range.end)
        )
    }

    /// The caret `n` bytes in front of the end of the selection.
    pub fn with_caret_before(mut self, n: usize) -> Edit {
        let caret = self.selection.end - n;
        self.selection = caret..caret;
        self
    }

    /// Shifts the unchanged selection by the length change of an edit made before it.
    pub(super) fn keeping_selection_after(mut self, at: Byte, removed: usize, inserted: usize) -> Edit {
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
