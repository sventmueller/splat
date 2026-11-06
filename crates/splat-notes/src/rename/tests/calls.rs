//! What `renames` and `applied` promise their callers.

use super::*;

#[test]
fn notes_are_asked_for_once_and_in_order_and_only_those_that_change_are_named() {
    let tree = tree_of(&["a.md", "Old.md", "z/b.md", "z/c.md"]);
    let texts = HashMap::from([
        ("/v/a.md", "[x](Old.md)\n"),
        ("/v/Old.md", "text\n"),
        ("/v/z/b.md", "[x](../Old.md)\n"),
    ]);
    let mut asked = Vec::new();
    let rewrites = renames(
        &tree,
        Path::new("/v/Old.md"),
        Path::new("/v/New.md"),
        &AtomicBool::new(false),
        &mut |path| {
            asked.push(path.display().to_string());
            texts.get(path.to_str().unwrap()).map(ToString::to_string)
        },
    );
    assert_eq!(asked, ["/v/a.md", "/v/Old.md", "/v/z/b.md", "/v/z/c.md"]);
    let changed: Vec<&Path> = rewrites.iter().map(|rewrite| rewrite.path.as_path()).collect();
    assert_eq!(changed, [Path::new("/v/a.md"), Path::new("/v/z/b.md")]);
}

#[test]
fn a_rename_that_is_none_changes_nothing() {
    let tree = tree_of(&["from.md", "Old.md", "sub/x.md"]);
    let mut read = |_: &Path| -> Option<String> { panic!("no need to read") };
    let no_stop = AtomicBool::new(false);
    let rename = |from: &str, to: &str, read: &mut dyn FnMut(&Path) -> Option<String>| {
        renames(&tree, Path::new(from), Path::new(to), &no_stop, read)
    };
    assert_eq!(rename("/v/Old.md", "/v/Old.md", &mut read), []);
    assert_eq!(rename("/v/Old.md", "/v/sub/Old.md", &mut read), [], "another folder");
}

#[test]
fn a_stop_gives_no_changes_at_all() {
    let tree = tree_of(&["a.md", "Old.md"]);
    let stop = AtomicBool::new(false);
    let rewrites = renames(
        &tree,
        Path::new("/v/Old.md"),
        Path::new("/v/New.md"),
        &stop,
        &mut |_| {
            stop.store(true, Ordering::Relaxed);
            Some("[x](Old.md)\n".to_string())
        },
    );
    assert_eq!(rewrites, []);
}

#[test]
fn replacements_that_do_not_fit_are_left_out() {
    let replacement = |range: Range<usize>, text: &str| Replacement {
        range,
        text: text.to_string(),
    };
    assert_eq!(applied("abc", &[]), "abc");
    assert_eq!(applied("abc", &[replacement(1..2, "XY")]), "aXYc");
    assert_eq!(
        applied("abc", &[replacement(2..3, "Y"), replacement(0..1, "X")]),
        "XbY",
        "in any order"
    );
    assert_eq!(
        applied("abc", &[replacement(0..2, "X"), replacement(1..3, "Y")]),
        "Xc",
        "inside another"
    );
    assert_eq!(applied("abc", &[replacement(2..9, "X")]), "abc", "beyond the text");
    assert_eq!(applied("äb", &[replacement(1..2, "X")]), "äb", "inside a letter");
    #[expect(clippy::reversed_empty_ranges, reason = "what the function has to bear")]
    let backwards = replacement(2..1, "X");
    assert_eq!(applied("abc", &[backwards]), "abc", "backwards");
}
