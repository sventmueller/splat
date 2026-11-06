//! What the editor window knows and decides that needs no window: the state derived from the source text and how it
//! changes, the copy of the text, the queue of what WebKit renders, what a click on a link means, and the files beside a
//! document. It needs no macOS; the app in `src/editor/` asks it and does what it answers to the views.

#![forbid(unsafe_code)]
#![warn(missing_docs, unreachable_pub)]

use splat_i18n as i18n;
use splat_markdown as markdown;
use splat_preview as preview;

pub mod bibtex;
pub mod changes;
pub mod editing;
pub mod files;
pub mod lanes;
pub mod links;
pub mod metadata;
pub mod model;
pub mod navigation;
pub mod notes;
pub mod outline;
pub mod progress;
pub mod render_queue;
pub mod repository;
pub mod state;
pub mod sync;
pub mod word_count;
