//! Footnotes and endnotes behind those of the template.

use super::*;

#[test]
fn notes_go_behind_those_of_the_template_and_its_separators_stay() {
    let result = apply_template(
        &with_notes("footnoteReference", "footnotes", "footnote"),
        &full_template(),
    )
    .unwrap();
    assert_valid(&result);
    let notes = text_of_part(&result, "word/footnotes.xml");
    assert!(
        notes.contains("<w:footnote w:type=\"separator\" w:id=\"-1\"><w:p/></w:footnote>"),
        "{notes}"
    );
    assert!(
        notes.contains("<w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p/></w:footnote>"),
        "{notes}"
    );
    assert_eq!(notes.matches("w:type=\"separator\"").count(), 1, "{notes}");
    assert!(
        notes.contains(
            "<w:footnote w:id=\"1\"><w:p><w:pPr><w:pStyle w:val=\"FootnoteText\"/></w:pPr><w:hyperlink r:id=\"rId1\">"
        ),
        "{notes}"
    );
    assert!(
        notes.contains(R),
        "the prefix of its relationships is declared: {notes}"
    );
    let document = text_of_part(&result, "word/document.xml");
    assert!(document.contains("<w:footnoteReference w:id=\"1\"/>"), "{document}");
    let rels = text_of_part(&result, "word/_rels/footnotes.xml.rels");
    assert!(
        rels.contains("Target=\"https://a.bc\" TargetMode=\"External\"") && rels.contains("Target=\"media/note.png\""),
        "{rels}"
    );
    assert_eq!(entries(&result)["word/media/note.png"], PNG);
    let styles = text_of_part(&result, "word/styles.xml");
    assert!(
        styles.contains("w:styleId=\"FootnoteText\""),
        "a style only a note uses comes along: {styles}"
    );
}

#[test]
fn notes_are_added_to_relationships_the_template_has_for_its_notes() {
    let mut parts = template_parts(DOCUMENT_TYPE);
    parts.push(stored(
        "word/_rels/footnotes.xml.rels",
        format!("<Relationships {RELS}><Relationship Id=\"rId3\" Type=\"a\" Target=\"https://x.y\" TargetMode=\"External\"/></Relationships>").as_bytes(),
    ));
    let result = apply_template(
        &with_notes("footnoteReference", "footnotes", "footnote"),
        &archive(&parts),
    )
    .unwrap();
    assert_valid(&result);
    let notes = text_of_part(&result, "word/footnotes.xml");
    assert!(notes.contains("r:id=\"rId4\""), "{notes}");
    let rels = text_of_part(&result, "word/_rels/footnotes.xml.rels");
    assert!(rels.contains("Id=\"rId3\"") && rels.contains("Id=\"rId4\""), "{rels}");
}

#[test]
fn a_note_part_only_the_text_has_comes_along_with_its_relationships() {
    let result = apply_template(&with_notes("endnoteReference", "endnotes", "endnote"), &full_template()).unwrap();
    assert_valid(&result);
    let notes = text_of_part(&result, "word/endnotes.xml");
    assert!(
        notes.contains("w:type=\"separator\"") && notes.contains("Notiz"),
        "{notes}"
    );
    assert!(
        notes.contains("<w:endnote w:id=\"2\">") && notes.starts_with("<?xml"),
        "{notes}"
    );
    assert!(text_of_part(&result, "word/document.xml").contains("<w:endnoteReference w:id=\"2\"/>"));
    let rels = text_of_part(&result, "word/_rels/document.xml.rels");
    assert!(rels.contains("/endnotes\" Target=\"endnotes.xml\""), "{rels}");
    assert!(text_of_part(&result, "[Content_Types].xml").contains(&format!(
        "<Override PartName=\"/word/endnotes.xml\" ContentType=\"{OFFICE}.endnotes+xml\"/>"
    )));
    assert!(text_of_part(&result, "word/_rels/endnotes.xml.rels").contains("media/note.png"));
}

#[test]
fn a_note_part_nothing_refers_to_stays_out() {
    let file = with_notes("footnoteReference", "footnotes", "footnote");
    let file = replaced(
        &file,
        "word/document.xml",
        "<w:r><w:footnoteReference w:id=\"2\"/></w:r>",
        "",
    );
    let template = full_template();
    let result = apply_template(&file, &template).unwrap();
    assert_valid(&result);
    assert_eq!(
        text_of_part(&result, "word/footnotes.xml"),
        text_of_part(&template, "word/footnotes.xml")
    );
    assert!(!entries(&result).contains_key("word/media/note.png"));
}

#[test]
fn a_reference_to_a_note_that_did_not_come_along_loses_its_id() {
    let file = replaced(
        &export("a\n"),
        "word/document.xml",
        "<w:r>",
        "<w:r><w:footnoteReference w:id=\"2\"/></w:r><w:r>",
    );
    let result = apply_template(&file, &full_template()).unwrap();
    assert_valid(&result);
    assert!(text_of_part(&result, "word/document.xml").contains("<w:footnoteReference/>"));
}
