use super::*;
use splat_ooxml::zip::testing::{archive, stored};

const WORD_TYPES: &str = "<Types><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";

fn file_of(parts: &[(&str, &str)]) -> Vec<u8> {
    let parts: Vec<_> = parts
        .iter()
        .map(|(name, content)| stored(name, content.as_bytes()))
        .collect();
    archive(&parts)
}

#[test]
fn a_document_of_too_many_nodes_is_too_large() {
    assert_eq!(WordError::from(XmlError::TooLarge), WordError::TooLarge);
}

#[test]
fn zip_errors_say_what_they_mean_for_a_word_file() {
    assert_eq!(WordError::from(ZipError::NotAnArchive), WordError::NotAWordFile);
    assert_eq!(WordError::from(ZipError::Encrypted), WordError::Encrypted);
    assert_eq!(WordError::from(ZipError::Zip64), WordError::TooLarge);
    assert_eq!(WordError::from(ZipError::TooLarge), WordError::TooLarge);
    assert_eq!(WordError::from(ZipError::Damaged), WordError::Damaged);
}

#[test]
fn targets_resolve_inside_the_archive() {
    assert_eq!(resolve("word", "media/a.png").as_deref(), Some("word/media/a.png"));
    assert_eq!(resolve("word", "/custom/a.xml").as_deref(), Some("custom/a.xml"));
    assert_eq!(resolve("word/sub", "../a.xml").as_deref(), Some("word/a.xml"));
    assert_eq!(resolve("word", "./x/./a.xml").as_deref(), Some("word/x/a.xml"));
    assert_eq!(resolve("", "a.xml").as_deref(), Some("a.xml"));
    assert_eq!(resolve("word", "../../../etc/passwd").as_deref(), Some("etc/passwd"));
}

#[test]
fn relationship_targets_resolve_inside_the_package_only() {
    assert_eq!(resolve("word", "media/a.png").as_deref(), Some("word/media/a.png"));
    assert_eq!(
        resolve("word", "../customXml/a.xml").as_deref(),
        Some("customXml/a.xml")
    );
    assert_eq!(resolve("word", "/word/./a//b.xml").as_deref(), Some("word/a/b.xml"));
    assert_eq!(resolve("", "a.xml").as_deref(), Some("a.xml"));
    assert_eq!(resolve("word", "../../etc/passwd").as_deref(), Some("etc/passwd"));
    assert_eq!(resolve("word", ".."), None);
    assert_eq!(resolve("", "/"), None);
}

#[test]
fn the_relationships_of_a_part_are_next_to_it() {
    assert_eq!(rels_name("word/document.xml"), "word/_rels/document.xml.rels");
    assert_eq!(rels_name("document.xml"), "_rels/document.xml.rels");
    assert_eq!(folder_of("word/sub/a.xml"), "word/sub");
    assert_eq!(folder_of("a.xml"), "");
}

#[test]
fn relationships_without_a_target_or_an_id_are_no_relationships() {
    let rels = Relationships::read(
        "<Relationships><Relationship Id=\"a\"/><Relationship Target=\"b\"/>\
         <Relationship Id=\"c\" Target=\"d\" TargetMode=\"External\"/><x/></Relationships>",
        "word",
        &XmlBudget::default(),
    )
    .unwrap();
    assert_eq!(rels.iter().count(), 1);
    let only = rels.get("c").unwrap();
    assert_eq!(
        (only.id.as_str(), only.external, only.part.as_deref()),
        ("c", true, None)
    );
    assert!(only.kind.is_empty());
}

#[test]
fn relationships_are_found_by_id_and_by_the_last_word_of_their_type() {
    let rels = Relationships::read(
        "<Relationships>\
         <Relationship Id=\"x\" Type=\"urn:t/styles\" Target=\"http://a.bc/s.xml\" TargetMode=\"External\"/>\
         <Relationship Id=\"a\" Type=\"urn:t/styles\" Target=\"first.xml\"/>\
         <Relationship Id=\"b\" Type=\"urn:u/styles\" Target=\"second.xml\"/>\
         <Relationship Id=\"a\" Type=\"urn:t/image\" Target=\"media/p.png\"/>\
         <Relationship Id=\"r\" Type=\"urn:t/root\" Target=\"..\"/></Relationships>",
        "word",
        &XmlBudget::default(),
    )
    .unwrap();
    assert_eq!(
        rels.of_type("styles").and_then(|rel| rel.part.as_deref()),
        Some("word/first.xml")
    );
    assert_eq!(
        rels.get("a").map(|rel| rel.target.as_str()),
        Some("media/p.png"),
        "the last of an id"
    );
    assert!(rels.of_type("root").is_none(), "the root is no part");
    assert!(rels.of_type("nothing").is_none());
}

#[test]
fn a_byte_order_mark_is_left_out_and_other_encodings_are_damage() {
    assert_eq!(into_text(b"\xEF\xBB\xBF<a/>".to_vec()), Ok("<a/>".to_string()));
    assert_eq!(into_text(b"<a/>".to_vec()), Ok("<a/>".to_string()));
    assert_eq!(into_text(b"\xFF\xFE<".to_vec()), Err(WordError::Damaged));
}

#[test]
fn a_part_in_another_form_only_changes_where_it_is_xml() {
    let bytes = b"<a xmlns=\"http://purl.oclc.org/ooxml/drawingml/main\"/>".to_vec();
    assert_eq!(in_the_usual_form("x.png", bytes.clone()), bytes);
    assert!(String::from_utf8(in_the_usual_form("x.xml", bytes)).is_ok_and(|text| text.contains("openxmlformats")));
    let broken = b"purl.oclc.org/ooxml/\xFF".to_vec();
    assert_eq!(in_the_usual_form("x.xml", broken.clone()), broken);
}

#[test]
fn the_strict_form_changes_in_names_of_vocabularies_only() {
    let strict = "<?xml version=\"1.0\"?><!-- xmlns=\"http://purl.oclc.org/ooxml/drawingml/main\" -->\
        <w:document xmlns:w = 'http://purl.oclc.org/ooxml/wordprocessingml/main' \
        xmlns:r=\"http://purl.oclc.org/ooxml/officeDocument/relationships\" x=\"http://purl.oclc.org/ooxml/drawingml/main\" \
        w:conformance=\"strict\">\
        <w:t>xmlns=\"http://purl.oclc.org/ooxml/drawingml/main\"</w:t>\
        <a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\"/></w:document>";
    let usual = "<?xml version=\"1.0\"?><!-- xmlns=\"http://purl.oclc.org/ooxml/drawingml/main\" -->\
        <w:document xmlns:w = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main' \
        xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" x=\"http://purl.oclc.org/ooxml/drawingml/main\" \
        w:conformance=\"transitional\">\
        <w:t>xmlns=\"http://purl.oclc.org/ooxml/drawingml/main\"</w:t>\
        <a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"/></w:document>";
    assert_eq!(with_usual_values(strict).as_deref(), Some(usual));
    assert_eq!(with_usual_values(usual), None);
    assert_eq!(with_usual_values("<a b=\"http://purl.oclc.org/ooxml/x"), None);
    assert_eq!(with_usual_values("<a xmlns=\""), None);

    let rels = "<Relationships><Relationship Type=\"http://purl.oclc.org/ooxml/officeDocument/relationships/styles\"/>\
        <Relationship Type=\"http://purl.oclc.org/ooxml/officeDocument/relationships/extendedProperties\"/>\
        <Relationship Type=\"http://purl.oclc.org/ooxml/officeDocument/relationships/customProperties\"/>\
        <Relationship Target=\"http://purl.oclc.org/ooxml/officeDocument/relationships/styles\"/></Relationships>";
    assert_eq!(
        with_usual_values(rels).as_deref(),
        Some(
            "<Relationships><Relationship Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\"/>\
             <Relationship Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\"/>\
             <Relationship Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties\"/>\
             <Relationship Target=\"http://purl.oclc.org/ooxml/officeDocument/relationships/styles\"/></Relationships>"
        )
    );
}

#[test]
fn a_package_names_its_main_part_and_what_types_it_has() {
    let types = "<Types><Default Extension=\"PNG\" ContentType=\"image/png\"/>\
        <Override PartName=\"/word/styles.xml\" ContentType=\"x\"/>\
        <Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    let file = file_of(&[(TYPES_PART, types), ("word/document.xml", "<a/>")]);
    let budget = XmlBudget::default();
    let package = Package::open(&file, &[DOCUMENT_TYPE], &budget).unwrap();
    assert_eq!(package.main, "word/document.xml");
    assert_eq!(package.defaults["png"], "image/png");
    assert_eq!(package.overrides["word/styles.xml"], "x");
    assert_eq!(package.types, types);
    assert!(matches!(
        Package::open(&file, &[TEMPLATE_TYPE], &budget),
        Err(WordError::NotAWordFile)
    ));
    assert_eq!(package.text("word/none.xml"), Ok(None));
    assert_eq!(
        package
            .relationships("word/document.xml")
            .map(|rels| rels.iter().count()),
        Ok(0)
    );
}

#[test]
fn the_parts_of_a_package_share_one_budget_and_each_is_refused_before_it_is_inflated() {
    let document = format!("<a>{}</a>", "x".repeat(500));
    let file = file_of(&[
        (TYPES_PART, WORD_TYPES),
        ("word/document.xml", &document),
        ("word/styles.xml", &document),
    ]);
    let room = u64::try_from(WORD_TYPES.len() + document.len()).unwrap();
    let budget = XmlBudget::new(room, u32::MAX);
    let package = Package::open(&file, &[DOCUMENT_TYPE], &budget).unwrap();
    assert_eq!(package.text("word/document.xml"), Ok(Some(document.clone())));
    assert_eq!(package.text("word/styles.xml"), Err(WordError::TooLarge));
    let alone = XmlBudget::new(room, u32::MAX);
    let package = Package::open(&file, &[DOCUMENT_TYPE], &alone).unwrap();
    assert_eq!(
        package.text("word/styles.xml"),
        Ok(Some(document)),
        "either part alone fits"
    );
    let without_room = XmlBudget::new(u64::try_from(WORD_TYPES.len()).unwrap() - 1, u32::MAX);
    assert!(matches!(
        Package::open(&file, &[DOCUMENT_TYPE], &without_room),
        Err(WordError::TooLarge)
    ));
}

#[test]
fn what_is_no_package_says_why() {
    let budget = XmlBudget::default();
    let open = |bytes: &[u8]| Package::open(bytes, &[DOCUMENT_TYPE], &budget).map(|package| package.main);
    assert_eq!(open(b""), Err(WordError::NotAWordFile));
    let mut old = OLE_MAGIC.to_vec();
    old.extend_from_slice(b"WordDocument");
    assert_eq!(open(&old), Err(WordError::NotAWordFile));
    let mut encrypted = OLE_MAGIC.to_vec();
    encrypted.extend("xEncryptedPackagey".encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(open(&encrypted), Err(WordError::Encrypted));
    assert_eq!(open(&file_of(&[("a", "b")])), Err(WordError::NotAWordFile));
    assert_eq!(open(&file_of(&[(TYPES_PART, WORD_TYPES)])), Err(WordError::Damaged));
    assert_eq!(open(&file_of(&[(TYPES_PART, "<Types")])), Err(WordError::Damaged));
}
