//! Random mixes of tags.

use super::*;

const PIECES: &[&str] = &[
    "a",
    "b c",
    " ",
    "\n",
    "ä",
    "😀",
    "&amp;",
    "&nbsp;",
    "&#x41;",
    "&",
    "*",
    "_",
    "`",
    "|",
    "[x]",
    "<",
    ">",
    "</",
    "<!",
    "<?",
    "<!--",
    "-->",
    "<![if !supportLists]>",
    "<![endif]>",
    "<!--[if !supportLists]-->",
    "<!--[endif]-->",
    "<p>",
    "</p>",
    "<div>",
    "</div>",
    "<span>",
    "</span>",
    "<b>",
    "</b>",
    "<i>",
    "</i>",
    "<u>",
    "</u>",
    "<s>",
    "</s>",
    "<mark>",
    "</mark>",
    "<sup>",
    "</sup>",
    "<sub>",
    "</sub>",
    "<code>",
    "</code>",
    "<kbd>",
    "</kbd>",
    "<pre>",
    "</pre>",
    "<br>",
    "<br/>",
    "<hr>",
    "<a href=\"x y\">",
    "<a href=\"javascript:x\">",
    "<a>",
    "</a>",
    "<img src=\"p.png\" alt=\"q\">",
    "<img src=\"file:///a\" alt=\"r\">",
    "<img src=\"data:image/gif;base64,R0lGODlh\">",
    "<h1>",
    "</h1>",
    "<h3>",
    "</h3>",
    "<ul>",
    "</ul>",
    "<ol start=\"5\">",
    "</ol>",
    "<li>",
    "</li>",
    "<input type=checkbox>",
    "<input type=checkbox checked>",
    "<blockquote>",
    "</blockquote>",
    "<table>",
    "</table>",
    "<thead>",
    "</thead>",
    "<tbody>",
    "</tbody>",
    "<tr>",
    "</tr>",
    "<td>",
    "</td>",
    "<th>",
    "</th>",
    "<td colspan=2>",
    "<td rowspan=3>",
    "<td colspan=70 rowspan=70>",
    "<caption>",
    "</caption>",
    "<head>",
    "</head>",
    "<body>",
    "<meta charset=utf-8>",
    "<title>",
    "</title>",
    "<style>",
    "</style>",
    "<script>",
    "</script>",
    "<o:p>",
    "</o:p>",
    "<p class=MsoListParagraph style='mso-list:l0 level2 lfo1'>",
    "<p class=MsoHeading2>",
    "<span style='mso-list:Ignore'>",
    "<span style=\"font-weight:700\">",
    "<span style=\"font-family:Menlo\">",
    "<b style=\"font-weight:normal\">",
    "<div style=\"white-space:pre;font-family:Menlo\">",
    "<span style=\"vertical-align:super\">",
    "<dl>",
    "<dt>",
    "<dd>",
    "</dl>",
    "<p/>",
    "<a b=\"",
    "\"",
    "'",
    "=",
    "<td",
    "<x y=z",
];

#[test]
fn any_mix_of_tags_gives_markdown_that_reads() {
    let mut rng = Rng::new(0x51EA_8001);
    for _ in 0..rounds(4000) {
        let mut html = String::new();
        for _ in 0..1 + rng.below(30) {
            html.push_str(rng.pick(PIECES));
        }
        let written = md(&html);
        let _ = parse(&written);
        let again = from_html(&html).into_markdown();
        assert_eq!(written, again, "{html}");
    }
}

#[test]
fn pieces_of_any_length_keep_to_char_boundaries() {
    let html = "<p>ä😀<b>ö<i>ü</i></b>€</p><img alt=\"ß\" src=\"é.png\"><!--ä--><!ä><?ä>";
    for end in 0..=html.len() {
        if html.is_char_boundary(end) {
            let _ = md(&html[..end]);
            let _ = md(&html[end..]);
        }
    }
}
