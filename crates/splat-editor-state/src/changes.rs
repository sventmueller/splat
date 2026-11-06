//! What differs from the last Git commit, and where the marks for it stand in the two views. The repository is read by
//! `splat-git`, never by the `git` program, whose configuration in a foreign repository can start other programs.

use std::path::{Path, PathBuf};

use splat_file_state::{Encoding, FileFormat};
use splat_git::Oid;
use splat_git::diff::{Change, ChangeKind};

use crate::markdown::text::{Byte, Utf16Cursor};
use crate::preview::map::preview_offset;
use crate::state::State;

/// Where a change stands in a view's text, in UTF-16 units.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mark {
    /// Whether lines were added, changed or removed.
    pub kind: ChangeKind,
    /// Where the mark starts.
    pub start: usize,
    /// Where it ends; a removal ends where it starts.
    pub end: usize,
}

/// What a look at the repository found.
pub enum BaseLook {
    /// HEAD stands where it stood.
    Unchanged,
    /// No repository, or the file is not in the last commit, or it could not be read.
    Nothing,
    /// What the file holds in the commit HEAD stands on now.
    New {
        /// The file.
        path: PathBuf,
        /// The commit.
        commit: Oid,
        /// Its text there, as the source view holds a text.
        text: String,
    },
}

/// Reads what the file holds in the last commit, unless HEAD still stands on `known`.
pub fn read_base(file: &Path, known: Option<Oid>) -> BaseLook {
    let Some(repository) = splat_git::find(file) else {
        return BaseLook::Nothing;
    };
    let Ok(Some(commit)) = repository.head_commit() else {
        return BaseLook::Nothing;
    };
    if known == Some(commit) {
        return BaseLook::Unchanged;
    }
    let Ok(Some(bytes)) = repository.head_version(file) else {
        return BaseLook::Nothing;
    };
    let Some((_, decoded)) = Encoding::read(bytes) else {
        return BaseLook::Nothing;
    };
    let (_, text) = FileFormat::of(&decoded);
    BaseLook::New {
        path: file.to_path_buf(),
        commit,
        text: text.to_string(),
    }
}

/// Where `changes` of `text` stand in the source view, and in the preview of `st`, which was made of that text.
pub fn marks(changes: &[Change], text: &str, st: &State) -> (Vec<Mark>, Vec<Mark>) {
    let mut cursor = Utf16Cursor::new(text);
    let source = changes
        .iter()
        .map(|change| Mark {
            kind: change.kind,
            start: cursor.at(Byte::new(change.bytes.start)).get(),
            end: cursor.at(Byte::new(change.bytes.end)).get(),
        })
        .collect();
    let preview = changes
        .iter()
        .map(|change| {
            let start = preview_offset(st.layout(), st.index(), Byte::new(change.bytes.start)).get();
            // The end of a change is the start of the next line, which in the preview is another paragraph.
            let last = text.floor_char_boundary(change.bytes.end.saturating_sub(1));
            let end = if change.bytes.is_empty() {
                start
            } else {
                preview_offset(st.layout(), st.index(), Byte::new(last))
                    .get()
                    .max(start)
                    + 1
            };
            Mark {
                kind: change.kind,
                start,
                end,
            }
        })
        .collect();
    (source, preview)
}

/// The marks that reach into the lines from `top` to `bottom`, in the order of the text.
pub fn in_view(marks: &[Mark], top: usize, bottom: usize) -> &[Mark] {
    let first = marks.partition_point(|m| m.end < top);
    let behind = marks.partition_point(|m| m.start <= bottom);
    marks.get(first..behind.max(first)).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::layout::NoImages;
    use crate::state::ViewFacts;
    use crate::sync::Snapshot;

    fn mark(start: usize, end: usize) -> Mark {
        Mark {
            kind: ChangeKind::Modified,
            start,
            end,
        }
    }

    #[test]
    fn only_marks_that_reach_into_view_are_looked_at() {
        let marks = [mark(0, 5), mark(10, 20), mark(30, 40), mark(50, 50)];
        assert_eq!(in_view(&marks, 12, 35), &marks[1..3]);
        assert_eq!(in_view(&marks, 21, 29), &[] as &[Mark]);
        assert_eq!(in_view(&marks, 50, 60), &marks[3..]);
        assert_eq!(in_view(&marks, 0, 100), &marks[..]);
        assert_eq!(in_view(&[], 0, 100), &[] as &[Mark]);
    }

    #[test]
    fn a_change_is_marked_in_the_source_by_its_text_and_in_the_preview_by_its_paragraphs() {
        let text = "# Über\n\nEins\n\nZwei\n";
        let mut st = State::new();
        let snap = Snapshot::new(text.to_string(), text.encode_utf16().count(), 1);
        let facts = ViewFacts {
            caret_at: None,
            visible: true,
            shown_len: 0,
            dark: false,
        };
        let _ = st.advance(&snap, facts, None, |_| NoImages);
        let zwei = text.find("Zwei").unwrap();
        let change = |kind, bytes: std::ops::Range<usize>| Change {
            kind,
            lines: 0..0,
            bytes,
        };
        let (source, preview) = marks(
            &[
                change(ChangeKind::Modified, 0..8),
                change(ChangeKind::Removed, zwei..zwei),
                change(ChangeKind::Added, zwei..text.len()),
            ],
            text,
            &st,
        );
        assert_eq!(source[0], mark(0, 7), "UTF-16 units: the umlaut is one");
        assert_eq!((source[2].start, source[2].end), (14, 19));
        let preview_text: String = st
            .layout()
            .blocks
            .iter()
            .flat_map(|b| &b.paras)
            .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>() + "\n")
            .collect();
        let shown = |at: usize| preview_text[..at].encode_utf16().count();
        assert_eq!(preview[0].start, 0);
        assert_eq!(
            preview[0].end,
            shown(preview_text.find('\n').unwrap()) + 1,
            "its paragraph and no further"
        );
        let zwei_shown = shown(preview_text.find("Zwei").unwrap());
        assert_eq!(
            (preview[1].start, preview[1].end),
            (zwei_shown, zwei_shown),
            "a removal is a place"
        );
        assert_eq!(preview[2].start, zwei_shown);
        assert!(preview[2].end > zwei_shown);
    }

    #[test]
    fn a_file_outside_a_repository_compares_with_nothing() {
        let dir = std::env::temp_dir().join(format!("splat-no-repository-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.md");
        std::fs::write(&file, "x").unwrap();
        assert!(matches!(read_base(&file, None), BaseLook::Nothing));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_commit_gives_the_text_once_until_head_moves() {
        let dir = std::env::temp_dir().join(format!("splat-repository-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .is_ok_and(|o| o.status.success())
        };
        if !git(&["init", "-q"]) {
            eprintln!("no git here");
            return;
        }
        let file = dir.join("a.md");
        std::fs::write(&file, "\u{FEFF}Text\n").unwrap();
        assert!(git(&["add", "a.md"]));
        assert!(git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "x"
        ]));
        let BaseLook::New { commit, text, .. } = read_base(&file, None) else {
            panic!("the file is in the commit");
        };
        assert_eq!(text, "Text\n", "without its byte order mark, as the text view holds it");
        assert!(matches!(read_base(&file, Some(commit)), BaseLook::Unchanged));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
