//! What cannot be merged, and what it is said to be.

use super::*;

#[test]
fn what_is_no_word_file_is_refused() {
    let generated = export("a\n");
    let template = full_template();
    assert_eq!(apply_template(&generated, b"plain"), Err(TemplateError::NotAWordFile));
    assert_eq!(apply_template(b"plain", &template), Err(TemplateError::NotAWordFile));
    let mut ole = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1".to_vec();
    ole.extend_from_slice(&[0; 600]);
    assert_eq!(apply_template(&generated, &ole), Err(TemplateError::NotAWordFile));
    let no_types = archive(&[stored("word/document.xml", b"<a/>")]);
    assert_eq!(apply_template(&generated, &no_types), Err(TemplateError::NotAWordFile));
    let other = archive(&[stored(
        "[Content_Types].xml",
        format!("<Types {TYPES}><Override PartName=\"/x.xml\" ContentType=\"application/xml\"/><Other/></Types>")
            .as_bytes(),
    )]);
    assert_eq!(apply_template(&generated, &other), Err(TemplateError::NotAWordFile));
}

#[test]
fn an_encrypted_archive_and_a_template_with_macros_are_refused() {
    let mut parts = template_parts(DOCUMENT_TYPE);
    parts[2].flags = 1;
    assert_eq!(
        apply_template(&export("a\n"), &archive(&parts)),
        Err(TemplateError::Encrypted)
    );
    let mut protected = crate::package::OLE_MAGIC.to_vec();
    protected.extend("xEncryptedPackagey".encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(
        apply_template(&export("a\n"), &protected),
        Err(TemplateError::Encrypted)
    );
    let macros = archive(&template_parts(
        "application/vnd.ms-word.template.macroEnabledTemplate.main+xml",
    ));
    assert_eq!(
        apply_template(&export("a\n"), &macros),
        Err(TemplateError::NotAWordFile)
    );
    let macros = archive(&template_parts(
        "application/vnd.ms-word.document.macroEnabled.main+xml",
    ));
    assert_eq!(
        apply_template(&export("a\n"), &macros),
        Err(TemplateError::NotAWordFile)
    );
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
            Err(TemplateError::Damaged),
            "{part}"
        );
    }
    let mut parts = template_parts(DOCUMENT_TYPE);
    without_part(&mut parts, "word/document.xml");
    assert_eq!(
        apply_template(&generated, &archive(&parts)),
        Err(TemplateError::Damaged)
    );
    for main in ["<a/>", &format!("<w:document {W}/>")] {
        let mut parts = template_parts(DOCUMENT_TYPE);
        set_part(&mut parts, "word/document.xml", main);
        assert_eq!(
            apply_template(&generated, &archive(&parts)),
            Err(TemplateError::Damaged),
            "{main}"
        );
    }
    let mut parts = template_parts(DOCUMENT_TYPE);
    set_part(&mut parts, "word/_rels/document.xml.rels", "\u{0}");
    let mut broken = archive(&parts);
    assert_eq!(apply_template(&generated, &broken), Err(TemplateError::Damaged));
    let cut = broken.len() - 3;
    broken.truncate(cut);
    assert_eq!(apply_template(&generated, &broken), Err(TemplateError::NotAWordFile));
    let mut wrong_sum = template_parts(DOCUMENT_TYPE);
    wrong_sum[6].crc ^= 1;
    assert_eq!(
        apply_template(&generated, &archive(&wrong_sum)),
        Err(TemplateError::Damaged)
    );
}

#[test]
fn what_is_too_large_is_said_to_be() {
    let generated = export("a\n");
    let mut parts = template_parts(DOCUMENT_TYPE);
    parts[2].size = 200 << 20;
    assert_eq!(
        apply_template(&generated, &archive(&parts)),
        Err(TemplateError::TooLarge)
    );
    let mut parts = template_parts(DOCUMENT_TYPE);
    parts[2].size = u32::MAX;
    assert_eq!(
        apply_template(&generated, &archive(&parts)),
        Err(TemplateError::TooLarge),
        "the 64-bit extension"
    );
    let mut parts = template_parts(DOCUMENT_TYPE);
    let media = parts.iter_mut().find(|p| p.name == "word/media/image1.png").unwrap();
    *media = deflated("word/media/image1.png", b"x");
    media.size = u32::try_from(MOST_ENTRY_BYTES).unwrap() + 1;
    assert_eq!(
        apply_template(&generated, &archive(&parts)),
        Err(TemplateError::TooLarge),
        "a part beyond its limit"
    );
}

#[test]
fn an_error_says_in_words_what_went_wrong() {
    let all = [
        TemplateError::NotAWordFile,
        TemplateError::Encrypted,
        TemplateError::TooLarge,
        TemplateError::Damaged,
    ];
    let words: HashSet<String> = all.iter().map(ToString::to_string).collect();
    assert_eq!(words.len(), all.len());
    let error: Box<dyn std::error::Error> = Box::new(TemplateError::Encrypted);
    assert!(error.to_string().contains("password"));
}

#[test]
fn an_error_converts_into_what_the_app_knows() {
    assert_eq!(TemplateError::from(XmlError::TooLarge), TemplateError::TooLarge);
    assert_eq!(TemplateError::from(XmlError::Damaged), TemplateError::Damaged);
    assert_eq!(TemplateError::from(TooLarge), TemplateError::TooLarge);
}
