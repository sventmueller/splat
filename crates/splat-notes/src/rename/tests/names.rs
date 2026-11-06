//! How a new name is written into a link: escapes, extensions and case.

use super::*;

#[test]
fn an_address_keeps_the_form_of_its_escapes() {
    let tree = tree_of(&["from.md", "My Old.md", "M%C3%BCller.md", "Müller.md"]);
    let rename = |text: &str, from: &str, to: &str| renamed_in(&tree, "/v/from.md", text, from, to);
    let (old, new) = ("/v/My Old.md", "/v/New Name.md");
    assert_eq!(rename("[t](My%20Old.md)", old, new), "[t](New%20Name.md)");
    assert_eq!(rename("[t](<My Old.md>)", old, new), "[t](<New Name.md>)");
    assert_eq!(
        rename("[r]\n\n[r]: My%20Old.md\n", old, new),
        "[r]\n\n[r]: New%20Name.md\n"
    );
    assert_eq!(
        rename("[r]\n\n[r]: <My Old.md>\n", old, new),
        "[r]\n\n[r]: <New Name.md>\n"
    );
    assert_eq!(rename("[[My Old]]", old, new), "[[New Name]]");
    assert_eq!(rename("[t](Müller.md)", "/v/Müller.md", "/v/Größe.md"), "[t](Größe.md)");
    assert_eq!(
        rename("[t](M%C3%BCller.md)", "/v/Müller.md", "/v/Größe.md"),
        "[t](Gr%C3%B6%C3%9Fe.md)"
    );
    assert_eq!(
        rename("[t](M%c3%bcller.md)", "/v/Müller.md", "/v/Größe.md"),
        "[t](Gr%C3%B6%C3%9Fe.md)"
    );
    assert_eq!(
        rename("[t](Müller.md)", "/v/Müller.md", "/v/a b.md"),
        "[t](a%20b.md)",
        "no space without an escape"
    );
    assert_eq!(
        rename("[t](<Müller.md>)", "/v/Müller.md", "/v/a (1).md"),
        "[t](<a (1).md>)"
    );
    assert_eq!(
        rename("[t](Müller.md)", "/v/Müller.md", "/v/a (1).md"),
        "[t](a%20%281%29.md)"
    );
}

#[test]
fn what_ends_or_starts_an_escape_in_a_new_name_gets_one_of_its_own() {
    let tree = tree_of(&["from.md", "Old.md"]);
    let rename = |text: &str, to: &str| renamed_in(&tree, "/v/from.md", text, "/v/Old.md", to);
    assert_eq!(rename("[t](Old.md)", "/v/100%.md"), "[t](100%25.md)");
    assert_eq!(rename("[t](<Old.md>)", "/v/100%.md"), "[t](<100%25.md>)");
    assert_eq!(rename("[t](Old.md)", "/v/C#.md"), "[t](C%23.md)");
    assert_eq!(rename("[t](Old.md#h)", "/v/q?.md"), "[t](q%3F.md#h)");
    assert_eq!(rename("[t](Old.md)", "/v/a<b>.md"), "[t](a%3Cb%3E.md)");
    assert_eq!(rename("[t](Old.md)", "/v/a\\b.md"), "[t](a%5Cb.md)");
    assert_eq!(rename("[t](Old.md)", "/v/a&amp;b.md"), "[t](a%26amp;b.md)");
    assert_eq!(rename("[t](Old.md)", "/v/Q&A.md"), "[t](Q&A.md)");
    assert_eq!(rename("[t](Old.md)", "/v/a\tb.md"), "[t](a%09b.md)");
    assert_eq!(rename("[t](Old.md)", "/v/a%20b.md"), "[t](a%2520b.md)");
    assert_eq!(
        rename("[[Old]]", "/v/100%.md"),
        "[[100%]]",
        "a wiki link has no escapes"
    );
}

#[test]
fn a_wiki_link_that_cannot_hold_the_new_name_stays() {
    let tree = tree_of(&["from.md", "Old.md"]);
    let rename = |text: &str, to: &str| renamed_in(&tree, "/v/from.md", text, "/v/Old.md", to);
    for to in [
        "/v/C#.md",
        "/v/a|b.md",
        "/v/a^b.md",
        "/v/a]b.md",
        "/v/a[b.md",
        "/v/a..b.md",
        "/v/a&amp;b.md",
        "/v/a\\b.md",
        "/v/ a.md",
    ] {
        assert_eq!(rename("[[Old]]", to), "[[Old]]", "{to}");
        assert_ne!(rename("[t](Old.md)", to), "[t](Old.md)", "{to}");
    }
}

#[test]
fn a_rename_that_changes_the_extension_writes_it_where_it_must() {
    let tree = tree_of(&["from.md", "Old.md", "Text.markdown"]);
    let rename = |text: &str, from: &str, to: &str| renamed_in(&tree, "/v/from.md", text, from, to);
    assert_eq!(rename("[[Old]]", "/v/Old.md", "/v/New.markdown"), "[[New.markdown]]");
    assert_eq!(rename("[[Old.md]]", "/v/Old.md", "/v/New.markdown"), "[[New.markdown]]");
    assert_eq!(
        rename("[[Text.markdown]]", "/v/Text.markdown", "/v/New.md"),
        "[[New.md]]"
    );
    assert_eq!(
        rename("[[Text.markdown]]", "/v/Text.markdown", "/v/New.markdown"),
        "[[New.markdown]]"
    );
    assert_eq!(rename("[[Old]]", "/v/Old.md", "/v/New.MD"), "[[New]]");
    assert_eq!(
        rename("[t](Old.md)", "/v/Old.md", "/v/New.markdown"),
        "[t](New.markdown)"
    );
    assert_eq!(
        rename("[[Old]]", "/v/Old.md", "/v/New"),
        "[[New]]",
        "the plain name is found first"
    );
}

#[test]
fn a_change_of_case_alone_is_a_rename_as_well() {
    let tree = tree_of(&["from.md", "old.md"]);
    let rename = |text: &str| renamed_in(&tree, "/v/from.md", text, "/v/old.md", "/v/Old.md");
    assert_eq!(
        rename("[[old]] [[Old]] [t](old.md) [u](Old.md)"),
        "[[Old]] [[Old]] [t](Old.md) [u](Old.md)"
    );
}

#[test]
fn the_parts_of_a_written_name_are_found() {
    assert_eq!(file_name_in("a/b.md#c"), Some(2..6));
    assert_eq!(file_name_in("a/b.md?c"), Some(2..6));
    assert_eq!(file_name_in("b.md"), Some(0..4));
    assert_eq!(file_name_in("a%2Fb.md"), Some(4..8));
    assert_eq!(file_name_in("a%2fb.md"), Some(4..8));
    assert_eq!(file_name_in("a\\_b/c&#35;d"), Some(5..6));
    assert_eq!(file_name_in("a/"), None);
    assert_eq!(file_name_in(""), None);
    assert_eq!(name_in(" a/b #c"), Some((1..4, 3)));
    assert_eq!(name_in("b|c"), Some((0..1, 0)));
    assert_eq!(name_in("b\\|c"), Some((0..1, 0)));
    assert_eq!(name_in(" "), None);
    assert_eq!(name_in("#c"), None);
    assert_eq!(
        wiki_path(Path::new("/v/a/b.md"), Path::new("/v")).as_deref(),
        Some("a/b")
    );
    assert_eq!(
        wiki_path(Path::new("/v/a/b.MD"), Path::new("/v")).as_deref(),
        Some("a/b")
    );
    assert_eq!(
        wiki_path(Path::new("/v/a/b.mdown"), Path::new("/v")).as_deref(),
        Some("a/b.mdown")
    );
    assert_eq!(wiki_path(Path::new("/w/b.md"), Path::new("/v")), None);
}
