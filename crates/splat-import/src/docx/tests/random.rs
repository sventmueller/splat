//! Random mixes of elements and damaged bytes.

use splat_markdown::doc::{BlockKind, InlineKind, walk_blocks, walk_inlines};
use splat_testkit::Reporting;

use super::*;

const PIECES: &[&str] = &[
    "<w:p>",
    "</w:p>",
    "<w:r>",
    "</w:r>",
    "<w:t>",
    "</w:t>",
    "text",
    " ",
    "*",
    "<w:tbl>",
    "</w:tbl>",
    "<w:tr>",
    "</w:tr>",
    "<w:tc>",
    "</w:tc>",
    "<w:hyperlink w:anchor=\"a\">",
    "</w:hyperlink>",
    "<w:ins>",
    "</w:ins>",
    "<w:del>",
    "</w:del>",
    "<w:sdt>",
    "</w:sdt>",
    "<w:sdtContent>",
    "</w:sdtContent>",
    "<mc:AlternateContent>",
    "</mc:AlternateContent>",
    "<mc:Choice>",
    "</mc:Choice>",
    "<mc:Fallback>",
    "</mc:Fallback>",
    "<w:rPr><w:b/></w:rPr>",
    "<w:rPr><w:i/></w:rPr>",
    "<w:pPr><w:numPr><w:ilvl w:val=\"1\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr>",
    "<w:pPr><w:pStyle w:val=\"H\"/></w:pPr>",
    "<w:pPr><w:pStyle w:val=\"Q\"/></w:pPr>",
    "<w:pPr><w:pStyle w:val=\"C\"/></w:pPr>",
    "<w:pPr><w:pBdr><w:top w:val=\"single\"/></w:pBdr></w:pPr>",
    "<w:pPr><w:pBdr><w:bottom w:val=\"single\"/></w:pBdr></w:pPr>",
    "<w:pPr><w:pStyle w:val=\"F\"/></w:pPr>",
    "<w:pPr><w:ind w:left=\"720\"/></w:pPr>",
    "<w:br/>",
    "<w:tab/>",
    "<w:fldChar w:fldCharType=\"begin\"/>",
    "<w:fldChar w:fldCharType=\"separate\"/>",
    "<w:fldChar w:fldCharType=\"end\"/>",
    "<w:instrText>HYPERLINK \"a\"</w:instrText>",
    "<w:instrText>TOC</w:instrText>",
    "<w:footnoteReference w:id=\"1\"/>",
    "<w:drawing>",
    "</w:drawing>",
    "<w:txbxContent>",
    "</w:txbxContent>",
    "<a:blip r:embed=\"rId1\"/>",
    "<m:oMath>",
    "</m:oMath>",
    "<m:oMathPara>",
    "</m:oMathPara>",
    "<m:f>",
    "</m:f>",
    "<m:num>",
    "</m:num>",
    "<m:r><m:t>x</m:t></m:r>",
];

fn random_body(rng: &mut Rng) -> String {
    let mut body = String::new();
    let mut open: Vec<&str> = Vec::new();
    for _ in 0..rng.below(40) {
        let piece: &str = rng.pick(PIECES);
        if let Some(name) = piece.strip_prefix("</") {
            let name = &name[..name.len() - 1];
            if let Some(at) = open.iter().rposition(|tag| *tag == name) {
                for tag in open.drain(at..).rev() {
                    body.push_str(&format!("</{tag}>"));
                }
            }
        } else if piece.starts_with('<') && !piece.ends_with("/>") && !piece.contains("</") {
            let name = piece[1..piece.len() - 1].split(' ').next().unwrap_or("");
            open.push(name);
            body.push_str(piece);
        } else {
            body.push_str(piece);
        }
    }
    for tag in open.iter().rev() {
        body.push_str(&format!("</{tag}>"));
    }
    body
}

/// Whether Markdown holds a formula or a link to a bookmark, which Splat's export writes differently.
fn holds_formula_or_bookmark_link(markdown: &str) -> bool {
    let mut found = false;
    walk_blocks(read_markdown(markdown).blocks(), &mut |block| {
        found |= matches!(block.kind, BlockKind::MathBlock { .. });
        walk_inlines(block.inlines(), &mut |inline| {
            found |= match &inline.kind {
                InlineKind::Math { .. } => true,
                InlineKind::Link { url, .. } => url.starts_with('#'),
                _ => false,
            };
        });
    });
    found
}

/// Markdown with the numbers taken out of the names of its pictures.
fn without_picture_numbers(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut rest = markdown;
    while let Some(at) = rest.find("media/image") {
        let (before, after) = rest.split_at(at + "media/image".len());
        out.push_str(before);
        rest = after.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    out.push_str(rest);
    out
}

#[test]
fn any_mix_of_elements_gives_markdown_that_can_be_read() {
    let fixture = Fixture {
        styles: Some(format!(
            "{}{}{}{}",
            style("H", "heading 1", ""),
            style("Q", "Quote", ""),
            style("C", "Code", ""),
            style("F", "footnote text", "")
        )),
        numbering: Some(
            "<w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"1\"><w:numFmt w:val=\"decimal\"/></w:lvl></w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>"
                .to_string(),
        ),
        footnotes: Some(format!("<w:footnote w:id=\"1\">{}</w:footnote>", p(&t("Fuß")))),
        rels: vec![("rId1", "media/a.png", false)],
        parts: vec![("word/media/a.png", PNG.to_vec())],
        ..Fixture::default()
    };
    let mut rng = Rng::new(0x57A7_D0C5);
    for _ in 0..rounds(4000) {
        let body = random_body(&mut rng);
        let _reporting = Reporting(&body);
        let first = import(&fixture.file(&body), "media")
            .unwrap_or_else(|e| panic!("{e:?}"))
            .markdown;
        let again = round_trip(&first);
        // The export writes a picture once for each place it stands, which the import numbers anew. Without a renderer
        // it writes a formula as code, and a link to a bookmark only where a heading carries that bookmark, so what
        // holds either changes once and then stays.
        if holds_formula_or_bookmark_link(&first) {
            assert_eq!(
                without_picture_numbers(&round_trip(&again)),
                without_picture_numbers(&again)
            );
        } else {
            assert_eq!(without_picture_numbers(&again), without_picture_numbers(&first));
        }
    }
}

#[test]
fn any_bytes_end_in_a_result() {
    let mut rng = Rng::new(0x57A7_D0C6);
    let fixture = Fixture::default();
    let good = fixture.file(&p(&t("Hallo")));
    for _ in 0..rounds(2000) {
        let mut bytes = good.clone();
        for _ in 0..1 + rng.below(8) {
            let at = rng.below(bytes.len());
            bytes[at] = u8::try_from(rng.below(256)).unwrap_or(0);
        }
        // Each part carries its checksum, so damage either shows or leaves the text as it was.
        if let Ok(imported) = import(&bytes, "x") {
            assert_eq!(imported.markdown, "Hallo\n");
        }
    }
}
