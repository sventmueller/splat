//! Which files read as Word files, and what the others are said to be.

use super::*;
use crate::package::OLE_MAGIC;
use crate::xml::{MOST_XML_BYTES, XmlBudget};
use splat_ooxml::zip::MOST_ENTRY_BYTES;

#[test]
fn a_strict_file_reads_as_a_usual_one() {
    let strict = "<w:document xmlns:w=\"http://purl.oclc.org/ooxml/wordprocessingml/main\"><w:body>\
        <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Fett</w:t></w:r></w:p></w:body></w:document>";
    let part = stored("word/document.xml", strict.as_bytes());
    let imported = import(&with_types(WORD_TYPE, &[part]), "images").expect("a word file");
    assert_eq!(imported.markdown, "**Fett**\n");
}

#[test]
fn what_cannot_be_read_says_why() {
    let import_of = |bytes: &[u8]| import(bytes, "x").map(|imported| imported.markdown);
    assert_eq!(import_of(b"plain text"), Err(WordError::NotAWordFile));
    assert_eq!(import_of(b""), Err(WordError::NotAWordFile));
    let mut old = OLE_MAGIC.to_vec();
    old.extend_from_slice(b"WordDocument");
    assert_eq!(import_of(&old), Err(WordError::NotAWordFile));
    let mut encrypted = OLE_MAGIC.to_vec();
    encrypted.extend("xEncryptedPackagey".encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(import_of(&encrypted), Err(WordError::Encrypted));

    let mut secret = stored("word/document.xml", b"x");
    secret.flags = 1;
    assert_eq!(import_of(&archive(&[secret])), Err(WordError::Encrypted));
    let mut big = stored("a", b"x");
    big.size = u32::MAX;
    assert_eq!(
        import_of(&archive(&[big])),
        Err(WordError::TooLarge),
        "the 64-bit extension"
    );
    let mut wrong = stored("a", b"x");
    wrong.stored = b"xx".to_vec();
    assert_eq!(import_of(&archive(&[wrong])), Err(WordError::Damaged));
}

fn with_types(types: &str, parts: &[Part]) -> Vec<u8> {
    let mut all = vec![stored("[Content_Types].xml", types.as_bytes())];
    all.extend_from_slice(parts);
    archive(&all)
}

const WORD_TYPE: &str = "<Types><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";

#[test]
fn another_kind_of_archive_is_no_word_file() {
    let import_of = |bytes: &[u8]| import(bytes, "x").map(|imported| imported.markdown);
    assert_eq!(
        import_of(&archive(&[stored("mimetype", b"application/epub+zip")])),
        Err(WordError::NotAWordFile)
    );
    let sheet = "<Types><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/></Types>";
    assert_eq!(import_of(&with_types(sheet, &[])), Err(WordError::NotAWordFile));
    assert_eq!(import_of(&with_types("<Types", &[])), Err(WordError::Damaged));
    assert_eq!(
        import_of(&with_types("<Types>\u{e9}</Types>", &[])),
        Err(WordError::NotAWordFile)
    );
    let invalid = archive(&[stored("[Content_Types].xml", b"\xFF")]);
    assert_eq!(import_of(&invalid), Err(WordError::Damaged));
}

#[test]
fn damage_inside_the_archive_is_found() {
    let import_of = |bytes: &[u8]| import(bytes, "x").map(|imported| imported.markdown);
    assert_eq!(
        import_of(&with_types(WORD_TYPE, &[])),
        Err(WordError::Damaged),
        "no main part"
    );
    let document = |xml: &str| stored("word/document.xml", xml.as_bytes());
    assert_eq!(
        import_of(&with_types(WORD_TYPE, &[document("<a>")])),
        Err(WordError::Damaged)
    );
    assert_eq!(
        import_of(&with_types(WORD_TYPE, &[stored("word/document.xml", b"\xFF\xFE")])),
        Err(WordError::Damaged)
    );
    let no_body = wrapped("document", "");
    assert_eq!(
        import_of(&with_types(WORD_TYPE, &[document(&no_body)])),
        Err(WordError::Damaged)
    );
    let body = wrapped("document", "<w:body/>");
    let bad_rels = stored("word/_rels/document.xml.rels", b"<Relationships");
    assert_eq!(
        import_of(&with_types(WORD_TYPE, &[document(&body), bad_rels])),
        Err(WordError::Damaged)
    );
    let invalid_rels = stored("word/_rels/document.xml.rels", b"\xFF");
    assert_eq!(
        import_of(&with_types(WORD_TYPE, &[document(&body), invalid_rels])),
        Err(WordError::Damaged)
    );
    assert_eq!(import_of(&with_types(WORD_TYPE, &[document(&body)])), Ok(String::new()));

    let rels = |kind: &str| {
        stored(
            "word/_rels/document.xml.rels",
            format!("<Relationships><Relationship Id=\"a\" Type=\"urn:x/{kind}\" Target=\"p.xml\"/></Relationships>")
                .as_bytes(),
        )
    };
    for kind in ["styles", "numbering", "footnotes", "endnotes"] {
        let bad = with_types(WORD_TYPE, &[document(&body), rels(kind), stored("word/p.xml", b"<a>")]);
        assert_eq!(import_of(&bad), Err(WordError::Damaged), "{kind}");
        let invalid = with_types(WORD_TYPE, &[document(&body), rels(kind), stored("word/p.xml", b"\xFF")]);
        assert_eq!(import_of(&invalid), Err(WordError::Damaged), "{kind}");
        let absent = with_types(WORD_TYPE, &[document(&body), rels(kind)]);
        assert_eq!(import_of(&absent), Ok(String::new()), "{kind}");
    }
}

#[test]
fn a_main_part_too_large_is_refused_before_it_is_inflated() {
    let mut huge = deflated("word/document.xml", b"x");
    huge.size = u32::try_from(MOST_XML_BYTES).unwrap() + 1;
    assert_eq!(import(&with_types(WORD_TYPE, &[huge]), "x"), Err(WordError::TooLarge));
}

#[test]
fn all_parts_of_a_file_share_one_budget_of_nodes() {
    let used = |fixture: &Fixture| {
        let budget = XmlBudget::default();
        import_within(&fixture.file(&p(&t("x")).repeat(1000)), "x", &budget).expect("a word file");
        XmlBudget::default().nodes_left() - budget.nodes_left()
    };
    let document_alone = used(&Fixture::default());
    let with_notes = Fixture {
        footnotes: Some(format!(
            "<w:footnote w:id=\"1\">{}</w:footnote>",
            p(&t("y")).repeat(1000)
        )),
        ..Fixture::default()
    };
    let together = used(&with_notes);
    assert!(together > 2 * document_alone - 100, "{document_alone} {together}");
    let budget = XmlBudget::new(MOST_XML_BYTES, together - 1);
    let file = with_notes.file(&p(&t("x")).repeat(1000));
    assert_eq!(
        import_within(&file, "x", &budget).map(|imported| imported.markdown),
        Err(WordError::TooLarge),
        "each part alone fits into what is left"
    );
}

#[test]
fn pictures_over_the_budget_end_the_import() {
    let one = deflated("word/media/0.png", &vec![0u8; 200 << 20]);
    let mut parts = vec![stored(
        "word/document.xml",
        wrapped(
            "document",
            &format!(
                "<w:body><w:p>{}</w:p></w:body>",
                (0..6)
                    .map(|n| format!("<w:r><w:drawing><a:blip r:embed=\"rId{n}\"/></w:drawing></w:r>"))
                    .collect::<String>()
            ),
        )
        .as_bytes(),
    )];
    let mut rels = String::new();
    for n in 0..6 {
        rels.push_str(&format!("<Relationship Id=\"rId{n}\" Target=\"media/{n}.png\"/>"));
        parts.push(Part {
            name: format!("word/media/{n}.png"),
            ..one.clone()
        });
    }
    parts.push(stored(
        "word/_rels/document.xml.rels",
        format!("<Relationships>{rels}</Relationships>").as_bytes(),
    ));
    assert_eq!(import(&with_types(WORD_TYPE, &parts), "x"), Err(WordError::TooLarge));
}

#[test]
fn a_picture_too_large_on_its_own_goes_missing_and_the_import_goes_on() {
    let mut huge = deflated("word/media/a.png", PNG);
    huge.size = u32::try_from(MOST_ENTRY_BYTES).unwrap() + 1;
    let parts = [
        stored(
            "word/document.xml",
            wrapped(
                "document",
                &format!(
                    "<w:body><w:p><w:r><w:drawing><wp:docPr descr=\"Riesig\"/><a:blip r:embed=\"rId1\"/></w:drawing></w:r>\
                     <w:r><w:drawing><a:blip r:embed=\"rId2\"/></w:drawing></w:r></w:p>{}</w:body>",
                    p(&t("danach"))
                ),
            )
            .as_bytes(),
        ),
        stored(
            "word/_rels/document.xml.rels",
            b"<Relationships><Relationship Id=\"rId1\" Target=\"media/a.png\"/>\
              <Relationship Id=\"rId2\" Target=\"media/b.png\"/></Relationships>",
        ),
        huge,
        stored("word/media/b.png", PNG),
    ];
    let imported = import(&with_types(WORD_TYPE, &parts), "x").expect("the rest of the file");
    assert_eq!(imported.markdown, "Riesig![](x/image1.png)\n\ndanach\n");
    assert_eq!(imported.media.len(), 1);
    assert_eq!(
        imported.missing,
        [MissingPicture {
            part: "word/media/a.png".to_string(),
            reason: MissingReason::TooLarge,
        }]
    );
}

#[test]
fn a_damaged_picture_leaves_its_description() {
    let mut damaged = stored("word/media/a.png", PNG);
    damaged.crc ^= 1;
    let parts = [
        stored(
            "word/document.xml",
            wrapped(
                "document",
                "<w:body><w:p><w:r><w:drawing><wp:docPr descr=\"Bild\"/><a:blip r:embed=\"rId1\"/></w:drawing></w:r></w:p></w:body>",
            )
            .as_bytes(),
        ),
        stored(
            "word/_rels/document.xml.rels",
            b"<Relationships><Relationship Id=\"rId1\" Target=\"media/a.png\"/></Relationships>",
        ),
        damaged,
    ];
    let imported = import(&with_types(WORD_TYPE, &parts), "x").unwrap();
    assert_eq!((imported.markdown.as_str(), imported.media.len()), ("Bild\n", 0));
    assert_eq!(
        imported.missing,
        [MissingPicture {
            part: "word/media/a.png".to_string(),
            reason: MissingReason::Damaged,
        }]
    );
}

#[test]
fn an_error_says_in_words_what_went_wrong() {
    let all = [
        WordError::NotAWordFile,
        WordError::Encrypted,
        WordError::TooLarge,
        WordError::Damaged,
    ];
    let words: std::collections::HashSet<String> = all.iter().map(ToString::to_string).collect();
    assert_eq!(words.len(), all.len());
    let error: Box<dyn std::error::Error> = Box::new(WordError::Damaged);
    assert_eq!(error.to_string(), "the Word file is damaged");
}

#[test]
fn a_template_and_a_macro_file_read_like_a_document() {
    for kind in [
        "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml",
        "application/vnd.ms-word.document.macroEnabled.main+xml",
        "application/vnd.ms-word.template.macroEnabledTemplate.main+xml",
    ] {
        let types = format!("<Types><Override PartName=\"/main.xml\" ContentType=\"{kind}\"/></Types>");
        let body = wrapped("document", &format!("<w:body>{}</w:body>", p(&t("a"))));
        let file = with_types(&types, &[stored("main.xml", body.as_bytes())]);
        assert_eq!(import(&file, "x").unwrap().markdown, "a\n", "{kind}");
    }
}

#[test]
fn another_prefix_means_the_same() {
    let body = format!(
        "<x:document xmlns:x=\"{W}\"><x:body><x:p><x:r><x:rPr><x:b/></x:rPr><x:t>a</x:t></x:r></x:p></x:body></x:document>"
    );
    let file = with_types(WORD_TYPE, &[stored("word/document.xml", body.as_bytes())]);
    assert_eq!(import(&file, "x").unwrap().markdown, "**a**\n");
    let foreign = "<w:document xmlns:w=\"urn:other\"><w:body/></w:document>";
    let file = with_types(WORD_TYPE, &[stored("word/document.xml", foreign.as_bytes())]);
    assert_eq!(import(&file, "x"), Err(WordError::Damaged));
}
