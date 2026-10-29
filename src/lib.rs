//! The part of Splat that needs no window: the Markdown model and its commands, the preview layout and the interface
//! texts. The app in `main.rs` builds on it, and the tests in `tests/` reach it from outside.

pub mod fuzz;
pub mod i18n;
pub mod markdown;
pub mod preview;
