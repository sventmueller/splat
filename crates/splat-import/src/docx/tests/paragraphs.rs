//! What a paragraph is by its style: a heading, code, a quote or a rule.

use super::*;

#[test]
fn headings_are_known_by_the_name_of_their_style() {
    let fixture = with_styles(&format!(
        "{}{}{}{}{}{}",
        style("berschrift1", "heading 1", "<w:rPr><w:b/></w:rPr>"),
        style("berschrift2", "Heading 2", ""),
        style("Mine", "Meine", "<w:basedOn w:val=\"berschrift2\"/>"),
        style("Outline", "Gliederung", "<w:pPr><w:outlineLvl w:val=\"3\"/></w:pPr>"),
        style("Heading6", "heading 6", "<w:rPr><w:b/><w:i/></w:rPr>"),
        style("Text", "Text", "<w:pPr><w:outlineLvl w:val=\"9\"/></w:pPr>"),
    ));
    assert_eq!(fixture.markdown(&styled("berschrift1", &t("Titel"))), "# Titel\n");
    assert_eq!(fixture.markdown(&styled("berschrift2", &t("Titel"))), "## Titel\n");
    assert_eq!(fixture.markdown(&styled("Mine", &t("Titel"))), "## Titel\n");
    assert_eq!(fixture.markdown(&styled("Outline", &t("Titel"))), "#### Titel\n");
    assert_eq!(fixture.markdown(&styled("Heading6", &t("Titel"))), "###### Titel\n");
    assert_eq!(fixture.markdown(&styled("Text", &t("Titel"))), "Titel\n");
    assert_eq!(
        fixture.markdown(&styled("berschrift1", &format!("{}{}", t("a "), marked("<w:i/>", "b")))),
        "# a *b*\n"
    );
    assert_eq!(fixture.markdown(&styled("berschrift1", "")), "");
    assert_eq!(
        markdown("<w:p><w:pPr><w:outlineLvl w:val=\"1\"/></w:pPr><w:r><w:t>direct</w:t></w:r></w:p>"),
        "## direct\n"
    );
    assert_eq!(
        fixture.markdown("<w:p><w:pPr><w:pStyle w:val=\"berschrift1\"/><w:outlineLvl w:val=\"9\"/></w:pPr><w:r><w:t>body</w:t></w:r></w:p>"),
        "**body**\n",
        "no heading, so what its style makes bold shows"
    );
}

#[test]
fn a_title_is_a_heading_only_if_the_document_has_no_first_heading() {
    let fixture = with_styles(&format!(
        "{}{}{}",
        style("Titel", "Title", "<w:rPr><w:b/></w:rPr>"),
        style("Untertitel", "Subtitle", "<w:rPr><w:i/></w:rPr>"),
        style("H1", "heading 1", ""),
    ));
    let title = format!("{}{}", styled("Titel", &t("T")), styled("Untertitel", &t("U")));
    assert_eq!(fixture.markdown(&title), "# T\n\n## U\n");
    assert_eq!(
        fixture.markdown(&format!("{title}{}", styled("H1", &t("A")))),
        "T\n\nU\n\n# A\n"
    );
}

#[test]
fn code_paragraphs_join_into_a_fenced_block() {
    let fixture = with_styles(&format!(
        "{}{}{}",
        style("Quelltext", "Code", ""),
        style("Pre", "HTML Preformatted", ""),
        style("Mine", "Eigen", "<w:basedOn w:val=\"Quelltext\"/>"),
    ));
    assert_eq!(
        fixture.markdown(&format!(
            "{}{}{}{}",
            styled("Quelltext", &t("fn a() {")),
            styled("Quelltext", ""),
            styled("Pre", &t("  x")),
            styled("Mine", &format!("{}<w:r><w:br/></w:r>{}", t("}"), t("y")))
        )),
        "```\nfn a() {\n\n  x\n}\ny\n```\n"
    );
    let mono = |text: &str| p(&marked("<w:rFonts w:ascii=\"Courier New\"/>", text));
    assert_eq!(markdown(&format!("{}{}", mono("a"), mono("b"))), "```\na\nb\n```\n");
    assert_eq!(
        markdown(&format!(
            "{}{}",
            mono("a"),
            p(&format!("{}{}", marked("<w:rFonts w:ascii=\"Menlo\"/>", "b"), t(" c")))
        )),
        "```\na\n```\n\n`b` c\n"
    );
    assert_eq!(
        markdown(&format!(
            "<w:p><w:hyperlink w:anchor=\"x\">{}</w:hyperlink></w:p>",
            marked("<w:rFonts w:ascii=\"Menlo\"/>", "a")
        )),
        "[`a`](#x)\n",
        "a link is no code"
    );
    assert_eq!(
        markdown(&p(&format!(
            "{}{}",
            marked("<w:rFonts w:ascii=\"Menlo\"/>", "a"),
            "<w:r><w:drawing/></w:r>"
        ))),
        "```\na\n```\n"
    );
    assert_eq!(
        markdown(&p(&format!(
            "{}<m:oMath>{}</m:oMath>",
            marked("<w:rFonts w:ascii=\"Menlo\"/>", "a"),
            "<m:r><m:t>x</m:t></m:r>"
        ))),
        "`a`$x$\n"
    );
}

#[test]
fn a_code_paragraph_holds_the_text_of_what_stands_in_it() {
    let fixture = Fixture {
        styles: Some(style("C", "Code", "")),
        footnotes: Some(format!("<w:footnote w:id=\"1\">{}</w:footnote>", p(&t("Fuß")))),
        rels: vec![("rId1", "a.png", false)],
        parts: vec![("word/a.png", PNG.to_vec())],
        ..Fixture::default()
    };
    assert_eq!(
        fixture.markdown(&styled(
            "C",
            &format!(
                "{}<w:r><w:drawing><wp:docPr descr=\"Bild\"/><a:blip r:embed=\"rId1\"/></w:drawing></w:r><w:r><w:footnoteReference w:id=\"1\"/></w:r>",
                t("a ")
            )
        )),
        "```\na Bild\n```\n"
    );
}

#[test]
fn a_monospace_default_style_makes_no_code() {
    let fixture = with_styles(
        "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Standard\"><w:name w:val=\"Normal\"/><w:rPr><w:rFonts w:ascii=\"Courier New\"/></w:rPr></w:style>",
    );
    assert_eq!(fixture.markdown(&p(&t("Szene 1"))), "Szene 1\n");
}

#[test]
fn quotes_join_and_hold_their_lists() {
    let fixture = with_styles(&format!(
        "{}{}",
        style("Zitat", "Quote", "<w:rPr><w:i/></w:rPr>"),
        style("Betont", "Intense Quote", ""),
    ));
    assert_eq!(
        fixture.markdown(&format!(
            "{}{}{}",
            styled("Zitat", &t("a")),
            styled("Betont", &t("b")),
            p(&t("c"))
        )),
        "> a\n>\n> b\n\nc\n"
    );
    assert_eq!(
        fixture.markdown(&format!(
            "{}<w:p><w:pPr><w:pStyle w:val=\"Zitat\"/><w:numPr><w:numId w:val=\"9\"/></w:numPr></w:pPr>{}</w:p>{}",
            styled("Zitat", &t("a")),
            t("b"),
            "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>"
        )),
        "> a\n>\n> - b\n\n| 1 | |\n|---|---|\n"
    );
    assert_eq!(fixture.markdown(&styled("Zitat", "")), "");
}

#[test]
fn a_line_below_an_empty_paragraph_is_a_rule() {
    let line = "<w:p><w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\"/></w:pBdr></w:pPr></w:p>";
    assert_eq!(
        markdown(&format!("{}{line}{}", p(&t("a")), p(&t("b")))),
        "a\n\n---\n\nb\n"
    );
    let none = "<w:p><w:pPr><w:pBdr><w:bottom w:val=\"none\"/></w:pBdr></w:pPr></w:p>";
    assert_eq!(markdown(none), "");
    let with_text = "<w:p><w:pPr><w:pBdr><w:bottom w:val=\"single\"/></w:pBdr></w:pPr><w:r><w:t>a</w:t></w:r></w:p>";
    assert_eq!(markdown(with_text), "a\n");
    let shape = "<w:p><w:r><w:pict><v:rect xmlns:o=\"urn:schemas-microsoft-com:office:office\" o:hr=\"t\"/></w:pict></w:r></w:p>";
    assert_eq!(
        markdown(&format!("{}{shape}", p(&t("a")))),
        "a\n\n---\n",
        "the shape Word draws a line with"
    );
    let other_shape = "<w:p><w:r><w:pict><v:rect/></w:pict></w:r></w:p>";
    assert_eq!(markdown(other_shape), "");
}

#[test]
fn a_text_box_stands_before_its_paragraph() {
    assert_eq!(
        markdown(&p(&format!(
            "{}<w:r><w:drawing><wps:txbx xmlns:wps=\"urn:wps\"><w:txbxContent>{}{}</w:txbxContent></wps:txbx></w:drawing></w:r>",
            t("im Absatz"),
            p(&t("Kasten 1")),
            p(&t("Kasten 2"))
        ))),
        "Kasten 1\n\nKasten 2\n\nim Absatz\n"
    );
}
