//! HTML as the apps paste it, read into the Markdown that Splat writes.

use splat_markdown::reader::parse;
use splat_testkit::{Rng, rounds};

use super::*;
use crate::rich::{Block, ListKind, Marks, Piece, Source, Span};

mod apps;
mod blocks;
mod pictures;
mod random;
mod sheet;
mod tables;
mod text;

fn md(html: &str) -> String {
    from_html(html).into_markdown()
}

fn same(cases: &[(&str, &str)]) {
    for (html, wanted) in cases {
        assert_eq!(md(html), *wanted, "{html}");
    }
}
