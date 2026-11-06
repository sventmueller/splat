//! Pictures, links and the parts that come along with them.

use super::*;

#[test]
fn pictures_and_links_get_ids_above_those_of_the_template_and_unique_names() {
    let result = apply_template(
        &export("![Bild](a.png) ![Bild](b.png)\n\n[Link](https://a.bc/?x=1&y=2)\n"),
        &full_template(),
    )
    .unwrap();
    assert_valid(&result);
    let rels = text_of_part(&result, "word/_rels/document.xml.rels");
    assert!(
        rels.contains(&format!(
            "<Relationship Id=\"rId10\" Type=\"{REL_BASE}/image\" Target=\"media/image1_1.png\"/>"
        )),
        "{rels}"
    );
    assert!(
        rels.contains(&format!(
            "<Relationship Id=\"rId11\" Type=\"{REL_BASE}/image\" Target=\"media/image2.png\"/>"
        )),
        "{rels}"
    );
    assert!(
        rels.contains(&format!(
            "<Relationship Id=\"rId12\" Type=\"{REL_BASE}/hyperlink\" Target=\"https://a.bc/?x=1&amp;y=2\" TargetMode=\"External\"/>"
        )),
        "{rels}"
    );
    let document = text_of_part(&result, "word/document.xml");
    assert!(
        document.contains("r:embed=\"rId10\"") && document.contains("r:id=\"rId12\""),
        "{document}"
    );
    assert!(!document.contains("rIdI1"), "{document}");
    assert!(document.contains("xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\""));
    assert!(
        document.contains("xmlns:pic=") && document.contains("xmlns:a="),
        "{document}"
    );
    let parts = entries(&result);
    assert_eq!(parts["word/media/image1.png"], b"template picture");
    assert_eq!(parts["word/media/image1_1.png"], PNG);
    let types = text_of_part(&result, "[Content_Types].xml");
    assert!(
        !types.contains("Extension=\"png\""),
        "the template's PNG covers it: {types}"
    );
    assert_same_text_as(
        &result,
        &export("![Bild](a.png) ![Bild](b.png)\n\n[Link](https://a.bc/?x=1&y=2)\n"),
    );
}

#[test]
fn a_picture_type_the_template_does_not_know_is_added_once() {
    let generated = export("![a](a.png) ![b](b.png)\n");
    let mut parts = template_parts(DOCUMENT_TYPE);
    set_part(
        &mut parts,
        "[Content_Types].xml",
        &template_types(DOCUMENT_TYPE).replace("<Default Extension=\"PNG\" ContentType=\"image/png\"/>", ""),
    );
    let result = apply_template(&generated, &archive(&parts)).unwrap();
    assert_valid(&result);
    assert_eq!(
        text_of_part(&result, "[Content_Types].xml")
            .matches("Extension=\"png\"")
            .count(),
        1
    );
}

#[test]
fn a_template_without_relationships_gets_them() {
    let parts = vec![
        stored(
            "[Content_Types].xml",
            format!(
                "<Types {TYPES}><Default Extension=\"xml\" ContentType=\"application/xml\"/>\
                 <Override PartName=\"/word/document.xml\" ContentType=\"{DOCUMENT_TYPE}\"/></Types>"
            )
            .as_bytes(),
        ),
        stored(
            "word/document.xml",
            format!("<w:document {W}><w:body><w:p/></w:body></w:document>").as_bytes(),
        ),
    ];
    let generated = export(SAMPLE);
    let result = apply_template(&generated, &archive(&parts)).unwrap();
    assert_valid(&result);
    assert_same_text_as(&result, &generated);
    let rels = text_of_part(&result, "word/_rels/document.xml.rels");
    assert!(rels.starts_with("<?xml") && rels.contains("Id=\"rId1\""), "{rels}");
    let types = text_of_part(&result, "[Content_Types].xml");
    assert!(
        types.contains("Extension=\"rels\"") && types.contains("Extension=\"png\" ContentType=\"image/png\""),
        "{types}"
    );
    let document = text_of_part(&result, "word/document.xml");
    assert!(
        document.contains("<w:sectPr>") && document.contains("w:w=\"11906\""),
        "the generated section stands in: {document}"
    );
}

#[test]
fn relationships_to_what_cannot_come_along_are_dropped_with_their_references() {
    let mut file = replaced(
        &export("a\n"),
        "word/document.xml",
        "<w:r>",
        "<w:r><w:x r:id=\"gone\" r:embed=\"chart\" r:link=\"far\" r:dm=\"out\" r:pict=\"noname\"/></w:r><w:r>",
    );
    file = replaced(
        &file,
        "word/_rels/document.xml.rels",
        "</Relationships>",
        &format!(
            "<Relationship Id=\"chart\" Type=\"{REL_BASE}/chart\" Target=\"charts/a.xml\"/>\
             <Relationship Id=\"far\" Type=\"{REL_BASE}/image\" Target=\"https://a.bc/p.png\" TargetMode=\"External\"/>\
             <Relationship Id=\"out\" Type=\"{REL_BASE}/image\" Target=\"../../../etc/passwd\"/>\
             <Relationship Id=\"noname\" Type=\"{REL_BASE}/image\" Target=\"media/none.png\"/></Relationships>"
        ),
    );
    let result = apply_template(&file, &full_template()).unwrap();
    assert_valid(&result);
    let document = text_of_part(&result, "word/document.xml");
    assert!(document.contains("<w:x r:link=\"rId10\"/>"), "{document}");
    let rels = text_of_part(&result, "word/_rels/document.xml.rels");
    assert!(
        rels.contains("Target=\"https://a.bc/p.png\" TargetMode=\"External\""),
        "{rels}"
    );
    assert!(
        !rels.contains("etc/passwd") && !rels.contains("none.png") && !rels.contains("charts"),
        "{rels}"
    );
}

#[test]
fn the_name_of_a_picture_cannot_lead_out_of_the_media_folder() {
    let body = "<w:r><w:a r:embed=\"a\"/><w:b r:embed=\"b\"/><w:c r:embed=\"c\"/><w:d r:embed=\"d\"/></w:r><w:r>";
    let mut file = replaced(&export("a\n"), "word/document.xml", "<w:r>", body);
    file = replaced(
        &file,
        "word/_rels/document.xml.rels",
        "</Relationships>",
        &format!(
            "<Relationship Id=\"a\" Type=\"{REL_BASE}/image\" Target=\"../evil/..dots!.GIF\"/>\
             <Relationship Id=\"b\" Type=\"{REL_BASE}/image\" Target=\"noextension\"/>\
             <Relationship Id=\"c\" Type=\"{REL_BASE}/image\" Target=\"odd.x-y\"/>\
             <Relationship Id=\"d\" Type=\"{REL_BASE}/image\" Target=\"media/!!!.wmf\"/></Relationships>"
        ),
    );
    file = with_part(&file, "evil/..dots!.GIF", b"gif");
    file = with_part(&file, "word/noextension", b"x");
    file = with_part(&file, "word/odd.x-y", b"y");
    file = with_part(&file, "word/media/!!!.wmf", b"w");
    let result = apply_template(&file, &full_template()).unwrap();
    assert_valid(&result);
    let parts = entries(&result);
    assert_eq!(parts["word/media/dots.gif"], b"gif");
    assert_eq!(parts["word/media/noextension.bin"], b"x");
    assert_eq!(parts["word/media/odd.bin"], b"y");
    assert_eq!(parts["word/media/image.wmf"], b"w");
    let types = text_of_part(&result, "[Content_Types].xml");
    assert!(
        types.contains("<Default Extension=\"gif\" ContentType=\"image/gif\"/>"),
        "{types}"
    );
    assert!(
        types.contains("<Default Extension=\"bin\" ContentType=\"application/octet-stream\"/>"),
        "{types}"
    );
    assert!(
        types.contains("<Default Extension=\"wmf\" ContentType=\"image/x-wmf\"/>"),
        "{types}"
    );
}

#[test]
fn a_picture_two_relationships_lead_to_is_copied_once() {
    let body = "<w:r><w:a r:embed=\"a\"/><w:b r:embed=\"b\"/></w:r><w:r>";
    let mut file = replaced(&export("x\n"), "word/document.xml", "<w:r>", body);
    file = replaced(
        &file,
        "word/_rels/document.xml.rels",
        "</Relationships>",
        &format!(
            "<Relationship Id=\"a\" Type=\"{REL_BASE}/image\" Target=\"media/p.png\"/>\
             <Relationship Id=\"b\" Type=\"{REL_BASE}/image\" Target=\"/word/media/p.png\"/></Relationships>"
        ),
    );
    file = with_part(&file, "word/media/p.png", PNG);
    let result = apply_template(&file, &full_template()).unwrap();
    assert_valid(&result);
    let parts = entries(&result);
    assert_eq!(parts.keys().filter(|name| name.contains("media/p")).count(), 1);
    let document = text_of_part(&result, "word/document.xml");
    assert!(
        document.contains("r:embed=\"rId10\"") && document.contains("r:embed=\"rId11\""),
        "{document}"
    );
}

#[test]
fn a_part_that_comes_along_has_a_type_even_if_the_text_gives_it_none() {
    let file = replaced(
        &with_notes("endnoteReference", "endnotes", "endnote"),
        "[Content_Types].xml",
        &format!("<Override PartName=\"/word/endnotes.xml\" ContentType=\"{OFFICE}.endnotes+xml\"/>"),
        "",
    );
    let result = apply_template(&file, &full_template()).unwrap();
    assert_valid(&result);
    let types = text_of_part(&result, "[Content_Types].xml");
    assert!(types.contains(&format!(
        "<Override PartName=\"/word/endnotes.xml\" ContentType=\"{OFFICE}.endnotes+xml\"/>"
    )));
}

#[test]
fn the_name_of_a_new_part_does_not_take_that_of_one_the_template_has() {
    let mut parts = template_parts(DOCUMENT_TYPE);
    set_part(
        &mut parts,
        "word/_rels/document.xml.rels",
        &template_rels().replace(
            &format!("<Relationship Id=\"rId1\" Type=\"{REL_BASE}/styles\" Target=\"styles.xml\"/>"),
            "",
        ),
    );
    let result = apply_template(&export("a\n"), &archive(&parts)).unwrap();
    let parts = entries(&result);
    assert!(parts.contains_key("word/styles_1.xml"), "{:?}", parts.keys());
    assert_eq!(text_of_part(&result, "word/styles.xml"), template_styles());
    assert!(text_of_part(&result, "word/_rels/document.xml.rels").contains("Target=\"styles_1.xml\""));
}

#[test]
fn the_type_of_a_picture_comes_from_the_table_as_a_last_resort() {
    for (extension, kind) in [
        ("png", "image/png"),
        ("jpeg", "image/jpeg"),
        ("jpg", "image/jpeg"),
        ("gif", "image/gif"),
        ("webp", "image/webp"),
        ("bmp", "image/bmp"),
        ("tiff", "image/tiff"),
        ("tif", "image/tiff"),
        ("emf", "image/x-emf"),
        ("wmf", "image/x-wmf"),
        ("xyz", "application/octet-stream"),
    ] {
        assert_eq!(media_type(extension), kind);
    }
}

#[test]
fn a_main_part_in_another_folder_gets_targets_that_lead_to_the_media() {
    let parts = vec![
        stored(
            "[Content_Types].xml",
            format!(
                "<Types {TYPES}><Default Extension=\"rels\" ContentType=\"a\"/><Default Extension=\"xml\" ContentType=\"b\"/>\
                 <Override PartName=\"/doc/main.xml\" ContentType=\"{DOCUMENT_TYPE}\"/></Types>"
            )
            .as_bytes(),
        ),
        stored(
            "doc/main.xml",
            format!("<w:document {W}><w:body><w:p/></w:body></w:document>").as_bytes(),
        ),
    ];
    let result = apply_template(&export("![a](a.png)\n"), &archive(&parts)).unwrap();
    let rels = text_of_part(&result, "doc/_rels/main.xml.rels");
    assert!(rels.contains("Target=\"/word/media/image1.png\""), "{rels}");
    assert!(rels.contains("Target=\"/word/styles.xml\""), "{rels}");
    assert_valid(&result);
}
