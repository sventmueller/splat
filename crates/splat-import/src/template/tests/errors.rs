//! What cannot be merged, and what it is said to be.

use super::*;
use crate::xml::XmlError;
use splat_ooxml::zip::{TooLarge, ZipError};

#[test]
fn what_is_no_word_file_is_refused() {
    let generated = export("a\n");
    let template = full_template();
    assert_eq!(apply_template(&generated, b"plain"), Err(WordError::NotAWordFile));
    assert_eq!(apply_template(b"plain", &template), Err(WordError::NotAWordFile));
    let mut ole = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1".to_vec();
    ole.extend_from_slice(&[0; 600]);
    assert_eq!(apply_template(&generated, &ole), Err(WordError::NotAWordFile));
    let no_types = archive(&[stored("word/document.xml", b"<a/>")]);
    assert_eq!(apply_template(&generated, &no_types), Err(WordError::NotAWordFile));
    let other = archive(&[stored(
        "[Content_Types].xml",
        format!("<Types {TYPES}><Override PartName=\"/x.xml\" ContentType=\"application/xml\"/><Other/></Types>")
            .as_bytes(),
    )]);
    assert_eq!(apply_template(&generated, &other), Err(WordError::NotAWordFile));
}

#[test]
fn an_encrypted_archive_and_a_template_with_macros_are_refused() {
    let mut parts = template_parts(DOCUMENT_TYPE);
    parts[2].flags = 1;
    assert_eq!(
        apply_template(&export("a\n"), &archive(&parts)),
        Err(WordError::Encrypted)
    );
    let mut protected = crate::package::OLE_MAGIC.to_vec();
    protected.extend("xEncryptedPackagey".encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(apply_template(&export("a\n"), &protected), Err(WordError::Encrypted));
    let macros = archive(&template_parts(
        "application/vnd.ms-word.template.macroEnabledTemplate.main+xml",
    ));
    assert_eq!(apply_template(&export("a\n"), &macros), Err(WordError::NotAWordFile));
    let macros = archive(&template_parts(
        "application/vnd.ms-word.document.macroEnabled.main+xml",
    ));
    assert_eq!(apply_template(&export("a\n"), &macros), Err(WordError::NotAWordFile));
}

#[test]
fn damage_is_called_damage() {
    let generated = export("a\n");
    for part in [
        "[Content_Types].xml",
        "word/document.xml",
        "word/_rels/document.xml.rels",
        "word/styles.xml",
        "word/numbering.xml",
        "word/footnotes.xml",
    ] {
        let mut parts = template_parts(DOCUMENT_TYPE);
        set_part(&mut parts, part, "<not xml");
        assert_eq!(
            apply_template(&generated, &archive(&parts)),
            Err(WordError::Damaged),
            "{part}"
        );
    }
    let mut parts = template_parts(DOCUMENT_TYPE);
    without_part(&mut parts, "word/document.xml");
    assert_eq!(apply_template(&generated, &archive(&parts)), Err(WordError::Damaged));
    for main in ["<a/>", &format!("<w:document {W}/>")] {
        let mut parts = template_parts(DOCUMENT_TYPE);
        set_part(&mut parts, "word/document.xml", main);
        assert_eq!(
            apply_template(&generated, &archive(&parts)),
            Err(WordError::Damaged),
            "{main}"
        );
    }
    let mut parts = template_parts(DOCUMENT_TYPE);
    set_part(&mut parts, "word/_rels/document.xml.rels", "\u{0}");
    let mut broken = archive(&parts);
    assert_eq!(apply_template(&generated, &broken), Err(WordError::Damaged));
    let cut = broken.len() - 3;
    broken.truncate(cut);
    assert_eq!(apply_template(&generated, &broken), Err(WordError::NotAWordFile));
    let mut wrong_sum = template_parts(DOCUMENT_TYPE);
    wrong_sum[6].crc ^= 1;
    assert_eq!(
        apply_template(&generated, &archive(&wrong_sum)),
        Err(WordError::Damaged)
    );
}

#[test]
fn what_is_too_large_is_said_to_be() {
    let generated = export("a\n");
    let mut parts = template_parts(DOCUMENT_TYPE);
    parts[2].size = 200 << 20;
    assert_eq!(apply_template(&generated, &archive(&parts)), Err(WordError::TooLarge));
    let mut parts = template_parts(DOCUMENT_TYPE);
    parts[2].size = u32::MAX;
    assert_eq!(
        apply_template(&generated, &archive(&parts)),
        Err(WordError::TooLarge),
        "the 64-bit extension"
    );
    let mut parts = template_parts(DOCUMENT_TYPE);
    let media = parts.iter_mut().find(|p| p.name == "word/media/image1.png").unwrap();
    *media = deflated("word/media/image1.png", b"x");
    media.size = u32::try_from(MOST_ENTRY_BYTES).unwrap() + 1;
    assert_eq!(
        apply_template(&generated, &archive(&parts)),
        Err(WordError::TooLarge),
        "a part beyond its limit"
    );
}

#[test]
fn both_files_of_a_merge_share_one_budget() {
    let generated = export(&"Absatz\n\n".repeat(2000));
    let template = full_template();
    let budget = XmlBudget::default();
    assert!(apply_within(&generated, &template, &budget).is_ok());
    let used = XmlBudget::default().nodes_left() - budget.nodes_left();
    let short = || XmlBudget::new(crate::xml::MOST_XML_BYTES, used - 1);
    assert!(
        crate::docx::import_within(&generated, "x", &short()).is_ok(),
        "the generated file alone fits"
    );
    assert_eq!(apply_within(&generated, &template, &short()), Err(WordError::TooLarge));
}

#[test]
fn an_error_converts_into_what_the_app_knows() {
    assert_eq!(WordError::from(XmlError::TooLarge), WordError::TooLarge);
    assert_eq!(WordError::from(XmlError::Damaged), WordError::Damaged);
    assert_eq!(WordError::from(TooLarge), WordError::TooLarge);
    assert_eq!(WordError::from(ZipError::EntryTooLarge), WordError::TooLarge);
}
