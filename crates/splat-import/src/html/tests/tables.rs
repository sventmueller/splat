//! Tables and the cells that span more than one column or row.

use super::*;

#[test]
fn a_column_takes_the_alignment_of_a_paragraph_in_its_first_cell() {
    same(&[
        (
            "<table><tr><td><p>a</p></td><td><p class=MsoNormal align=right>b</p></td><td><p style='text-align:center'>c</p><p align=right>d</p></td></tr>\
             <tr><td><p align=right>1</p></td><td>2</td><td>3</td></tr></table>",
            "| a | b | c<br>d |\n|---|---:|:---:|\n| 1 | 2 | 3 |\n",
        ),
        (
            "<table><tr><td align=left><p align=right>a</p></td><td colspan=2><p align=center>b</p></td><td>c</td></tr></table>",
            "<table>\n<tr><th align=\"left\">a</th><th colspan=\"2\" align=\"center\">b</th><th>c</th></tr>\n</table>\n",
        ),
    ]);
}

#[test]
fn tables_read_with_a_header_and_cell_paragraphs_flattened() {
    same(&[
        (
            "<table><thead><tr><th>A</th><th>B</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table>",
            "| A | B |\n|---|---|\n| 1 | 2 |\n",
        ),
        (
            "<table><tr><td>A</td><td>B</td></tr><tr><td>1</td><td>2</td></tr></table>",
            "| A | B |\n|---|---|\n| 1 | 2 |\n",
        ),
        (
            "<table><tr><td>A<td>B<tr><td>1<td>2</table>",
            "| A | B |\n|---|---|\n| 1 | 2 |\n",
        ),
        (
            "<table><td>A</td><td>B</td><tr><td>1</td></table>",
            "| A | B |\n|---|---|\n| 1 | |\n",
        ),
        (
            "<table><thead><tr><th>A<tbody><tr><td>1</table>",
            "| A |\n|---|\n| 1 |\n",
        ),
        (
            "<table><tr><th><b>A</b></th><th>B</th></tr></table>",
            "| A | B |\n|---|---|\n",
        ),
        (
            "<table><tr><td><b>A</b> c</td><td>B</td></tr></table>",
            "| **A** c | B |\n|---|---|\n",
        ),
        (
            "<table><tr><td><p>a</p><p>b</p></td><td><ul><li>c<li>d</ul></td></tr></table>",
            "| a<br>b | c<br>d |\n|---|---|\n",
        ),
        (
            "<table><tr><td><h1>a</h1>b</td><td><pre>c\nd</pre>e</td></tr></table>",
            "| a<br>b | `c`<br>`d`<br>e |\n|---|---|\n",
        ),
        (
            "<table><tr><td>a<hr>b</td><td><blockquote>c</blockquote></td></tr></table>",
            "| a<br>b | c |\n|---|---|\n",
        ),
        (
            "<table><tr><td>a<br>b</td><td><p>c<br></p></td></tr></table>",
            "| a<br>b | c |\n|---|---|\n",
        ),
        (
            "<table><tr><td>a|b</td><td><a href=\"x|y\">c</a></td></tr></table>",
            "| a\\|b | [c](x%7Cy) |\n|---|---|\n",
        ),
        (
            "<table><tr><td>a<img src=\"b.png\" alt=\"c\"></td><td>d</td></tr></table>",
            "| a![c](b.png) | d |\n|---|---|\n",
        ),
        (
            "<p>a<table><tr><td>b</td><td>c</td></tr></table>d",
            "a\n\n| b | c |\n|---|---|\n\nd\n",
        ),
        (
            "<table><caption>t</caption><tr><td>a</td><td>b</td></tr></table>",
            "t\n\n| a | b |\n|---|---|\n",
        ),
        (
            "<table>x<tr>y<td>a</td>z<td>b</td></tr></table>",
            "x\n\ny\n\nz\n\n| a | b |\n|---|---|\n",
        ),
        ("<table></table>", ""),
        (
            "<table><tr></tr><tr><td>a</td><td>b</td></tr></table>",
            "| a | b |\n|---|---|\n",
        ),
    ]);
}

#[test]
fn a_cell_keeps_a_look_that_ends_inside_it_from_ending_the_table() {
    same(&[
        (
            "<b><table><tr><td>a</b></td><td>b</td></tr></table>",
            "| a | b |\n|---|---|\n",
        ),
        (
            "<table><tr><td><ul><li>a</li></td><td>b</td></tr></table>",
            "| a | b |\n|---|---|\n",
        ),
        (
            "<ul><li><table><tr><td>a</li></td><td>b</td></tr></table></li></ul>",
            "- | a | b |\n  |---|---|\n",
        ),
    ]);
}

#[test]
fn a_nested_table_flattens_into_its_cell() {
    same(&[(
        "<table><tr><td>a<table><tr><td>b</td><td>c</td></tr><tr><td>d</td></tr></table>e</td><td>f</td></tr></table>",
        "| a<br>b<br>c<br>d<br>e | f |\n|---|---|\n",
    )]);
}

#[test]
fn colspan_makes_an_html_table_and_rowspan_empty_cells() {
    same(&[
        (
            "<table><tr><th colspan=\"2\">a</th></tr><tr><td>b</td><td>c</td></tr></table>",
            "<table>\n<tr><th colspan=\"2\">a</th></tr>\n<tr><td>b</td><td>c</td></tr>\n</table>\n",
        ),
        (
            "<table><tr><td colspan=\"0\">a</td><td colspan=\"1\">b</td></tr></table>",
            "| a | b |\n|---|---|\n",
        ),
        (
            "<table><tr><td colspan=\"x\">a</td><td>b</td></tr></table>",
            "| a | b |\n|---|---|\n",
        ),
        (
            "<table><tr><td rowspan=\"2\">a</td><td>b</td></tr><tr><td>c</td></tr><tr><td>d</td><td>e</td></tr></table>",
            "| a | b |\n|---|---|\n| | c |\n| d | e |\n",
        ),
        (
            "<table><tr><td>a</td><td rowspan=\"3\">b</td></tr><tr><td>c</td></tr><tr><td>d</td></tr><tr><td>e</td><td>f</td></tr></table>",
            "| a | b |\n|---|---|\n| c | |\n| d | |\n| e | f |\n",
        ),
        (
            "<table><tr><td rowspan=\"2\" colspan=\"2\">a</td><td>b</td></tr><tr><td>c</td></tr></table>",
            "<table>\n<tr><th colspan=\"2\">a</th><th>b</th></tr>\n<tr><td></td><td></td><td>c</td></tr>\n</table>\n",
        ),
        (
            "<table><tr><td rowspan=\"0\">a</td><td>b</td></tr><tr><td>c</td></tr></table>",
            "| a | b |\n|---|---|\n| c | |\n",
        ),
        (
            "<table><tr><td>a</td><td rowspan=\"2\">b</td><td rowspan=\"2\">c</td></tr><tr></tr><tr><td>d</td><td>e</td><td>f</td></tr></table>",
            "| a | b | c |\n|---|---|---|\n| d | e | f |\n",
        ),
    ]);
}

#[test]
fn rowspan_cannot_make_a_table_quadratically_long() {
    let cells = 2000;
    let html = format!(
        "<table><tr>{}</tr>{}<tr><td>x</td></tr></table>",
        "<td rowspan=\"100000\" colspan=\"64\">a</td>".repeat(cells),
        "<tr><td>b</td></tr>".repeat(cells)
    );
    let written = md(&html);
    assert!(written.len() < html.len() * 8, "{} from {}", written.len(), html.len());
    assert!(written.contains("<td>b</td>"));
}

#[test]
fn a_column_takes_its_alignment_from_the_first_row() {
    same(&[
        (
            "<table><tr><th align=\"right\">a</th><th style=\"text-align:center\">b</th><th align=\"left\">c</th><th>d</th></tr></table>",
            "| a | b | c | d |\n|---:|:---:|:---|---|\n",
        ),
        (
            "<table><tr><td>a</td><td>b</td></tr><tr><td align=\"right\">c</td><td>d</td></tr></table>",
            "| a | b |\n|---|---|\n| c | d |\n",
        ),
        (
            "<table><tr><td align=\"middle\">a</td><td style=\"color:red;text-align:justify\">b</td></tr></table>",
            "| a | b |\n|:---:|---|\n",
        ),
    ]);
}

#[test]
fn a_table_of_one_cell_is_a_layout_and_no_table() {
    same(&[
        ("<table><tr><td>a <b>b</b></td></tr></table>", "a **b**\n"),
        (
            "<table><tr><td><table><tr><td>a</td></tr></table></td></tr></table>",
            "a\n",
        ),
        (
            "<table><tr><td>a</td></tr><tr><td>b</td></tr></table>",
            "| a |\n|---|\n| b |\n",
        ),
    ]);
}

#[test]
fn stray_table_parts_end_lines_and_do_nothing_else() {
    same(&[
        ("<tr><td>a</td><td>b</td></tr>", "a\n\nb\n"),
        ("a<td>b", "a\n\nb\n"),
        ("<ul><li><td>a</td></li></ul>", "- a\n"),
    ]);
}
