//! Links, fields, tracked changes and the elements that only wrap what they hold.

use super::*;

#[test]
fn links_lead_where_their_relationship_or_bookmark_says() {
    let fixture = Fixture {
        rels: vec![
            ("rId1", "https://a.bc/d e", true),
            ("rId2", "javascript:alert(1)", true),
            ("rId3", "other.docx", false),
        ],
        ..Fixture::default()
    };
    let link = |attrs: &str, text: &str| format!("<w:p><w:hyperlink {attrs}>{}</w:hyperlink></w:p>", t(text));
    assert_eq!(
        fixture.markdown(&link("r:id=\"rId1\"", "a")),
        "[a](<https://a.bc/d e>)\n"
    );
    assert_eq!(
        fixture.markdown(&link("r:id=\"rId1\" w:anchor=\"b c\"", "a")),
        "[a](<https://a.bc/d e#bc>)\n"
    );
    assert_eq!(fixture.markdown(&link("w:anchor=\"_Ref1\"", "a")), "[a](#_Ref1)\n");
    assert_eq!(fixture.markdown(&link("r:id=\"rId2\"", "a")), "a\n");
    assert_eq!(fixture.markdown(&link("r:id=\"rId3\"", "a")), "[a](other.docx)\n");
    assert_eq!(fixture.markdown(&link("r:id=\"missing\"", "a")), "a\n");
    assert_eq!(fixture.markdown(&link("", "a")), "a\n");
    assert_eq!(
        fixture.markdown(&format!(
            "<w:p><w:hyperlink r:id=\"rId3\">{}{}</w:hyperlink></w:p>",
            marked("<w:b/>", "a"),
            t("b")
        )),
        "[**a**b](other.docx)\n",
        "the runs of a link are one link"
    );
}

#[test]
fn a_heading_with_a_bookmark_a_link_uses_takes_its_name() {
    let fixture = with_styles(&style("H", "heading 2", ""));
    assert_eq!(
        fixture.markdown(&format!(
            "<w:p><w:pPr><w:pStyle w:val=\"H\"/></w:pPr><w:bookmarkStart w:name=\"unused\"/><w:bookmarkStart w:name=\"ziel_1\"/>{}<w:bookmarkEnd/></w:p>\
             <w:p><w:hyperlink w:anchor=\"ziel_1\">{}</w:hyperlink></w:p>",
            t("Titel"),
            t("hin")
        )),
        "## Titel {#ziel_1}\n\n[hin](#ziel_1)\n"
    );
}

#[test]
fn fields_keep_their_result_and_drop_their_instruction() {
    let field = |instruction: &str, result: &str| {
        format!(
            "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText>{instruction}</w:instrText></w:r>\
             <w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>{}<w:r><w:fldChar w:fldCharType=\"end\"/></w:r>",
            t(result)
        )
    };
    assert_eq!(
        markdown(&p(&format!("{}{}", t("Seite "), field(" PAGE ", "3")))),
        "Seite 3\n"
    );
    assert_eq!(
        markdown(&p(&field("HYPERLINK \"https://a.bc\"", "hin"))),
        "[hin](https://a.bc)\n"
    );
    assert_eq!(
        markdown(&p(&field("HYPERLINK \\l \"_Ref1\"", "hin"))),
        "[hin](#_Ref1)\n"
    );
    assert_eq!(
        markdown(&format!(
            "<w:p><w:fldSimple w:instr=\" DATE \">{}</w:fldSimple></w:p>\
             <w:p><w:fldSimple w:instr=\"HYPERLINK &quot;x.md&quot;\">{}</w:fldSimple></w:p><w:p><w:fldSimple>{}</w:fldSimple></w:p>",
            t("heute"),
            t("hin"),
            t("frei")
        )),
        "heute\n\n[hin](x.md)\n\nfrei\n"
    );
    assert_eq!(
        markdown(&p(&format!(
            "<w:r><w:fldChar w:fldCharType=\"begin\"/><w:instrText>PAGE</w:instrText><w:fldChar w:fldCharType=\"end\"/></w:r>{}",
            t("danach")
        ))),
        "danach\n",
        "without a result, nothing stands there"
    );
    assert_eq!(
        markdown(&format!(
            "<w:p><w:r><w:fldChar w:fldCharType=\"separate\"/><w:fldChar w:fldCharType=\"end\"/><w:fldChar w:fldCharType=\"mystery\"/><w:instrText>a</w:instrText><w:fldChar/></w:r>{}</w:p>",
            t("x")
        )),
        "x\n",
        "field marks without a field mean nothing"
    );
    let in_a_link = format!(
        "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText>HYPERLINK \"a.md\"</w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>{}</w:p><w:p>{}<w:r><w:fldChar w:fldCharType=\"end\"/></w:r>{}</w:p>",
        field("PAGEREF x", "1"),
        t("2"),
        t("3")
    );
    assert_eq!(
        markdown(&in_a_link),
        "[1](a.md)\n\n[2](a.md)3\n",
        "a field goes on over paragraphs and holds others"
    );
}

#[test]
fn a_table_of_contents_is_left_out() {
    let fixture = with_styles(&format!(
        "{}{}{}",
        style("Verzeichnis1", "toc 1", ""),
        style("Inhalt", "TOC Heading", "<w:basedOn w:val=\"H\"/>"),
        style("H", "heading 1", ""),
    ));
    let entry = format!(
        "<w:p><w:pPr><w:pStyle w:val=\"Verzeichnis1\"/></w:pPr><w:hyperlink w:anchor=\"_Toc1\">{}<w:r><w:rPr><w:webHidden/></w:rPr><w:tab/></w:r>\
         <w:r><w:rPr><w:webHidden/></w:rPr><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:rPr><w:webHidden/></w:rPr><w:instrText>PAGEREF _Toc1</w:instrText></w:r>\
         <w:r><w:rPr><w:webHidden/></w:rPr><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:rPr><w:webHidden/></w:rPr><w:t>2</w:t></w:r>\
         <w:r><w:rPr><w:webHidden/></w:rPr><w:fldChar w:fldCharType=\"end\"/></w:r></w:hyperlink></w:p>",
        t("Kapitel")
    );
    assert_eq!(
        fixture.markdown(&format!("{}{entry}{}", p(&t("vor")), p(&t("nach")))),
        "vor\n\nnach\n"
    );
    assert_eq!(
        fixture.markdown(&styled("Verzeichnis1", &t("von Hand"))),
        "von Hand\n",
        "a paragraph in this style with text of its own stays"
    );
    let field = format!(
        "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText> TOC \\o \"1-3\" </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>{}</w:p><w:p>{}</w:p><w:p><w:r><w:fldChar w:fldCharType=\"end\"/></w:r>{}</w:p>",
        t("Eintrag 1"),
        t("Eintrag 2"),
        t("danach")
    );
    assert_eq!(markdown(&field), "danach\n");
}

#[test]
fn tracked_changes_show_what_is_there_now() {
    assert_eq!(
        markdown(&p(&format!(
            "{}<w:ins>{}</w:ins><w:del><w:r><w:delText>weg</w:delText></w:r>{}</w:del><w:moveFrom>{}</w:moveFrom><w:moveTo>{}</w:moveTo>",
            t("a "),
            t("b "),
            t("weg"),
            t("weg"),
            t("c")
        ))),
        "a b c\n"
    );
    assert_eq!(
        markdown(&format!(
            "<w:ins>{}</w:ins><w:del>{}</w:del><w:moveFrom>{}</w:moveFrom><w:moveTo>{}</w:moveTo>",
            p(&t("neu")),
            p(&t("weg")),
            p(&t("weg")),
            p(&t("hier"))
        )),
        "neu\n\nhier\n"
    );
}

#[test]
fn wrappers_keep_what_they_hold() {
    assert_eq!(
        markdown(&format!(
            "<w:sdt><w:sdtPr><w:alias w:val=\"nein\"/></w:sdtPr><w:sdtContent>{}</w:sdtContent></w:sdt><w:customXml>{}</w:customXml><w:smartTag>{}</w:smartTag>",
            p(&t("a")),
            p(&t("b")),
            p(&t("c"))
        )),
        "a\n\nb\n\nc\n"
    );
    assert_eq!(
        markdown(&p(&format!(
            "<w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt><w:smartTag>{}</w:smartTag><w:customXml>{}</w:customXml><w:dir>{}</w:dir><w:bdo>{}</w:bdo>",
            t("a"),
            t("b"),
            t("c"),
            t("d"),
            t("e")
        ))),
        "abcde\n"
    );
}

#[test]
fn alternate_content_is_taken_once() {
    let alternate = |choice: &str, fallback: &str| {
        format!(
            "<mc:AlternateContent><mc:Choice Requires=\"x\">{choice}</mc:Choice><mc:Fallback>{fallback}</mc:Fallback></mc:AlternateContent>"
        )
    };
    assert_eq!(markdown(&alternate(&p(&t("a")), &p(&t("b")))), "a\n");
    assert_eq!(markdown(&alternate("", &p(&t("b")))), "b\n");
    assert_eq!(markdown(&p(&alternate(&t("a"), &t("b")))), "a\n");
    assert_eq!(markdown(&p(&alternate("", &t("b")))), "b\n");
    assert_eq!(
        markdown(&p(&format!("<w:r>{}</w:r>", alternate("<w:t>a</w:t>", "<w:t>b</w:t>")))),
        "a\n"
    );
    assert_eq!(markdown("<mc:AlternateContent/>"), "");
}
