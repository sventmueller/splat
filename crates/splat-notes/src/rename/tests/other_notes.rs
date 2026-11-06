//! Links to other notes of the old or the new name.

use super::*;

#[test]
fn a_wiki_link_to_a_nearer_note_of_the_same_name_stays() {
    let tree = tree_of(&[
        "from.md",
        "a/Old.md",
        "a/from.md",
        "a/b/Old.md",
        "a/b/from.md",
        "a/b/c/Old.md",
    ]);
    let rename = |file: &str, text: &str, from: &str, to: &str| renamed_in(&tree, file, text, from, to);
    let (from, to) = ("/v/a/Old.md", "/v/a/New.md");
    assert_eq!(rename("/v/a/from.md", "[[Old]]", from, to), "[[New]]");
    assert_eq!(
        rename("/v/a/b/from.md", "[[Old]]", from, to),
        "[[Old]]",
        "its own folder has one"
    );
    assert_eq!(
        rename("/v/from.md", "[[Old]]", from, to),
        "[[New]]",
        "the nearest from above"
    );
    assert_eq!(
        rename("/v/from.md", "[[a/b/Old]]", from, to),
        "[[a/b/Old]]",
        "names the other one"
    );
    let (from, to) = ("/v/a/b/c/Old.md", "/v/a/b/c/New.md");
    assert_eq!(
        rename("/v/from.md", "[[Old]]", from, to),
        "[[Old]]",
        "a nearer one stands in the way"
    );
}

#[test]
fn a_link_to_another_note_of_the_new_name_keeps_leading_there() {
    let tree = tree_of(&["from.md", "Old.md", "deep/New.md", "other/x.md"]);
    let rename = |file: &str, text: &str| renamed_in(&tree, file, text, "/v/Old.md", "/v/New.md");
    assert_eq!(rename("/v/from.md", "[[New]]"), "[[deep/New]]");
    assert_eq!(rename("/v/from.md", "[[New|t]]"), "[[deep/New|t]]");
    assert_eq!(rename("/v/from.md", "[[New.md#h]]"), "[[deep/New#h]]");
    assert_eq!(rename("/v/from.md", "[[deep/New]]"), "[[deep/New]]");
    assert_eq!(rename("/v/from.md", "[[x]]"), "[[x]]");
}

#[test]
fn a_link_that_the_new_name_would_send_elsewhere_gets_a_path() {
    let tree = tree_of(&["from.md", "sub/from.md", "sub/x/New.md", "sub/y/z/Old.md"]);
    let rename = |file: &str, text: &str| renamed_in(&tree, file, text, "/v/sub/y/z/Old.md", "/v/sub/y/z/New.md");
    assert_eq!(rename("/v/from.md", "[[Old]]"), "[[sub/y/z/New]]");
    assert_eq!(rename("/v/sub/from.md", "[[Old#h|t]]"), "[[y/z/New#h|t]]");
    let tree = tree_of(&["from.md", "sub/x/New.markdown", "sub/y/z/Old.markdown"]);
    let rename = renamed_in(
        &tree,
        "/v/from.md",
        "[[Old.markdown]]",
        "/v/sub/y/z/Old.markdown",
        "/v/sub/y/z/New.markdown",
    );
    assert_eq!(rename, "[[sub/y/z/New.markdown]]");
}
