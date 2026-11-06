//! Deep nesting and long documents.

use super::*;

#[test]
fn nesting_too_deep_is_read_as_text_without_a_stack() {
    let depth = 100_000;
    let thread = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            for (open, close) in [
                ("<w:sdt><w:sdtContent>", "</w:sdtContent></w:sdt>"),
                ("<w:ins>", "</w:ins>"),
                ("<w:customXml>", "</w:customXml>"),
                ("<w:tbl><w:tr><w:tc>", "</w:tc></w:tr></w:tbl>"),
                (
                    "<w:p><w:r><w:drawing><w:txbxContent>",
                    "</w:txbxContent></w:drawing></w:r></w:p>",
                ),
                ("<mc:AlternateContent><mc:Choice>", "</mc:Choice></mc:AlternateContent>"),
            ] {
                let body = format!("{}{}{}", open.repeat(depth), p(&t("tief")), close.repeat(depth));
                assert!(markdown(&body).contains("tief"), "{open}");
            }
            for (open, close) in [
                ("<w:ins>", "</w:ins>"),
                ("<w:hyperlink>", "</w:hyperlink>"),
                ("<w:fldSimple>", "</w:fldSimple>"),
                ("<mc:AlternateContent><mc:Choice>", "</mc:Choice></mc:AlternateContent>"),
            ] {
                let inner = format!("{}{}{}", open.repeat(depth), t("tief"), close.repeat(depth));
                assert_eq!(markdown(&p(&inner)), "tief\n", "{open}");
            }
            let math = format!(
                "{}<m:r><m:t>x</m:t></m:r>{}",
                "<m:f><m:num>".repeat(depth),
                "</m:num></m:f>".repeat(depth)
            );
            assert!(markdown(&p(&format!("<m:oMath>{math}</m:oMath>"))).contains('x'));
            let drawing = format!(
                "<w:r><w:drawing>{}<a:blip r:embed=\"x\"/>{}</w:drawing></w:r>",
                "<a:g>".repeat(depth),
                "</a:g>".repeat(depth)
            );
            assert_eq!(markdown(&p(&drawing)), "");
            let in_run = format!(
                "<w:r>{}<w:t>x</w:t>{}</w:r>",
                "<mc:AlternateContent><mc:Choice>".repeat(depth),
                "</mc:Choice></mc:AlternateContent>".repeat(depth)
            );
            assert_eq!(markdown(&p(&in_run)), "");
            let rows = format!(
                "<w:tbl>{}<w:tr><w:tc>{}</w:tc></w:tr>{}</w:tbl>",
                "<w:ins>".repeat(depth),
                p(&t("x")),
                "</w:ins>".repeat(depth)
            );
            assert_eq!(markdown(&rows), "");
            let cells = format!(
                "<w:tbl><w:tr>{}<w:tc>{}</w:tc>{}</w:tr></w:tbl>",
                "<w:ins>".repeat(depth),
                p(&t("x")),
                "</w:ins>".repeat(depth)
            );
            assert_eq!(markdown(&cells), "");
        })
        .unwrap();
    thread.join().unwrap();
}

#[test]
fn long_documents_take_no_quadratic_time() {
    let _limit = splat_testkit::time_limit(std::time::Duration::from_secs(120));
    let count = 200_000;
    let fixture = Fixture {
        styles: Some({
            let mut styles: String = (0..20_000)
                .map(|n| {
                    format!(
                        "<w:style w:styleId=\"s{n}\"><w:name w:val=\"x\"/><w:basedOn w:val=\"s{}\"/></w:style>",
                        n + 1
                    )
                })
                .collect();
            styles.push_str(&style("s20000", "Quote", ""));
            styles
        }),
        footnotes: Some(format!("<w:footnote w:id=\"1\">{}</w:footnote>", p(&t("Fuß")))),
        rels: vec![("rId1", "a.png", false)],
        parts: vec![("word/a.png", PNG.to_vec())],
        ..Fixture::default()
    };
    let cell = format!("<w:tc>{}</w:tc>", p(&t("x")));
    for body in [
        p(&t("x").repeat(count)),
        p(&format!(
            "<w:hyperlink w:anchor=\"a\">{}</w:hyperlink>",
            marked("<w:b/>", "x")
        ))
        .repeat(count / 4),
        p(&t("x")).repeat(count),
        format!(
            "<w:p><w:pPr><w:numPr><w:numId w:val=\"1\"/></w:numPr></w:pPr>{}</w:p>",
            t("x")
        )
        .repeat(count / 2),
        format!(
            "<w:tbl><w:tr>{}</w:tr><w:tr>{}</w:tr></w:tbl>",
            cell.repeat(count / 2),
            cell
        ),
        format!(
            "<w:tbl>{}</w:tbl>",
            format!("<w:tr>{cell}{cell}</w:tr>").repeat(count / 4)
        ),
        p(&"<w:r><w:footnoteReference w:id=\"1\"/></w:r>".repeat(count)),
        p(&"<w:r><w:drawing><a:blip r:embed=\"rId1\"/></w:drawing></w:r>".repeat(count)),
        p(&"<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>".repeat(count)),
        p(&format!(
            "<w:r><w:fldChar w:fldCharType=\"begin\"/><w:instrText>{}</w:instrText></w:r>{}",
            "x ".repeat(20_000),
            "<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>".repeat(count / 2)
        )),
        styled("s0", &t("x")).repeat(count / 2),
        p(&format!(
            "<m:oMath>{}</m:oMath>",
            "<m:r><m:t>α</m:t></m:r>".repeat(count)
        )),
    ] {
        assert!(import(&fixture.file(&body), "b").is_ok());
    }
}
