//! Searches for a tag and for the links to a note.

use super::*;

fn tags(text: &str, tag: &str) -> Vec<String> {
    let found = hits_in_text(text, &Query::Tag(tag.into()), Path::new("/n.md"), &tree());
    found
        .hits
        .iter()
        .map(|hit| text[hit.range.clone()].to_string())
        .collect()
}

#[test]
fn a_tag_also_finds_what_lies_below_it() {
    assert_eq!(
        tags("#project #project/splat #projects\n", "project"),
        ["#project", "#project/splat"]
    );
    assert_eq!(tags("#project/splat #project\n", "project/splat"), ["#project/splat"]);
    assert_eq!(tags("#project\n", "#project"), ["#project"]);
    assert_eq!(tags("#project\n", "project/"), ["#project"]);
    assert_eq!(tags("#Project #PROJECT/x\n", "project"), ["#Project", "#PROJECT/x"]);
    assert_eq!(tags("#Straße\n", "STRASSE"), ["#Straße"]);
    assert_eq!(tags("#灵感\n", "灵感"), ["#灵感"]);
    assert_eq!(tags("#a\n", ""), Vec::<String>::new());
    assert_eq!(tags("#a\n", "#"), Vec::<String>::new());
}

#[test]
fn a_tag_counts_where_a_reader_sees_one() {
    assert_eq!(
        tags(
            "# #a\n\n*#a* **x #a** > #a\n\n- #a\n\n> #a\n\n|#a|\n|-|\n|#a|\n\n[^1]\n\n[^1]: #a\n",
            "a"
        )
        .len(),
        7
    );
    for text in [
        "`#a`\n",
        "```\n#a\n```\n",
        "    #a\n",
        "%%#a%%\n",
        "%%\n#a\n%%\n",
        "<div>#a</div>\n",
        "[#a](u)\n",
        "[[#a]]\n",
        "![#a](u)\n",
        "<kbd>#a</kbd>\n",
        "$#a$\n",
        "---\ntags: #a\n---\n",
        "x#a\n",
        "#1\n",
        "#b\n",
        "\\#a\n",
    ] {
        assert_eq!(tags(text, "a"), Vec::<String>::new(), "{text:?}");
    }
}

#[test]
fn a_tag_is_placed_right_behind_what_only_the_source_spells_otherwise() {
    let text = "a &amp; #tag \\* #tag\n";
    let found = hits_in_text(text, &Query::Tag("tag".into()), Path::new("/n.md"), &tree());
    let starts: Vec<usize> = found.hits.iter().map(|hit| hit.range.start).collect();
    assert_eq!(starts, [8, 16]);
    assert_eq!(found.hits[0].range, 8..12);
    let text = "a &#35;x\n";
    let found = hits_in_text(text, &Query::Tag("x".into()), Path::new("/n.md"), &tree());
    assert_eq!(
        found.hits[0].range,
        0..text.len() - 1,
        "a tag spelled otherwise is told by the text it stands in"
    );
}

#[test]
fn links_to_a_note_are_found_as_the_tree_resolves_them() {
    let folder = Folder::new("links");
    let root = folder.path();
    folder.note("Target Note.md", "");
    folder.note(
        "from.md",
        "[[Target Note]] [[Target Note|T]] [[Target Note#H]] ![[Target Note]] [a](Target%20Note.md) [b](<Target Note.md#h>) [c][r] ![i](Target%20Note.md)\n`[[Target Note]]`\n\n[r]: Target%20Note.md\n",
    );
    folder.note("sub/below.md", "[up](../Target%20Note.md) [[Target Note]]\n");
    folder.note(
        "other.md",
        "[[Other]] [x](other.md) [y](https://a.de/Target%20Note.md)\n",
    );
    let tree = scan(root, &AtomicBool::new(false));
    let query = Query::LinksTo(root.join("Target Note.md"));
    let mut files = Vec::new();
    search(&tree, &query, &AtomicBool::new(false), &mut |hits| files.push(hits));
    let found: Vec<(&str, usize)> = files
        .iter()
        .map(|f| (f.path.file_name().unwrap().to_str().unwrap(), f.hits.len()))
        .collect();
    assert_eq!(found, [("below.md", 1), ("from.md", 9)]);
    let from = std::fs::read_to_string(root.join("from.md")).unwrap();
    assert_eq!(&from[files[1].hits[0].range.clone()], "[[Target Note]]");
    assert_eq!(&from[files[1].hits[8].range.clone()], "[r]: Target%20Note.md");
    assert_eq!(files[1].hits[8].line, 3);
}

#[test]
fn a_link_in_a_tag_search_or_the_other_way_round_is_not_read_without_a_reason() {
    let tree = tree();
    let links = Query::LinksTo(PathBuf::from("/n.md"));
    assert!(
        hits_in_text("no brackets", &links, Path::new("/m.md"), &tree)
            .hits
            .is_empty()
    );
    assert!(
        hits_in_text("no marks", &Query::Tag("a".into()), Path::new("/m.md"), &tree)
            .hits
            .is_empty()
    );
}
