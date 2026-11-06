//! The search through the notes of a tree on the disk.

use super::*;

#[test]
fn the_search_goes_through_the_tree_in_order_and_says_nothing_of_notes_without_hits() {
    let folder = Folder::new("search");
    folder.note("b.md", "Wort\n");
    folder.note("a.md", "kein\n");
    folder.note("sub/c.md", "Wort Wort\n");
    folder.note("d.txt", "Wort\n");
    let tree = scan(folder.path(), &AtomicBool::new(false));
    let query = Query::Text {
        needle: "wort".into(),
        case_sensitive: false,
    };
    let mut found = Vec::new();
    search(&tree, &query, &AtomicBool::new(false), &mut |hits| found.push(hits));
    let names: Vec<(&str, usize)> = found
        .iter()
        .map(|f| (f.path.file_name().unwrap().to_str().unwrap(), f.hits.len()))
        .collect();
    assert_eq!(names, [("c.md", 2), ("b.md", 1)]);
}

#[test]
fn a_note_too_large_or_not_in_utf8_is_skipped_and_a_byte_order_mark_is_fine() {
    let folder = Folder::new("skipped");
    folder.note("fine.md", "\u{FEFF}Wort\n");
    std::fs::write(folder.path().join("latin.md"), b"Wort \xE4\n").unwrap();
    let large = std::fs::File::create(folder.path().join("large.md")).unwrap();
    large.set_len(MOST_FILE_BYTES + 1).unwrap();
    let tree = scan(folder.path(), &AtomicBool::new(false));
    assert_eq!(tree.notes().count(), 3);
    let query = Query::Text {
        needle: "Wort".into(),
        case_sensitive: true,
    };
    let mut found = Vec::new();
    search(&tree, &query, &AtomicBool::new(false), &mut |hits| found.push(hits));
    assert_eq!(found.len(), 1);
    assert!(found[0].path.ends_with("fine.md"));
    assert_eq!(found[0].hits[0].range, 3..7);
}

#[test]
fn a_note_of_just_the_limit_is_read_and_one_gone_is_passed_over() {
    let folder = Folder::new("limit");
    let exact = std::fs::File::create(folder.path().join("exact.md")).unwrap();
    exact.set_len(MOST_FILE_BYTES).unwrap();
    assert!(read_note(&folder.path().join("exact.md")).unwrap().is_some());
    assert!(read_note(&folder.path().join("gone.md")).is_err());
    assert_eq!(read_note(folder.path()).unwrap(), None, "a folder");
}

#[test]
fn a_stop_ends_the_search() {
    let folder = Folder::new("stop");
    folder.note("a.md", "x");
    let tree = scan(folder.path(), &AtomicBool::new(false));
    let query = Query::Text {
        needle: "x".into(),
        case_sensitive: true,
    };
    let mut found = 0;
    search(&tree, &query, &AtomicBool::new(true), &mut |_| found += 1);
    assert_eq!(found, 0);
    search(&tree, &query, &AtomicBool::new(false), &mut |_| found += 1);
    assert_eq!(found, 1);
}

#[test]
fn notes_that_cannot_be_read_are_counted_and_passed_over() {
    use std::os::unix::fs::PermissionsExt;
    let folder = Folder::new("unreadable");
    folder.note("a.md", "Wort\n");
    folder.note("b.md", "Wort\n");
    folder.note("c.md", "Wort\n");
    let tree = scan(folder.path(), &AtomicBool::new(false));
    let locked = folder.path().join("b.md");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    std::fs::remove_file(folder.path().join("c.md")).unwrap();
    let query = Query::Text {
        needle: "wort".into(),
        case_sensitive: false,
    };
    let mut found = Vec::new();
    let searched = search(&tree, &query, &AtomicBool::new(false), &mut |hits| {
        found.push(hits.path)
    });
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(found, [folder.path().join("a.md")]);
    assert_eq!(
        searched,
        Searched { unreadable: 2 },
        "one locked, one gone since the walk"
    );
}
