//! The text in the look of the template, and what of the template stays as it was.

use super::*;

#[test]
fn the_generated_text_takes_the_look_of_the_template() {
    let generated = export(SAMPLE);
    let template = full_template();
    let result = apply_template(&generated, &template).unwrap();
    assert_valid(&result);
    assert_same_text_as(&result, &generated);
    let document = text_of_part(&result, "word/document.xml");
    assert!(
        document.contains(&ROOT_START["<w:document".len()..]),
        "its root keeps all it says: {document}"
    );
    assert!(document.contains(FINAL_SECTION), "{document}");
    assert!(
        document.ends_with(&format!("{FINAL_SECTION}</w:body></w:document>")),
        "{document}"
    );
    assert!(
        !document.contains("Vorlage") && !document.contains("w:w=\"1\""),
        "{document}"
    );
    assert!(
        document.contains("Zitat") && document.contains("fett") && document.contains("<w:tbl>"),
        "{document}"
    );
    assert!(
        !document.contains("1417"),
        "the generated page size is gone: {document}"
    );
}

#[test]
fn what_the_template_holds_stays_byte_for_byte_except_what_had_to_change() {
    let template = full_template();
    let result = apply_template(&export(SAMPLE), &template).unwrap();
    let (before, after) = (entries(&template), entries(&result));
    let changed: HashSet<&str> = [
        "word/document.xml",
        "word/_rels/document.xml.rels",
        "word/styles.xml",
        "word/numbering.xml",
    ]
    .into();
    for (name, content) in &before {
        if name.ends_with('/') {
            assert!(!after.contains_key(name));
        } else if changed.contains(name.as_str()) {
            assert_ne!(&after[name], content, "{name}");
        } else {
            assert_eq!(&after[name], content, "{name}");
        }
    }
    let names: Vec<String> = Archive::read(&result).unwrap().names().map(String::from).collect();
    assert_eq!(names[0], "[Content_Types].xml");
    assert_eq!(names[1], "_rels/.rels");
    let rels = text_of_part(&result, "word/_rels/document.xml.rels");
    for kept in ["rId1", "rId9", "anker", "header1.xml"] {
        assert!(rels.contains(kept), "{rels}");
    }
    let styles = text_of_part(&result, "word/styles.xml");
    assert!(
        styles.starts_with(&template_styles().replace("</w:styles>", "")),
        "{styles}"
    );
}

#[test]
fn applying_a_template_twice_gives_the_same_bytes() {
    let generated = export(SAMPLE);
    let template = full_template();
    assert_eq!(
        apply_template(&generated, &template).unwrap(),
        apply_template(&generated, &template).unwrap()
    );
}

#[test]
fn a_dotx_becomes_a_docx() {
    let template = archive(&template_parts(TEMPLATE_MAIN));
    let result = apply_template(&export(SAMPLE), &template).unwrap();
    assert_valid(&result);
    let types = text_of_part(&result, "[Content_Types].xml");
    assert!(
        types.contains(&format!(
            "<Override PartName=\"/word/document.xml\" ContentType=\"{DOCUMENT_TYPE}\"/>"
        )),
        "{types}"
    );
    assert!(!types.contains("template.main"), "{types}");
}

#[test]
fn a_body_that_holds_nothing_or_only_a_space_takes_the_text() {
    for body in [
        "<w:body/>",
        "<w:body></w:body>",
        "<w:body> </w:body>",
        "<w:body><!-- c --></w:body>",
    ] {
        let mut parts = template_parts(DOCUMENT_TYPE);
        set_part(
            &mut parts,
            "word/document.xml",
            &format!("<w:document {W} {R}>{body}</w:document>"),
        );
        let generated = export("Text\n");
        let result = apply_template(&generated, &archive(&parts)).unwrap();
        assert_valid(&result);
        assert_same_text_as(&result, &generated);
        let document = text_of_part(&result, "word/document.xml");
        assert!(document.contains("<w:body>"), "{document}");
        assert!(
            document.contains("Text</w:t>") && document.contains("</w:body></w:document>"),
            "{document}"
        );
    }
}

#[test]
fn a_text_without_a_section_takes_no_section_from_nowhere() {
    let generated = replaced(
        &export("a\n"),
        "word/document.xml",
        "<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1417\" w:bottom=\"1134\" w:left=\"1417\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr>",
        "",
    );
    let mut parts = template_parts(DOCUMENT_TYPE);
    set_part(
        &mut parts,
        "word/document.xml",
        &format!("<w:document {W}><w:body><w:p/></w:body></w:document>"),
    );
    let result = apply_template(&generated, &archive(&parts)).unwrap();
    assert_valid(&result);
    assert!(!text_of_part(&result, "word/document.xml").contains("sectPr"));
}

#[test]
fn a_template_with_other_prefixes_is_written_in_its_own_terms() {
    let main = "<ns0:document xmlns:ns0=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
                xmlns:ns1=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:w=\"urn:another\">\
                <ns0:body><ns0:p/><ns0:sectPr><ns0:headerReference ns1:id=\"rId5\"/></ns0:sectPr></ns0:body></ns0:document>";
    let styles = "<ns0:styles xmlns:ns0=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w=\"urn:another\">\
                  <ns0:style ns0:styleId=\"Standard\"><ns0:name ns0:val=\"Normal\"/></ns0:style></ns0:styles>";
    let numbering = "<ns0:numbering xmlns:ns0=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"/>";
    let rels = template_rels()
        .replace("Relationships xmlns=", "x:Relationships xmlns:x=")
        .replace("<Relationship ", "<x:Relationship ");
    let mut parts = template_parts(DOCUMENT_TYPE);
    set_part(&mut parts, "word/document.xml", main);
    set_part(&mut parts, "word/styles.xml", styles);
    set_part(&mut parts, "word/numbering.xml", numbering);
    set_part(
        &mut parts,
        "word/_rels/document.xml.rels",
        &rels.replace("</Relationships>", "</x:Relationships>"),
    );
    let generated = export(SAMPLE);
    let result = apply_template(&generated, &archive(&parts)).unwrap();
    assert_valid_prefixed(&result);
    assert_same_text_as(&result, &generated);
    let document = text_of_part(&result, "word/document.xml");
    assert!(
        document.contains("<ns0:p>") && !document.contains("<w:p>"),
        "{document}"
    );
    assert!(
        document.contains("ns1:id=\"rId10\"") && document.contains("ns1:embed=\"rId11\""),
        "{document}"
    );
    assert!(
        document.contains("<ns0:sectPr><ns0:headerReference ns1:id=\"rId5\"/></ns0:sectPr>"),
        "{document}"
    );
    assert!(text_of_part(&result, "word/_rels/document.xml.rels").contains("<x:Relationship Id=\"rId10\""));
    assert!(
        text_of_part(&result, "word/styles.xml")
            .contains("<ns0:style ns0:type=\"paragraph\" ns0:styleId=\"Heading1\">")
    );
    assert!(text_of_part(&result, "word/numbering.xml").contains("<ns0:abstractNum ns0:abstractNumId=\"1\">"));
}

/// As `assert_valid`, for parts whose relationships are under a prefix.
fn assert_valid_prefixed(file: &[u8]) {
    assert_valid(file);
    let parts = entries(file);
    let doc = Document::parse(std::str::from_utf8(&parts["word/document.xml"]).unwrap()).unwrap();
    let body = doc.root_element().first_element_child().unwrap();
    assert_eq!(body.tag_name().namespace(), Some(WORD));
    assert!(body.last_element_child().unwrap().has_tag_name((WORD, "sectPr")));
}

#[test]
fn a_template_in_the_default_namespace_works_as_well() {
    let main = "<document xmlns=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><body><sectPr><pgSz/></sectPr></body></document>";
    let styles = "<styles xmlns=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><style styleId=\"S\"/></styles>";
    let mut parts = template_parts(DOCUMENT_TYPE);
    set_part(&mut parts, "word/document.xml", main);
    set_part(&mut parts, "word/styles.xml", styles);
    let generated = export(SAMPLE);
    let result = apply_template(&generated, &archive(&parts)).unwrap();
    assert_valid(&result);
    assert_same_text_as(&result, &generated);
    let document = text_of_part(&result, "word/document.xml");
    assert!(
        document
            .starts_with("<document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r="),
        "{document}"
    );
    assert!(
        document.contains("<p>") && document.contains("<pStyle w:val=\"Heading1\"/>"),
        "{document}"
    );
    assert!(document.contains("<sectPr><pgSz/></sectPr></body>"), "{document}");
}

/// `file` as Word writes it in the Strict form of ISO/IEC 29500.
fn in_strict_form(file: &[u8]) -> Vec<u8> {
    let read = Archive::read(file).unwrap();
    let parts: Vec<Entry> = read
        .names()
        .map(|name| {
            let content = read.get(name).unwrap().unwrap();
            let content = match String::from_utf8(content) {
                Ok(text) => text
                    .replace(
                        "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
                        "http://purl.oclc.org/ooxml/wordprocessingml/main",
                    )
                    .replace(REL_BASE, "http://purl.oclc.org/ooxml/officeDocument/relationships")
                    .replace("<w:document ", "<w:document w:conformance=\"strict\" ")
                    .into_bytes(),
                Err(error) => error.into_bytes(),
            };
            stored(name, &content)
        })
        .collect();
    archive(&parts)
}

#[test]
fn a_template_in_the_strict_form_gives_a_document_in_the_usual_one() {
    let generated = export(SAMPLE);
    let template = in_strict_form(&full_template());
    assert!(text_of_part(&template, "word/styles.xml").contains("purl.oclc.org"));
    let result = apply_template(&generated, &template).unwrap();
    assert_valid(&result);
    assert_same_text_as(&result, &generated);
    for (name, content) in entries(&result) {
        let text = String::from_utf8_lossy(&content);
        assert!(!text.contains("purl.oclc.org"), "{name}: {text}");
    }
    assert!(text_of_part(&result, "word/header1.xml").contains("Kopf"));
    assert!(text_of_part(&result, "word/styles.xml").contains("w:styleId=\"berschrift1\""));
    assert!(text_of_part(&result, "word/document.xml").contains("w:conformance=\"transitional\""));
}
