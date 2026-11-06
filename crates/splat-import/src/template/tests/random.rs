//! Random parts and damaged bytes.

use super::*;

#[test]
fn a_generated_file_may_be_anything_but_gives_no_panic() {
    let template = full_template();
    assert_eq!(apply_template(b"", &template), Err(WordError::NotAWordFile));
    let mut parts = template_parts(DOCUMENT_TYPE);
    without_part(&mut parts, "word/styles.xml");
    without_part(&mut parts, "word/numbering.xml");
    without_part(&mut parts, "word/_rels/document.xml.rels");
    set_part(
        &mut parts,
        "word/document.xml",
        &format!("<w:document {W}><w:body><w:p><w:pPr><w:pStyle w:val=\"X\"/></w:pPr></w:p></w:body></w:document>"),
    );
    let result = apply_template(&archive(&parts), &template).unwrap();
    let document = text_of_part(&result, "word/document.xml");
    assert!(
        document.contains("<w:pStyle w:val=\"X\"/>"),
        "an id without a definition stays what it is: {document}"
    );
}

const PIECES: &[&str] = &[
    "<w:p>",
    "</w:p>",
    "<w:r>",
    "</w:r>",
    "<w:t>Text</w:t>",
    "<w:pStyle w:val=\"Heading1\"/>",
    "<w:rStyle w:val=\"X\"/>",
    "<w:numPr><w:numId w:val=\"1\"/></w:numPr>",
    "<w:numId w:val=\"x\"/>",
    "<w:drawing r:embed=\"rIdI1\"/>",
    "<w:hyperlink r:id=\"rId1\">",
    "</w:hyperlink>",
    "<w:footnoteReference w:id=\"1\"/>",
    "<w:endnoteReference w:id=\"-1\"/>",
    "<w:sectPr>",
    "</w:sectPr>",
    "<w:sectPr/>",
    "<w:body>",
    "</w:body>",
    "<w:body/>",
    "<w:style w:styleId=\"A\"><w:name w:val=\"a\"/><w:basedOn w:val=\"A\"/></w:style>",
    "<w:style w:styleId=\"Heading1\"/>",
    "<w:abstractNum w:abstractNumId=\"99999999999\"/>",
    "<w:num w:numId=\"-3\"/>",
    "<w:footnote w:id=\"1\"/>",
    "<Relationship Id=\"rId1\" Type=\"x/hyperlink\" Target=\"y\" TargetMode=\"External\"/>",
    "<Relationship Id=\"rIdI1\" Type=\"x/image\" Target=\"media/image1.png\"/>",
    "<Relationship Id=\"rId4294967295\" Type=\"x/styles\" Target=\"..\"/>",
    "<Relationship Id=\"a\" Type=\"x/numbering\" Target=\"../../..\"/>",
    "<Relationship Id=\"b\" Type=\"x/footnotes\" Target=\"footnotes.xml\"/>",
    "<Default Extension=\"\" ContentType=\"\"/>",
    "<Override PartName=\"/word/document.xml\" ContentType=\"application/xml\"/>",
    "<!-- c -->",
    "<![CDATA[x]]>",
    "&amp;",
    "&bad;",
    " ",
    "\u{ffff}",
    "<",
    ">",
];

fn random_text(rng: &mut Rng) -> String {
    (0..rng.below(12)).map(|_| *rng.pick(PIECES)).collect()
}

fn roots(rng: &mut Rng, name: &str) -> String {
    let middle = random_text(rng);
    match rng.below(4) {
        0 => middle,
        1 => format!("<w:{name} {W} {R}>{middle}</w:{name}>"),
        2 => format!("<w:document {W} {R}><w:body>{middle}</w:body></w:document>"),
        _ => format!("<{name} {RELS}>{middle}</{name}>"),
    }
}

#[test]
fn random_parts_never_panic_and_what_comes_out_parses() {
    let mut rng = Rng::new(0x51EA_7E01);
    let names = [
        ("word/document.xml", "document"),
        ("word/_rels/document.xml.rels", "Relationships"),
        ("word/styles.xml", "styles"),
        ("word/numbering.xml", "numbering"),
        ("word/footnotes.xml", "footnotes"),
    ];
    let good_generated = export(SAMPLE);
    let read = Archive::read(&good_generated).unwrap();
    for _ in 0..rounds(3000) {
        let mut template = template_parts(DOCUMENT_TYPE);
        for (name, root) in names {
            if rng.below(3) == 0 {
                set_part(&mut template, name, &roots(&mut rng, root));
            }
        }
        let mut generated: Vec<Entry> = read
            .names()
            .map(|name| stored(name, &read.get(name).unwrap().unwrap()))
            .collect();
        generated.push(stored("word/footnotes.xml", b""));
        for (name, root) in names {
            if rng.below(3) == 0 {
                set_part(&mut generated, name, &roots(&mut rng, root));
            }
        }
        if let Ok(result) = apply_template(&archive(&generated), &archive(&template)) {
            for (name, content) in entries(&result) {
                if name == "word/document.xml" {
                    let text = std::str::from_utf8(&content).unwrap();
                    Document::parse(text).unwrap_or_else(|e| panic!("{e}: {text}"));
                }
            }
        }
    }
}

#[test]
fn damaged_bytes_never_panic() {
    let mut rng = Rng::new(0x51EA_7E02);
    let generated = export(SAMPLE);
    let template = full_template();
    for _ in 0..rounds(300) {
        let mut broken = template.clone();
        for _ in 0..1 + rng.below(8) {
            let at = rng.below(broken.len());
            broken[at] = u8::try_from(rng.below(256)).unwrap();
        }
        let _ = apply_template(&generated, &broken);
        let _ = apply_template(&broken, &template);
    }
}
