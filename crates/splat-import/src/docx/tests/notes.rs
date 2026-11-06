//! Footnotes and endnotes: Word's own and those Splat writes into the text.

use super::*;

#[test]
fn footnotes_and_endnotes_go_to_the_end_in_the_order_they_are_used() {
    let fixture = Fixture {
        footnotes: Some(format!(
            "<w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>\
             <w:footnote w:id=\"2\"><w:p><w:r><w:footnoteRef/></w:r>{}<w:r><w:footnoteReference w:id=\"3\"/></w:r></w:p></w:footnote>\
             <w:footnote w:id=\"3\"><w:p>{}</w:p><w:p>{}<w:r><w:footnoteReference w:id=\"3\"/></w:r></w:p></w:footnote><w:footnote>{}</w:footnote>",
            t(" Fuß"),
            t("zwei"),
            t("Absätze"),
            p(&t("ohne Nummer"))
        )),
        endnotes: Some(format!("<w:endnote w:id=\"2\"><w:p>{}</w:p></w:endnote>", t("Schluss"))),
        ..Fixture::default()
    };
    let foot = |id: u32| {
        format!("<w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:footnoteReference w:id=\"{id}\"/></w:r>")
    };
    let end = |id: u32| format!("<w:r><w:endnoteReference w:id=\"{id}\"/></w:r>");
    assert_eq!(
        fixture.markdown(&p(&format!(
            "{}{}{}{}{}{}{}",
            t("a"),
            foot(2),
            t(" b"),
            end(2),
            t(" c"),
            foot(2),
            foot(99)
        ))),
        "a[^1] b[^2] c[^1]\n\n[^1]: Fuß[^3]\n\n[^2]: Schluss\n\n[^3]: zwei\n\n    Absätze[^3]\n"
    );
    assert_eq!(markdown(&p(&format!("{}{}", t("a"), foot(1)))), "a\n", "no notes part");
    assert_eq!(
        markdown(&p(
            "<w:r><w:rPr><w:vanish/></w:rPr><w:footnoteReference w:id=\"1\"/></w:r>"
        )),
        ""
    );
}

#[test]
fn a_note_has_relationships_of_its_own() {
    let fixture = Fixture {
        footnotes: Some(format!(
            "<w:footnote w:id=\"1\"><w:p><w:hyperlink r:id=\"rId1\">{}</w:hyperlink><w:r><w:drawing><a:blip r:embed=\"rId2\"/></w:drawing></w:r></w:p></w:footnote>",
            t("hin")
        )),
        parts: vec![
            (
                "word/_rels/footnotes.xml.rels",
                "<Relationships><Relationship Id=\"rId1\" Target=\"https://a.bc\" TargetMode=\"External\"/><Relationship Id=\"rId2\" Target=\"media/a.png\"/></Relationships>"
                    .as_bytes()
                    .to_vec(),
            ),
            ("word/media/a.png", PNG.to_vec()),
        ],
        rels: vec![("rId1", "https://falsch.de", true)],
        ..Fixture::default()
    };
    let body = format!(
        "<w:p><w:hyperlink r:id=\"rId1\">{}</w:hyperlink><w:r><w:footnoteReference w:id=\"1\"/></w:r></w:p>",
        t("a")
    );
    let imported = import(&fixture.file(&body), "b").unwrap();
    assert_eq!(
        imported.markdown,
        "[a](https://falsch.de)[^1]\n\n[^1]: [hin](https://a.bc)![](b/image1.png)\n"
    );
    assert_eq!(imported.media.len(), 1);
}

#[test]
fn the_notes_splat_writes_become_notes_again() {
    let fixture = with_styles(&style("FootnoteText", "footnote text", ""));
    let rule = "<w:p><w:pPr><w:pBdr><w:top w:val=\"single\"/></w:pBdr></w:pPr></w:p>";
    let reference = marked("<w:vertAlign w:val=\"superscript\"/>", "1");
    assert_eq!(
        fixture.markdown(&format!(
            "<w:p>{}{reference}</w:p>{rule}{}{}<w:p><w:pPr><w:numPr><w:numId w:val=\"7\"/></w:numPr></w:pPr>{}</w:p>{}",
            t("a"),
            styled("FootnoteText", &format!("{}{}", t("1. "), t("Fuß"))),
            styled("FootnoteText", &t("weiter")),
            t("Punkt"),
            "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>"
        )),
        "a[^1]\n\n[^1]: Fuß\n\n    weiter\n\n    - Punkt\n\n    | A | |\n    |---|---|\n"
    );
    assert_eq!(
        fixture.markdown(&format!(
            "<w:p>{}{reference}</w:p>{rule}{}{}",
            t("a"),
            styled("FootnoteText", &t("1. Fuß")),
            styled("FootnoteText", &t("2. Unbenutzt")),
        )),
        "a[^1]\n\n2\\. Unbenutzt\n\n[^1]: Fuß\n",
        "a note nothing refers to stays text"
    );
    assert_eq!(
        fixture.markdown(&format!(
            "<w:p>{}{reference}</w:p>{rule}{}{}{}",
            t("a"),
            styled("FootnoteText", &t("1. Fuß")),
            styled("FootnoteText", &t("2.5 mal")),
            styled("FootnoteText", &t("2.")),
        )),
        "a[^1]\n\n2\\.\n\n[^1]: Fuß\n\n    2.5 mal\n",
        "only a number with a space or nothing behind it starts a note"
    );
    assert_eq!(
        fixture.markdown(&format!(
            "<w:p>{}{reference}</w:p>{rule}{}<w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>{}{}",
            t("a"),
            styled("FootnoteText", &t("1. ")),
            styled("FootnoteText", &t("2. ")),
            "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>",
        )),
        "a[^1]\n\n2\\.\n\n| B | |\n|---|---|\n\n[^1]: | A | |\n    |---|---|\n",
        "a number can stand alone above what a note holds"
    );
    assert_eq!(
        fixture.markdown(&format!("{}{}", styled("FootnoteText", &t("1. Fuß")), p(&reference))),
        "1\\. Fuß\n\n<sup>1</sup>\n",
        "without the line above them, this is only text"
    );
    assert_eq!(
        fixture.markdown(&format!("{rule}{}", styled("FootnoteText", &marked("<w:b/>", "Fuß")),)),
        "**Fuß**\n"
    );
    assert_eq!(
        fixture.markdown(&format!(
            "{rule}{}",
            styled("FootnoteText", &marked("<w:rFonts w:ascii=\"Menlo\"/>", "x")),
        )),
        "```\nx\n```\n",
        "a note starts with a number in plain text"
    );
    assert_eq!(
        fixture.markdown(&format!(
            "{rule}{}{}",
            p(&t("zwischen")),
            styled("FootnoteText", &t("1. Fuß")),
        )),
        "zwischen\n\n1\\. Fuß\n",
    );
    assert_eq!(
        fixture.markdown(&format!(
            "{rule}<w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>{}",
            styled("FootnoteText", &t("1. Fuß")),
        )),
        "| A | |\n|---|---|\n\n1\\. Fuß\n",
    );
}
