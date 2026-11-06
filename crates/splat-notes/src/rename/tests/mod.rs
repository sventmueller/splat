//! What a rename changes in the notes, by topic.

use std::collections::HashMap;

use super::*;
use crate::search::tests::Folder;
use crate::tree::{Node, scan};

mod calls;
mod links;
mod names;
mod other_notes;
mod random;

/// A tree that exists only in memory.
fn tree_of(notes: &[&str]) -> Tree {
    let mut tree = Tree {
        root: PathBuf::from("/v"),
        ..Tree::default()
    };
    for note in notes {
        let mut owner: Option<usize> = None;
        let mut path = PathBuf::from("/v");
        let parts: Vec<&str> = note.split('/').collect();
        for (at, part) in parts.iter().enumerate() {
            path.push(part);
            let list = owner.map_or(&tree.top, |owner| &tree.nodes[owner].children);
            let known = list.iter().copied().find(|&child| tree.nodes[child].name == *part);
            let index = known.unwrap_or_else(|| {
                let index = tree.nodes.len();
                tree.nodes.push(Node {
                    name: (*part).to_string(),
                    path: path.clone(),
                    folder: at + 1 < parts.len(),
                    children: Vec::new(),
                });
                match owner {
                    Some(owner) => tree.nodes[owner].children.push(index),
                    None => tree.top.push(index),
                }
                index
            });
            owner = Some(index);
        }
    }
    tree
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
