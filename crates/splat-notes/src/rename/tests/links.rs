//! Which part of which link a rename changes.

use super::*;

#[test]
fn in_a_wiki_link_only_the_name_changes() {
    for (old, new) in [
        ("[[Old]]", "[[New]]"),
        ("[[Old|T]]", "[[New|T]]"),
        ("[[Old#H]]", "[[New#H]]"),
        ("[[Old#H|T]]", "[[New#H|T]]"),
        ("[[Old^b]]", "[[New^b]]"),
        ("![[Old]]", "![[New]]"),
        ("[[ Old ]]", "[[ New ]]"),
        ("[[old]]", "[[New]]"),
        ("[[Old.md]]", "[[New.md]]"),
        ("[[Old.MD]]", "[[New.MD]]"),
        ("[[Old.md#H|T]]", "[[New.md#H|T]]"),
    ] {
        assert_eq!(after(old), new, "{old}");
    }
}

#[test]
fn in_an_address_only_the_file_name_changes() {
    for (old, new) in [
        ("[t](Old.md)", "[t](New.md)"),
        ("[t](Old.md#h \"x\")", "[t](New.md#h \"x\")"),
        ("[t](./Old.md?a=b#h)", "[t](./New.md?a=b#h)"),
        ("[t](<Old.md> 'x')", "[t](<New.md> 'x')"),
        ("[t](< Old.md >)", "[t](< New.md >)"),
        ("![i](Old.md)", "![i](New.md)"),
        ("[t](old.MD)", "[t](New.md)"),
        ("[t](sub/../Old.md)", "[t](sub/../New.md)"),
        ("[t](file:///v/Old.md#h)", "[t](file:///v/New.md#h)"),
        ("[t](/v/Old.md)", "[t](/v/New.md)"),
        ("[t](sub%2F..%2FOld.md)", "[t](sub%2F..%2FNew.md)"),
        ("[t][r]\n\n[r]: Old.md\n", "[t][r]\n\n[r]: New.md\n"),
        ("[t][r]\n\n[r]: Old.md \"x\"\n", "[t][r]\n\n[r]: New.md \"x\"\n"),
        ("[t][r]\n\n[r]: <Old.md>\n", "[t][r]\n\n[r]: <New.md>\n"),
        (
            "[t][r]\n\n> [r]:\n>   ./Old.md#h\n",
            "[t][r]\n\n> [r]:\n>   ./New.md#h\n",
        ),
    ] {
        assert_eq!(after(old), new, "{old}");
    }
}

#[test]
fn a_rename_in_a_real_folder_reaches_every_kind_of_link() {
    let folder = Folder::new("real");
    let root = folder.path();
    folder.note(
        "Alte Notiz.md",
        "# Kopf\n\n[[Alte Notiz#Kopf]] [hier](Alte%20Notiz.md#kopf) [dort](#kopf)\n",
    );
    folder.note(
        "start.md",
        "\u{FEFF}[[Alte Notiz]] ![[Alte Notiz]] [[Alte Notiz.md#h|t]] `[[Alte Notiz]]`\r\n\r\n[a](Alte%20Notiz.md) \
         [b](<Alte Notiz.md#h> \"t\") [c][r] <a href=\"Alte%20Notiz.md\">d</a>\r\n\r\n[r]: Alte%20Notiz.md\r\n",
    );
    folder.note("sub/unten.md", "[hoch](../Alte%20Notiz.md) [[Alte Notiz]]\n");
    folder.note("sub/Alte Notiz.md", "Eine andere.\n");
    folder.note("frei/oben.md", "[[Alte Notiz]] [zwei](../sub/Alte%20Notiz.md)\n");
    let tree = scan(root, &AtomicBool::new(false));
    let rewritten = |to: &str| {
        let (from, to) = (root.join("Alte Notiz.md"), root.join(to));
        let mut read = |path: &Path| std::fs::read_to_string(path).ok();
        let rewrites = renames(&tree, &from, &to, &AtomicBool::new(false), &mut read);
        rewrites
            .iter()
            .map(|rewrite| {
                let name = rewrite.path.strip_prefix(root).unwrap().display().to_string();
                let old = std::fs::read_to_string(&rewrite.path).unwrap();
                (name, applied(&old, &rewrite.replacements))
            })
            .collect::<HashMap<String, String>>()
    };
    let plain = rewritten("Neue Größe 100%.md");
    assert_eq!(plain.len(), 3, "{plain:?}");
    assert_eq!(
        plain["Alte Notiz.md"],
        "# Kopf\n\n[[Neue Größe 100%#Kopf]] [hier](Neue%20Größe%20100%25.md#kopf) [dort](#kopf)\n"
    );
    assert_eq!(
        plain["start.md"],
        "\u{FEFF}[[Neue Größe 100%]] ![[Neue Größe 100%]] [[Neue Größe 100%.md#h|t]] `[[Alte Notiz]]`\r\n\r\n\
         [a](Neue%20Größe%20100%25.md) [b](<Neue Größe 100%25.md#h> \"t\") [c][r] \
         <a href=\"Alte%20Notiz.md\">d</a>\r\n\r\n[r]: Neue%20Größe%20100%25.md\r\n"
    );
    assert_eq!(
        plain["sub/unten.md"],
        "[hoch](../Neue%20Größe%20100%25.md) [[Alte Notiz]]\n"
    );
    let hash = rewritten("C# Notiz.md");
    assert_eq!(
        hash["sub/unten.md"], "[hoch](../C%23%20Notiz.md) [[Alte Notiz]]\n",
        "a wiki link to the nearer note stays"
    );
    assert!(
        hash["start.md"].contains("[[Alte Notiz]] ![[Alte Notiz]]"),
        "it cannot hold the name"
    );
    assert!(hash["start.md"].contains("[a](C%23%20Notiz.md)"));
}

#[test]
fn links_are_found_in_tables_notes_headings_and_callouts() {
    for (old, new) in [
        ("|a|\n|-|\n|[[Old\\|T]]|\n", "|a|\n|-|\n|[[New\\|T]]|\n"),
        ("|a|\n|-|\n|[t](Old.md)|\n", "|a|\n|-|\n|[t](New.md)|\n"),
        (
            "a^[see [[Old]] and [t](Old.md)] b\n",
            "a^[see [[New]] and [t](New.md)] b\n",
        ),
        ("a[^1]\n\n[^1]: [[Old]]\n", "a[^1]\n\n[^1]: [[New]]\n"),
        ("# [[Old]] [t](Old.md)\n", "# [[New]] [t](New.md)\n"),
        ("> [!note]\n> [[Old]]\n", "> [!note]\n> [[New]]\n"),
        ("- a\n  - [t](Old.md)\n", "- a\n  - [t](New.md)\n"),
        ("**[[Old]]** *[t](Old.md)*\n", "**[[New]]** *[t](New.md)*\n"),
    ] {
        assert_eq!(after(old), new, "{old:?}");
    }
}

#[test]
fn a_link_on_an_indented_line_of_a_windows_document_is_found() {
    assert_eq!(after("a\r\n [t](Old.md) b\r\n"), "a\r\n [t](New.md) b\r\n");
}

#[test]
fn links_inside_a_pictures_text_and_its_own_address_come_in_the_order_of_the_text() {
    assert_eq!(after("![a [t](Old.md) b](Old.md)\n"), "![a [t](New.md) b](New.md)\n");
}

#[test]
fn what_a_replacement_covers_is_only_the_name() {
    let tree = tree_of(&["from.md", "Old.md"]);
    let text = "[[Old.md#H|T]] [t](Old.md#h \"x\")";
    let rewrites = renames(
        &tree,
        Path::new("/v/Old.md"),
        Path::new("/v/New.md"),
        &AtomicBool::new(false),
        &mut |path| (path == Path::new("/v/from.md")).then(|| text.to_string()),
    );
    let covered: Vec<(&str, &str)> = rewrites[0]
        .replacements
        .iter()
        .map(|r| (&text[r.range.clone()], r.text.as_str()))
        .collect();
    assert_eq!(covered, [("Old", "New"), ("Old.md", "New.md")]);
}

#[test]
fn a_path_stays_in_a_link() {
    let tree = tree_of(&["from.md", "dir/Old.md", "dir/from.md", "dir/in/x.md"]);
    let rename = |file: &str, text: &str| renamed_in(&tree, file, text, "/v/dir/Old.md", "/v/dir/New.md");
    assert_eq!(rename("/v/from.md", "[[dir/Old]]"), "[[dir/New]]");
    assert_eq!(rename("/v/from.md", "[[dir/Old.md#h|t]]"), "[[dir/New.md#h|t]]");
    assert_eq!(rename("/v/from.md", "[[Old]]"), "[[New]]");
    assert_eq!(rename("/v/from.md", "[[elsewhere/Old]]"), "[[elsewhere/New]]");
    assert_eq!(
        rename("/v/from.md", "[t](dir/Old.md#h \"x\")"),
        "[t](dir/New.md#h \"x\")"
    );
    assert_eq!(rename("/v/dir/in/x.md", "[t](../Old.md)"), "[t](../New.md)");
    assert_eq!(
        rename("/v/dir/in/x.md", "[r]\n\n[r]: ../Old.md\n"),
        "[r]\n\n[r]: ../New.md\n"
    );
    assert_eq!(
        rename("/v/dir/in/x.md", "[[Old]] [[../Old]]"),
        "[[Old]] [[../Old]]",
        "wiki links look downwards only"
    );
}

#[test]
fn what_is_no_link_stays() {
    for text in [
        "`[[Old]]` `[t](Old.md)`\n",
        "```\n[[Old]]\n[r]: Old.md\n```\n",
        "    [[Old]]\n",
        "%%[[Old]] [t](Old.md)%%\n",
        "%%\n[[Old]]\n[r]: Old.md\n%%\n",
        "<a href=\"Old.md\">x</a>\n",
        "<div>\n<a href=\"Old.md\">x</a>\n</div>\n",
        "---\nlink: \"[[Old]]\"\n---\n",
        "\\[\\[Old]]\n",
        "Old.md Old\n",
        "[t][r] without its definition\n",
        "[t](https://a.de/Old.md)\n",
        "[t](#Old)\n",
        "[t](Older.md) [[Older]]\n",
    ] {
        assert_eq!(after(text), text, "{text:?}");
    }
}

#[test]
fn a_note_links_to_itself_under_its_new_name() {
    let tree = tree_of(&["Old.md"]);
    let text = "[[Old]] [t](Old.md#h) ![[Old#h]] [u](#h)\n";
    assert_eq!(
        renamed_in(&tree, "/v/Old.md", text, "/v/Old.md", "/v/New.md"),
        "[[New]] [t](New.md#h) ![[New#h]] [u](#h)\n"
    );
}

#[test]
fn every_byte_but_the_names_stays() {
    let text = "\u{FEFF}---\r\nt: 1\r\n---\r\n\r\n# Überschrift  \r\n\r\n[[Old]]  \r\n[t](Old.md)\t\r\r\n\u{0}x\r\n\r\n[r]: Old.md\r\n";
    let wanted = text.replace("[[Old]]", "[[New]]").replace("Old.md", "New.md");
    assert_eq!(after(text), wanted);
}
