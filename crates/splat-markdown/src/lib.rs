//! The Markdown model of Splat: the tree every consumer reads, the only binding to the parser, the commands as pure text
//! edits, the partial parse after an edit and the exports. It needs no window and no macOS.

#![forbid(unsafe_code)]
#![warn(missing_docs, unreachable_pub)]

pub mod code;
pub mod commands;
pub mod counts;
pub mod doc;
pub mod docx;
pub mod edit;
pub mod front_matter;
pub mod highlight;
pub mod html;
pub mod incremental;
pub mod markup;
pub mod palette;
pub mod reader;
pub mod remote;
pub mod snippets;
pub mod style;
pub mod table;
pub mod text;
pub mod typography;
pub mod url;
