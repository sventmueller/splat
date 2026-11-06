//! Pictures and formulas.

use super::*;

#[test]
fn pictures_are_named_by_a_counter_and_their_first_bytes() {
    let fixture = Fixture {
        rels: vec![
            ("rId1", "media/../../../../evil name.png", false),
            ("rId2", "media/b.bin", false),
            ("rId3", "media/c.txt", false),
            ("rId4", "https://a.bc/p.png", true),
            ("rId5", "media/missing.png", false),
            ("rId6", "/word/media/d.emf", false),
        ],
        parts: vec![
            ("evil name.png", JPEG.to_vec()),
            ("word/media/b.bin", PNG.to_vec()),
            ("word/media/c.txt", b"plain text".to_vec()),
            ("word/media/d.emf", {
                let mut emf = vec![1, 0, 0, 0];
                emf.extend_from_slice(&[0; 36]);
                emf.extend_from_slice(b" EMF");
                emf
            }),
        ],
        ..Fixture::default()
    };
    let drawing = |id: &str, description: &str| {
        format!(
            "<w:r><w:drawing><wp:inline><wp:docPr id=\"1\" name=\"Bild\" descr=\"{description}\"/><a:graphic><a:graphicData><a:blip r:embed=\"{id}\"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"
        )
    };
    let imported = import(
        &fixture.file(&format!(
            "<w:p>{}</w:p><w:p>{}{}</w:p><w:p>{}</w:p><w:p>{}</w:p><w:p>{}</w:p><w:p>{}</w:p><w:p>{}</w:p>",
            drawing("rId1", "Eins"),
            drawing("rId2", "Zwei"),
            drawing("rId1", "Wieder"),
            drawing("rId3", "Text"),
            drawing("rId4", "Fern"),
            drawing("rId5", "Fehlt"),
            drawing("rId6", "Vektor"),
            drawing("rId7", "Unbekannt"),
        )),
        "Meine Bilder/",
    )
    .unwrap();
    assert_eq!(
        imported.markdown,
        "![Eins](Meine%20Bilder/image1.jpeg)\n\n![Zwei](Meine%20Bilder/image2.png)![Wieder](Meine%20Bilder/image1.jpeg)\n\nText\n\nFern\n\nFehlt\n\n![Vektor](Meine%20Bilder/image3.emf)\n\nUnbekannt\n"
    );
    let names: Vec<&str> = imported.media.iter().map(|media| media.file_name.as_str()).collect();
    assert_eq!(names, ["image1.jpeg", "image2.png", "image3.emf"]);
    assert_eq!(imported.media[0].bytes, JPEG);
    let missing: Vec<(&str, MissingReason)> = imported
        .missing
        .iter()
        .map(|missing| (missing.part.as_str(), missing.reason))
        .collect();
    assert_eq!(
        missing,
        [
            ("word/media/c.txt", MissingReason::UnknownFormat),
            ("https://a.bc/p.png", MissingReason::NoPart),
            ("word/media/missing.png", MissingReason::Absent),
            ("rId7", MissingReason::NoPart),
        ]
    );
    assert_eq!(
        import(&fixture.file(&p(&drawing("rId2", "x"))), "").unwrap().markdown,
        "![x](image1.png)\n"
    );
}

#[test]
fn pictures_of_older_files_and_descriptions_are_found_too() {
    let fixture = Fixture {
        rels: vec![("rId1", "media/a.png", false)],
        parts: vec![("word/media/a.png", PNG.to_vec())],
        ..Fixture::default()
    };
    assert_eq!(
        fixture.markdown(&p(
            "<w:r><w:pict><v:shape alt=\"Alt\"><v:imagedata r:id=\"rId1\"/></v:shape></w:pict></w:r>"
        )),
        "![Alt](images/image1.png)\n"
    );
    assert_eq!(
        fixture.markdown(&p(
            "<w:r><w:object><v:shape><v:imagedata r:id=\"rId1\"/></v:shape></w:object></w:r>"
        )),
        "![](images/image1.png)\n"
    );
    assert_eq!(
        fixture.markdown(&p(
            "<w:r><w:drawing><wp:docPr title=\"Titel\" descr=\" \"/><mc:AlternateContent><mc:Choice><a:blip r:embed=\"rId1\"/></mc:Choice></mc:AlternateContent></w:drawing></w:r>"
        )),
        "![Titel](images/image1.png)\n"
    );
    assert_eq!(
        fixture.markdown(&p(
            "<w:r><w:drawing><wp:docPr descr=\"Diagramm\"/><a:blip/></w:drawing></w:r>"
        )),
        "Diagramm\n",
        "a drawing that is no picture leaves its description"
    );
    assert_eq!(
        fixture.markdown(
            "<w:p><w:hyperlink w:anchor=\"x\"><w:r><w:drawing><a:blip r:embed=\"rId1\"/></w:drawing></w:r></w:hyperlink></w:p>"
        ),
        "[![](images/image1.png)](#x)\n"
    );
    assert_eq!(
        fixture.markdown(&p(
            "<w:r><w:rPr><w:vanish/></w:rPr><w:drawing><a:blip r:embed=\"rId1\"/></w:drawing></w:r>"
        )),
        ""
    );
}

#[test]
fn formulas_become_latex_on_lines_of_their_own_or_within_the_line() {
    let formula = "<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>";
    assert_eq!(markdown(&p(&format!("{}{formula}", t("a ")))), "a $x$\n");
    assert_eq!(
        markdown(&p(&format!(
            "<m:oMathPara><m:oMathParaPr/>{formula}{formula}</m:oMathPara>"
        ))),
        "$$x \\\\ x$$\n"
    );
    assert_eq!(
        markdown(&p(&format!("{}<m:oMathPara>{formula}</m:oMathPara>", t("a ")))),
        "a $x$\n"
    );
    assert_eq!(
        markdown(&format!(
            "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>{formula}<m:oMathPara>{formula}</m:oMathPara></w:p>"
        )),
        ""
    );
}

#[test]
fn a_picture_left_out_is_named_once_with_the_reason() {
    let fixture = Fixture {
        rels: vec![("rId1", "media/gone.png", false)],
        ..Fixture::default()
    };
    let drawing = |id: &str| format!("<w:r><w:drawing><a:blip r:embed=\"{id}\"/></w:drawing></w:r>");
    let body = p(&[drawing("rId1"), drawing("rId1"), drawing("rId9"), drawing("rId9")].concat());
    let imported = import(&fixture.file(&body), "x").unwrap();
    assert_eq!(
        imported.missing,
        [
            MissingPicture {
                part: "word/media/gone.png".to_string(),
                reason: MissingReason::Absent,
            },
            MissingPicture {
                part: "rId9".to_string(),
                reason: MissingReason::NoPart,
            },
        ]
    );
    assert_eq!(
        imported.missing[0].to_string(),
        "picture word/media/gone.png: not in the file"
    );
    for reason in [
        MissingReason::NoPart,
        MissingReason::Damaged,
        MissingReason::TooLarge,
        MissingReason::UnknownFormat,
    ] {
        assert!(!reason.to_string().is_empty());
    }
}
