//! Tables, their merged cells and the alignment of their columns.

use super::*;

#[test]
fn tables_have_a_header_and_flatten_what_they_hold() {
    let cell = |inner: &str| format!("<w:tc>{inner}</w:tc>");
    let row = |cells: &[&str]| {
        let cells: String = cells.iter().map(|text| cell(&p(&t(text)))).collect();
        format!("<w:tr>{cells}</w:tr>")
    };
    assert_eq!(
        markdown(&format!(
            "<w:tbl><w:tblPr/><w:tblGrid/>{}{}</w:tbl>",
            row(&["A", "B"]),
            row(&["1", "2"])
        )),
        "| A | B |\n|---|---|\n| 1 | 2 |\n"
    );
    assert_eq!(
        markdown(&format!(
            "<w:tbl><w:tr>{}{}</w:tr>{}</w:tbl>",
            cell(&format!("{}{}", p(&t("x")), p(&t("y")))),
            cell(&format!(
                "<w:tbl><w:tr>{}{}</w:tr>{}</w:tbl>",
                cell(&p(&t("i1"))),
                cell(&p(&t("i2"))),
                row(&["i3", "i4"])
            )),
            row(&["1", "2"])
        )),
        "| x<br>y | i1 i2<br>i3 i4 |\n|---|---|\n| 1 | 2 |\n",
        "paragraphs and nested tables come down to lines"
    );
    assert_eq!(
        markdown(&format!(
            "<w:tbl><w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt><w:customXml>{}</w:customXml><w:ins><w:tr><w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt><w:customXml>{}</w:customXml></w:tr></w:ins></w:tbl>",
            row(&["A", "B"]),
            row(&["1", "2"]),
            cell(&p(&t("3"))),
            cell(&p(&t("4")))
        )),
        "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n"
    );
    assert_eq!(
        markdown(&format!(
            "<w:tbl><mc:AlternateContent><mc:Choice>{}</mc:Choice></mc:AlternateContent></w:tbl>",
            row(&["A", "B"]),
        )),
        "| A | B |\n|---|---|\n"
    );
    assert_eq!(
        markdown(&format!(
            "<w:tbl>{}<w:tr><w:trPr><w:del/></w:trPr>{}</w:tr>{}</w:tbl>",
            row(&["A", "B"]),
            cell(&p(&t("weg"))),
            row(&["1", "2"])
        )),
        "| A | B |\n|---|---|\n| 1 | 2 |\n",
        "a deleted row is gone"
    );
    assert_eq!(
        markdown(&format!(
            "<w:tbl><w:tr>{}</w:tr></w:tbl>",
            cell(&format!("{}{}", p(&t("a")), p(&t("b"))))
        )),
        "a\\\nb\n",
        "a table of one cell is a paragraph"
    );
    let item = |text: &str| {
        format!(
            "<w:p><w:pPr><w:numPr><w:numId w:val=\"5\"/></w:numPr></w:pPr>{}</w:p>",
            t(text)
        )
    };
    assert_eq!(
        markdown(&format!(
            "<w:tbl>{}<w:tr>{}{}</w:tr></w:tbl>",
            row(&["A", "B"]),
            cell(&format!("{}{}", item("a"), item("b"))),
            cell("<w:p/>")
        )),
        "| A | B |\n|---|---|\n| a<br>b | |\n"
    );
}

#[test]
fn merged_cells_make_an_html_table_or_empty_cells() {
    let cell = |props: &str, text: &str| format!("<w:tc><w:tcPr>{props}</w:tcPr>{}</w:tc>", p(&t(text)));
    assert_eq!(
        markdown(&format!(
            "<w:tbl><w:tr>{}{}</w:tr><w:tr>{}{}{}</w:tr></w:tbl>",
            cell("<w:gridSpan w:val=\"2\"/>", "breit"),
            cell("", "c"),
            cell("", "1"),
            cell("", "2"),
            cell("", "3"),
        )),
        "<table>\n<tr><th colspan=\"2\">breit</th><th>c</th></tr>\n<tr><td>1</td><td>2</td><td>3</td></tr>\n</table>\n"
    );
    assert_eq!(
        markdown(&format!(
            "<w:tbl><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>",
            cell("<w:vMerge w:val=\"restart\"/>", "a"),
            cell("", "b"),
            cell("<w:vMerge/>", "darunter"),
            cell("", "d"),
        )),
        "| a | b |\n|---|---|\n| | d |\n"
    );
    assert_eq!(
        markdown(&format!(
            "<w:tbl><w:tr>{}{}</w:tr><w:tr><w:trPr><w:gridBefore w:val=\"1\"/></w:trPr>{}</w:tr></w:tbl>",
            cell("", "a"),
            cell("", "b"),
            cell("", "d"),
        )),
        "| a | b |\n|---|---|\n| | d |\n"
    );
}

#[test]
fn columns_align_as_all_their_cells_do() {
    let cell = |jc: &str, text: &str| {
        format!(
            "<w:tc><w:p><w:pPr><w:jc w:val=\"{jc}\"/></w:pPr>{}</w:p></w:tc>",
            t(text)
        )
    };
    assert_eq!(
        markdown(&format!(
            "<w:tbl><w:tr>{}{}{}{}</w:tr><w:tr>{}{}{}{}</w:tr></w:tbl>",
            cell("center", "A"),
            cell("right", "B"),
            cell("end", "C"),
            cell("center", "D"),
            cell("center", "1"),
            cell("right", "2"),
            cell("end", ""),
            cell("left", "4"),
        )),
        "| A | B | C | D |\n|:---:|---:|---:|---|\n| 1 | 2 | | 4 |\n"
    );
}
