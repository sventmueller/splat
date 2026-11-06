//! The folder of notes around a document: its tree, searching it, where links lead and what renaming a note changes. It
//! needs no window and no macOS. What takes a `stop` flag reads the disk and blocks, so it belongs on a thread of its
//! own, as a volume can stop answering.

#![forbid(unsafe_code)]
#![warn(missing_docs, unreachable_pub)]

mod links;
mod rename;
mod resolve;
mod search;
mod tree;

pub use rename::{Replacement, Rewrite, applied, renames};
pub use search::{FileHits, Hit, MOST_HITS_PER_FILE, Query, Searched, hits_in_text, search};
pub use tree::{Node, Tree, is_note, root_for, scan, scan_flat};
