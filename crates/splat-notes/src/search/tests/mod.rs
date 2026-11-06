//! The search of the notes, by topic; `Folder` serves the tests of the other modules as well.

use std::sync::atomic::AtomicUsize;

use super::*;
use crate::tree::scan;

mod files;
mod places;
mod tags_and_links;
mod text;

/// A folder for one test, gone when the test ends.
pub(crate) struct Folder(PathBuf);

impl Folder {
    pub(crate) fn new(name: &str) -> Folder {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("splat-notes-{}-{unique}-{name}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Folder(path.canonicalize().unwrap())
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    pub(crate) fn note(&self, name: &str, text: &str) {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tree() -> Tree {
    Tree::default()
}

/// The source of the matches of `needle` in `text`.
fn matches(text: &str, needle: &str, case_sensitive: bool) -> Vec<String> {
    let query = Query::Text {
        needle: needle.to_string(),
        case_sensitive,
    };
    let found = hits_in_text(text, &query, Path::new("/n.md"), &tree());
    found
        .hits
        .iter()
        .map(|hit| text[hit.range.clone()].to_string())
        .collect()
}

fn folded(text: &str, needle: &str) -> Vec<String> {
    matches(text, needle, false)
}
