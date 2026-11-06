//! The styles and lists of the text among those of the template.

use super::*;

#[test]
fn localized_style_ids_are_found_by_name() {
    let result = apply_template(
        &export("# Titel\n\n## Zwei\n\n[Link](https://a.bc)\n\n> Zitat\n"),
        &full_template(),
    )
    .unwrap();
    assert_valid(&result);
    let document = text_of_part(&result, "word/document.xml");
    assert!(document.contains("<w:pStyle w:val=\"berschrift1\"/>"), "{document}");
    assert!(!document.contains("\"Heading1\""), "{document}");
    assert!(document.contains("<w:rStyle w:val=\"Link\"/>"), "{document}");
    let styles = text_of_part(&result, "word/styles.xml");
    assert!(!styles.contains("w:styleId=\"Heading1\""), "{styles}");
    assert!(!styles.contains("w:val=\"Hyperlink\"/></w:style><w:style w:type=\"character\" w:styleId=\"Hyperlink\""));
    assert!(
        styles.contains("w:styleId=\"Heading2\""),
        "a style it lacks is added: {styles}"
    );
    assert!(
        styles.contains("w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Standard\"/><w:next w:val=\"Standard\"/>"),
        "what it depends on reads in the template's terms: {styles}"
    );
    assert!(styles.contains("w:styleId=\"Quote\""), "{styles}");
    assert!(!styles.contains("w:styleId=\"Normal\""), "{styles}");
}

#[test]
fn an_id_that_means_something_else_in_the_template_gives_way() {
    let result = apply_template(&export("```\na\n```\n"), &full_template()).unwrap();
    assert_valid(&result);
    let styles = text_of_part(&result, "word/styles.xml");
    assert!(
        styles.contains("w:styleId=\"Code\"><w:name w:val=\"Quellcode\"/>"),
        "{styles}"
    );
    assert!(
        styles.contains("w:styleId=\"Code1\"><w:name w:val=\"Code\"/>"),
        "{styles}"
    );
    let document = text_of_part(&result, "word/document.xml");
    assert!(document.contains("<w:pStyle w:val=\"Code1\"/>"), "{document}");
}

#[test]
fn a_default_style_that_has_to_be_added_is_no_second_default() {
    let template =
        format!("<w:styles {W}><w:style w:default=\"1\" w:styleId=\"X\"><w:name w:val=\"x\"/></w:style></w:styles>");
    let generated = replaced(
        &export("a\n"),
        "word/document.xml",
        "<w:p>",
        "<w:p><w:pPr><w:pStyle w:val=\"Normal\"/></w:pPr>",
    );
    let mut parts = template_parts(DOCUMENT_TYPE);
    set_part(&mut parts, "word/styles.xml", &template);
    let result = apply_template(&generated, &archive(&parts)).unwrap();
    assert_valid(&result);
    let styles = text_of_part(&result, "word/styles.xml");
    assert_eq!(styles.matches("w:default=\"1\"").count(), 1, "{styles}");
    assert!(styles.contains("w:styleId=\"Normal\""), "{styles}");
}

#[test]
fn lists_get_numbers_of_their_own_behind_those_of_the_template() {
    let result = apply_template(&export("- a\n\n1. b\n\nx\n\n5. c\n"), &full_template()).unwrap();
    assert_valid(&result);
    let numbering = text_of_part(&result, "word/numbering.xml");
    assert!(numbering.contains("<w:abstractNum w:abstractNumId=\"4\"><w:lvl w:ilvl=\"0\"/></w:abstractNum>"));
    assert!(
        numbering.contains("w:abstractNumId=\"5\"") && numbering.contains("w:abstractNumId=\"6\""),
        "{numbering}"
    );
    assert!(
        numbering.contains("<w:num w:numId=\"8\"><w:abstractNumId w:val=\"5\"/></w:num>"),
        "{numbering}"
    );
    assert!(numbering.contains("<w:startOverride w:val=\"5\"/>"), "{numbering}");
    let abstract_at = numbering.rfind("<w:abstractNum ").unwrap();
    let number_at = numbering.find("<w:num ").unwrap();
    assert!(abstract_at < number_at, "{numbering}");
    let document = text_of_part(&result, "word/document.xml");
    assert!(document.contains("<w:numId w:val=\"8\"/>"), "{document}");
    assert!(document.contains("<w:numId w:val=\"10\"/>"), "{document}");
    assert!(!document.contains("<w:numId w:val=\"1\"/>"), "{document}");
}

#[test]
fn a_text_without_lists_leaves_the_numbering_alone() {
    let template = full_template();
    let result = apply_template(&export("a\n"), &template).unwrap();
    assert_eq!(
        text_of_part(&result, "word/numbering.xml"),
        text_of_part(&template, "word/numbering.xml")
    );
    assert_valid(&result);
}

#[test]
fn numbers_in_a_style_that_is_added_are_renumbered_too() {
    let generated = replaced(
        &replaced(
            &export("a\n"),
            "word/document.xml",
            "<w:p>",
            "<w:p><w:pPr><w:pStyle w:val=\"Eigen\"/></w:pPr>",
        ),
        "word/styles.xml",
        "</w:styles>",
        "<w:style w:styleId=\"Eigen\"><w:name w:val=\"Eigen\"/><w:pPr><w:numPr><w:numId w:val=\"1\"/></w:numPr></w:pPr></w:style></w:styles>",
    );
    let result = apply_template(&generated, &full_template()).unwrap();
    assert_valid(&result);
    assert!(text_of_part(&result, "word/styles.xml").contains("<w:numId w:val=\"8\"/>"));
    assert!(text_of_part(&result, "word/numbering.xml").contains("<w:num w:numId=\"8\">"));
}

#[test]
fn a_template_without_styles_takes_those_of_the_text() {
    let mut parts = template_parts(DOCUMENT_TYPE);
    without_part(&mut parts, "word/styles.xml");
    set_part(
        &mut parts,
        "word/_rels/document.xml.rels",
        &template_rels().replace(
            &format!("<Relationship Id=\"rId1\" Type=\"{REL_BASE}/styles\" Target=\"styles.xml\"/>"),
            "",
        ),
    );
    set_part(
        &mut parts,
        "[Content_Types].xml",
        &template_types(DOCUMENT_TYPE).replace(
            &format!("<Override PartName=\"/word/styles.xml\" ContentType=\"{OFFICE}.styles+xml\"/>"),
            "",
        ),
    );
    let generated = export(SAMPLE);
    let result = apply_template(&generated, &archive(&parts)).unwrap();
    assert_valid(&result);
    assert_same_text_as(&result, &generated);
    let document = text_of_part(&result, "word/document.xml");
    assert!(document.contains("<w:pStyle w:val=\"Heading1\"/>"), "{document}");
    let styles = text_of_part(&result, "word/styles.xml");
    assert!(
        styles.starts_with("<?xml") && styles.contains("<w:styles xmlns:w="),
        "{styles}"
    );
    assert!(
        styles.contains("w:default=\"1\""),
        "the only default style stays one: {styles}"
    );
    let rels = text_of_part(&result, "word/_rels/document.xml.rels");
    assert!(
        rels.contains(&format!("Type=\"{REL_BASE}/styles\" Target=\"styles.xml\"")),
        "{rels}"
    );
    let types = text_of_part(&result, "[Content_Types].xml");
    assert!(
        types.contains(&format!(
            "<Override PartName=\"/word/styles.xml\" ContentType=\"{OFFICE}.styles+xml\"/>"
        )),
        "{types}"
    );
}

#[test]
fn a_template_without_numbering_takes_that_of_the_text_and_keeps_its_numbers() {
    let mut parts = template_parts(DOCUMENT_TYPE);
    without_part(&mut parts, "word/numbering.xml");
    set_part(
        &mut parts,
        "word/_rels/document.xml.rels",
        &template_rels().replace(
            &format!("<Relationship Id=\"rId2\" Type=\"{REL_BASE}/numbering\" Target=\"numbering.xml\"/>"),
            "",
        ),
    );
    set_part(
        &mut parts,
        "[Content_Types].xml",
        &template_types(DOCUMENT_TYPE).replace(
            &format!("<Override PartName=\"/word/numbering.xml\" ContentType=\"{OFFICE}.numbering+xml\"/>"),
            "",
        ),
    );
    let generated = export("- a\n\n2. b\n");
    let result = apply_template(&generated, &archive(&parts)).unwrap();
    assert_valid(&result);
    assert_same_text_as(&result, &generated);
    assert!(text_of_part(&result, "word/document.xml").contains("<w:numId w:val=\"2\"/>"));
    assert!(text_of_part(&result, "word/numbering.xml").contains("<w:num w:numId=\"2\">"));
    assert!(text_of_part(&result, "word/_rels/document.xml.rels").contains("Target=\"numbering.xml\""));
    assert!(text_of_part(&result, "[Content_Types].xml").contains("/word/numbering.xml"));
}
