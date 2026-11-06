//! The preview of Splat as values: its layout, and the mapping between preview and source. It needs no window and no macOS.

#![forbid(unsafe_code)]
#![warn(missing_docs, unreachable_pub)]

splat_markdown::position!(
    /// A position in the text of the preview view, in UTF-16 units. It has nothing to do with a position in the source
    /// but what `preview_offset` and `insert_point` say.
    Shown
);

pub mod bridge;
pub mod layout;
pub mod map;
mod verify;
