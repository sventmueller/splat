//! What a rename changes in the notes, by topic.

use std::collections::HashMap;

use super::*;
use crate::search::tests::Folder;
use crate::tree::scan;

mod calls;
mod links;
mod names;
mod other_notes;
mod random;

fn tree_of(notes: &[&str]) -> Tree {
    Tree::of_notes(Path::new("/v"), notes)
}

fn renamed_in(tree: &Tree, file: &str, text: &str, from: &str, to: &str) -> String {
    let (from, to) = (Path::new(from), Path::new(to));
    let wanted = Path::new(file);
    let rewrites = renames(tree, from, to, &AtomicBool::new(false), &mut |path| {
        (path == wanted).then(|| text.to_string())
    });
    match rewrites.as_slice() {
        [] => text.to_string(),
        [rewrite] => applied(text, &rewrite.replacements),
        more => panic!("{more:?}"),
    }
}

/// `text` in a note at the top of a tree when `Old.md` becomes `New.md`.
fn after(text: &str) -> String {
    renamed_in(
        &tree_of(&["from.md", "Old.md", "sub/Sub.md"]),
        "/v/from.md",
        text,
        "/v/Old.md",
        "/v/New.md",
    )
}
