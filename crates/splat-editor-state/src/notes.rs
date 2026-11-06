//! What the sidebar's notes decide without a window: the rows of its lists, what a search looks for, where a hit lies in a
//! text that may have changed, the new name of a note, and how a rename changes the texts that link to it.

use std::ops::Range;
use std::path::{Path, PathBuf};

use splat_notes::{FileHits, MOST_HITS_PER_FILE, Query, Replacement, Tree};

use crate::i18n::{t, tf};
use crate::markdown::edit::Edit;
use crate::markdown::text::Byte;

/// What a row of a list in the sidebar stands for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RowKind {
    /// A folder of notes.
    Folder,
    /// A note.
    Note(PathBuf),
    /// A hit in a note: the note, the line and the place in it.
    Hit(PathBuf, usize, usize),
    /// A line that only tells something, such as that more hits stood there.
    Remark,
}

/// A row of a list in the sidebar.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    /// What the row shows.
    pub title: String,
    /// The part of the title that matched, in bytes.
    pub matched: Option<Range<usize>>,
    /// What the row's tooltip says.
    pub tip: String,
    /// The SF Symbol in front of the title, if it has one.
    pub symbol: &'static str,
    /// What the row stands for.
    pub kind: RowKind,
    /// Stays the same across a reload, so that a folder stays open.
    pub key: String,
    /// The rows nested under this one, as indexes of the list.
    pub children: Vec<usize>,
}

/// The rows of a folder's tree.
pub fn rows_of_tree(tree: &Tree) -> (Vec<Row>, Vec<usize>) {
    let rows = tree
        .nodes()
        .iter()
        .map(|node| Row {
            title: node.name().to_string(),
            matched: None,
            tip: node.path().display().to_string(),
            symbol: if node.is_folder() { "folder" } else { "doc.text" },
            kind: if node.is_folder() {
                RowKind::Folder
            } else {
                RowKind::Note(node.path().to_path_buf())
            },
            key: node.path().display().to_string(),
            children: node.children().to_vec(),
        })
        .collect();
    (rows, tree.top().to_vec())
}

/// The rows of search results: a row per file, relative to the folder, with its hits below.
pub fn rows_of_hits(hits: &[FileHits], root: Option<&Path>) -> (Vec<Row>, Vec<usize>) {
    let mut rows = Vec::new();
    let mut roots = Vec::new();
    for file in hits {
        let shown = root
            .and_then(|root| file.path.strip_prefix(root).ok())
            .unwrap_or(&file.path)
            .display()
            .to_string();
        let top = rows.len();
        roots.push(top);
        rows.push(Row {
            title: shown,
            matched: None,
            tip: file.path.display().to_string(),
            symbol: "doc.text",
            kind: RowKind::Note(file.path.clone()),
            key: file.path.display().to_string(),
            children: Vec::new(),
        });
        for hit in &file.hits {
            let child = rows.len();
            rows[top].children.push(child);
            let (title, matched) = near_the_match(&hit.context, hit.in_context.clone());
            rows.push(Row {
                title,
                matched: Some(matched),
                tip: format!("{}\n{}", tf("Zeile {}", &[&(hit.line + 1).to_string()]), hit.context),
                symbol: "",
                kind: RowKind::Hit(file.path.clone(), hit.line, hit.column),
                key: format!("{}#{}", file.path.display(), hit.range.start),
                children: Vec::new(),
            });
        }
        if file.more {
            let child = rows.len();
            rows[top].children.push(child);
            rows.push(Row {
                title: tf("Mehr als {} Treffer", &[&MOST_HITS_PER_FILE.to_string()]),
                matched: None,
                tip: String::new(),
                symbol: "",
                kind: RowKind::Remark,
                key: format!("{}#more", file.path.display()),
                children: Vec::new(),
            });
        }
    }
    (rows, roots)
}

/// What the status line says about a search.
pub fn search_status(hits: &[FileHits], searching: bool) -> String {
    let count: usize = hits.iter().map(|f| f.hits.len()).sum();
    match (count, searching) {
        (0, true) => t("Suche läuft …").to_string(),
        (0, false) => t("Keine Treffer").to_string(),
        _ => tf(
            "Treffer: {} · Dateien: {}",
            &[&count.to_string(), &hits.len().to_string()],
        ),
    }
}

/// The rows from the top of the list down to the note at `path`.
pub fn chain_to(rows: &[Row], roots: &[usize], path: &Path) -> Option<Vec<usize>> {
    for &root in roots {
        let row = rows.get(root)?;
        if matches!(&row.kind, RowKind::Note(p) if p == path) {
            return Some(vec![root]);
        }
        if let Some(mut below) = chain_to(rows, &row.children, path) {
            below.insert(0, root);
            return Some(below);
        }
    }
    None
}

/// What a search for `text` looks for: `#tag` for that tag, anything else for the text as typed; `None` for nothing.
pub fn query_of(text: &str) -> Option<Query> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let is_tag = text.len() > 1 && text.starts_with('#') && !text.contains(char::is_whitespace);
    Some(if is_tag {
        Query::Tag(text.to_string())
    } else {
        Query::Text {
            needle: text.to_string(),
            case_sensitive: false,
        }
    })
}

/// A line of a hit as a narrow sidebar has room for it: it starts shortly before the match, which would otherwise lie
/// beyond the edge of the row. Returns the line and where the match stands in it.
pub fn near_the_match(context: &str, matched: Range<usize>) -> (String, Range<usize>) {
    const LEAD_CHARS: usize = 12;
    let lead = context.get(..matched.start).unwrap_or("");
    let Some((cut, _)) = lead.char_indices().rev().nth(LEAD_CHARS) else {
        return (context.to_string(), matched);
    };
    let dots = "…";
    let moved = cut.min(matched.start);
    let start = matched.start - moved + dots.len();
    (format!("{dots}{}", &context[moved..]), start..start + matched.len())
}

/// A new name for a note: its stem as typed, with the note's extension unless one is typed. `None` if it cannot be a name.
pub fn renamed_path(path: &Path, typed: &str) -> Option<PathBuf> {
    let typed = typed.trim();
    if typed.is_empty() || typed.contains('/') || typed.contains(':') || typed.starts_with('.') {
        return None;
    }
    let name = if splat_notes::is_note(Path::new(typed)) {
        typed.to_string()
    } else {
        let extension = path.extension()?.to_string_lossy();
        format!("{typed}.{extension}")
    };
    let to = path.with_file_name(name);
    (to != path).then_some(to)
}

/// Writes every text first and moves them all into place after, so that a full disk changes none of them. Each write is
/// a file, the text it was computed from and the text it gets; returns the files that kept their text.
pub fn write_rewrites(writes: &[(PathBuf, String, String)]) -> Vec<PathBuf> {
    let mut failed = Vec::new();
    let mut ready = Vec::new();
    for (path, before, after) in writes {
        let Some(name) = path.file_name() else { continue };
        // What another program changed in the meantime stays as it is.
        let current = std::fs::read(path).ok();
        if current.as_deref() != Some(before.as_bytes()) {
            failed.push(path.clone());
            continue;
        }
        let spare = path.with_file_name(format!(".{}.splat-rename", name.to_string_lossy()));
        let kept = std::fs::metadata(path).ok().map(|m| m.permissions());
        let written = std::fs::write(&spare, after).is_ok()
            && kept.is_none_or(|permissions| std::fs::set_permissions(&spare, permissions).is_ok());
        if written {
            ready.push((spare, path.clone()));
        } else {
            let _ = std::fs::remove_file(&spare);
            failed.push(path.clone());
        }
    }
    if !failed.is_empty() {
        for (spare, _) in &ready {
            let _ = std::fs::remove_file(spare);
        }
        return writes.iter().map(|(path, ..)| path.clone()).collect();
    }
    for (spare, path) in ready {
        if std::fs::rename(&spare, &path).is_err() {
            let _ = std::fs::remove_file(&spare);
            failed.push(path);
        }
    }
    failed
}

/// A line and a place in it as a position of `text`, as near as the text comes: it may have changed since the search.
pub fn place_in_text(text: &str, line: usize, column: usize) -> Byte {
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut seen = 0;
    while seen < line {
        let Some(found) = bytes[start..].iter().position(|&b| b == b'\n' || b == b'\r') else {
            return Byte::new(text.len());
        };
        let at = start + found;
        start = at
            + if bytes[at] == b'\r' && bytes.get(at + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
        seen += 1;
    }
    let line_end = bytes[start..]
        .iter()
        .position(|&b| b == b'\n' || b == b'\r')
        .map_or(text.len(), |found| start + found);
    Byte::new(text.floor_char_boundary((start + column).min(line_end)))
}

/// Changes to a text as one edit: from the first replacement to the last, what lies between stays as it was.
pub fn single_edit(text: &str, replacements: &[Replacement]) -> Option<Edit> {
    let start = replacements.iter().map(|r| r.range.start).min()?;
    let end = replacements.iter().map(|r| r.range.end).max()?;
    let mut sorted: Vec<&Replacement> = replacements.iter().collect();
    sorted.sort_by_key(|r| r.range.start);
    let mut replacement = String::new();
    let mut at = start;
    for r in sorted {
        replacement.push_str(text.get(at..r.range.start)?);
        replacement.push_str(&r.text);
        at = r.range.end;
    }
    replacement.push_str(text.get(at..end)?);
    let selection = Byte::new(start + replacement.len());
    Some(Edit {
        range: Byte::new(start)..Byte::new(end),
        replacement,
        selection: selection..selection,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use splat_notes::Hit;

    fn row(kind: RowKind, children: Vec<usize>) -> Row {
        Row {
            title: String::new(),
            matched: None,
            tip: String::new(),
            symbol: "",
            kind,
            key: String::new(),
            children,
        }
    }

    #[test]
    fn a_tree_becomes_rows_with_the_same_shape() {
        let tree = Tree::of_notes(Path::new("/r"), &["d/n.md"]);
        let (rows, roots) = rows_of_tree(&tree);
        assert_eq!(roots, [0]);
        assert_eq!(rows[0].kind, RowKind::Folder);
        assert_eq!(rows[0].children, [1]);
        assert_eq!(rows[1].kind, RowKind::Note(PathBuf::from("/r/d/n.md")));
    }

    #[test]
    fn hits_stand_below_their_file_by_the_path_inside_the_folder() {
        let hits = [FileHits {
            path: PathBuf::from("/r/d/n.md"),
            hits: vec![Hit {
                line: 2,
                column: 4,
                range: 40..43,
                context: "ein Wort".to_string(),
                in_context: 4..8,
            }],
            more: true,
        }];
        let (rows, roots) = rows_of_hits(&hits, Some(Path::new("/r")));
        assert_eq!(roots, [0]);
        assert_eq!(rows[0].title, "d/n.md");
        assert_eq!(rows[0].children, [1, 2]);
        assert_eq!(rows[1].kind, RowKind::Hit(PathBuf::from("/r/d/n.md"), 2, 4));
        assert_eq!(rows[1].matched, Some(4..8));
        assert_eq!(rows[2].kind, RowKind::Remark);
    }

    #[test]
    fn the_status_says_what_a_search_came_to() {
        let none: [FileHits; 0] = [];
        assert_ne!(search_status(&none, true), search_status(&none, false));
        let some = [FileHits {
            path: PathBuf::from("/a.md"),
            hits: vec![Hit {
                line: 0,
                column: 0,
                range: 0..1,
                context: "a".to_string(),
                in_context: 0..1,
            }],
            more: false,
        }];
        let status = search_status(&some, true);
        assert!(status.contains('1'), "{status}");
    }

    #[test]
    fn the_way_to_a_note_leads_through_its_folders() {
        let note = |path: &str| RowKind::Note(PathBuf::from(path));
        let rows = vec![
            row(RowKind::Folder, vec![1, 2]),
            row(RowKind::Folder, vec![3]),
            row(note("/a/b.md"), vec![]),
            row(note("/a/x/c.md"), vec![]),
            row(note("/d.md"), vec![]),
        ];
        let roots = [0, 4];
        assert_eq!(chain_to(&rows, &roots, std::path::Path::new("/d.md")), Some(vec![4]));
        assert_eq!(
            chain_to(&rows, &roots, std::path::Path::new("/a/b.md")),
            Some(vec![0, 2])
        );
        assert_eq!(
            chain_to(&rows, &roots, std::path::Path::new("/a/x/c.md")),
            Some(vec![0, 1, 3])
        );
        assert_eq!(chain_to(&rows, &roots, std::path::Path::new("/nicht.md")), None);
    }

    #[test]
    fn a_hash_sign_in_front_of_one_word_looks_for_a_tag() {
        assert_eq!(query_of("#idee"), Some(Query::Tag("#idee".to_string())));
        assert_eq!(
            query_of("#idee und mehr"),
            Some(Query::Text {
                needle: "#idee und mehr".to_string(),
                case_sensitive: false
            })
        );
        assert_eq!(
            query_of("#"),
            Some(Query::Text {
                needle: "#".to_string(),
                case_sensitive: false
            })
        );
        assert_eq!(query_of("  "), None);
    }

    #[test]
    fn a_new_name_keeps_the_extension_and_stays_in_the_folder() {
        let path = Path::new("/a/Alt.md");
        assert_eq!(renamed_path(path, "Neu"), Some(PathBuf::from("/a/Neu.md")));
        assert_eq!(renamed_path(path, " Neu "), Some(PathBuf::from("/a/Neu.md")));
        assert_eq!(
            renamed_path(path, "Neu.markdown"),
            Some(PathBuf::from("/a/Neu.markdown"))
        );
        assert_eq!(renamed_path(path, "x/Neu"), None);
        assert_eq!(renamed_path(path, "a:b"), None);
        assert_eq!(renamed_path(path, ".versteckt"), None);
        assert_eq!(renamed_path(path, ""), None);
        assert_eq!(renamed_path(path, "Alt"), None, "nothing to do");
        assert_eq!(renamed_path(Path::new("/a/Ohne"), "Neu"), None, "no extension to keep");
    }

    #[test]
    fn a_long_line_starts_shortly_before_its_match() {
        assert_eq!(near_the_match("kurz Wort", 5..9), ("kurz Wort".to_string(), 5..9));
        let line = "Ein ganz langer Satz vor dem äpfel und mehr";
        let at = line.find("äpfel").unwrap();
        let (shown, matched) = near_the_match(line, at..at + "äpfel".len());
        assert_eq!(&shown[matched], "äpfel");
        assert!(shown.starts_with('…'), "{shown}");
        assert!(shown.len() < line.len());
        assert_eq!(near_the_match("äöü", 0..2), ("äöü".to_string(), 0..2));
    }

    #[test]
    fn several_replacements_make_one_edit_and_leave_what_lies_between() {
        let text = "ein [[Alt]] und [[Alt|x]] Ende";
        let at = |needle: usize| needle..needle + 3;
        let edit = single_edit(
            text,
            &[
                Replacement {
                    range: at(18),
                    text: "Neu".to_string(),
                },
                Replacement {
                    range: at(6),
                    text: "Neu".to_string(),
                },
            ],
        )
        .unwrap();
        assert_eq!(edit.applied_to(text), "ein [[Neu]] und [[Neu|x]] Ende");
        assert_eq!(edit.range.start.get(), 6);
        assert_eq!(edit.range.end.get(), 21);
        assert_eq!(single_edit(text, &[]), None);
    }

    #[test]
    fn a_hit_is_found_again_by_its_line_whatever_ends_the_lines() {
        let at = |text: &str, line, column| place_in_text(text, line, column).get();
        assert_eq!(at("ab\ncd\nef", 1, 1), 4);
        assert_eq!(at("ab\r\ncd\r\nef", 2, 1), 9);
        assert_eq!(at("ab\rcd", 1, 0), 3);
        assert_eq!(at("ab\ncd", 0, 99), 2, "not beyond its line");
        assert_eq!(at("ab", 5, 0), 2, "the text has lost lines since");
        assert_eq!(at("äb", 0, 1), 0, "on a letter");
    }

    #[test]
    fn links_change_in_all_files_or_in_none() {
        let dir = std::env::temp_dir().join(format!("splat-rewrites-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.md"), dir.join("b.md"));
        std::fs::write(&a, "alt a").unwrap();
        std::fs::write(&b, "alt b").unwrap();
        let writes = vec![
            (a.clone(), "alt a".to_string(), "neu a".to_string()),
            (b.clone(), "alt b".to_string(), "neu b".to_string()),
        ];
        assert_eq!(write_rewrites(&writes), Vec::<PathBuf>::new());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "neu a");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "neu b");

        std::fs::write(&b, "anders").unwrap();
        let writes = vec![
            (a.clone(), "neu a".to_string(), "x".to_string()),
            (b.clone(), "neu b".to_string(), "y".to_string()),
        ];
        assert_eq!(write_rewrites(&writes).len(), 2, "another program changed one of them");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "neu a", "so the other stays too");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "anders");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2, "nothing lies around");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
