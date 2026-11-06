//! Text, marks, links and what cannot be seen.

use super::*;

#[test]
fn what_holds_nothing_gives_nothing() {
    for html in [
        "",
        " \n ",
        "<!--StartFragment--><!--EndFragment-->",
        "<p></p><div> </div><p>&nbsp;</p>",
        "<meta charset=utf-8>",
        "<script>x</script><style>p{}</style>",
        "<head><title>t</title></head>",
        "<br><br>",
        "<!DOCTYPE html><html><body></body></html>",
        "<?xml version=\"1.0\"?>",
        "<ul><li></li></ul><table><tr><td></td></tr></table><blockquote></blockquote>",
        "<img>",
        "<img src=\"\">",
        "<!-- never closed",
        "<!never closed",
    ] {
        assert_eq!(md(html), "", "{html}");
    }
}

#[test]
fn text_collapses_as_html_does() {
    same(&[
        ("a  b\n\tc", "a b c\n"),
        ("  a  ", "a\n"),
        ("<span> a </span> <span> b </span>", "a b\n"),
        ("<b> a </b> b", "**a** b\n"),
        ("a&nbsp;&nbsp;b", "a\u{a0}\u{a0}b\n"),
        ("<p>\n  a\n  b\n</p>\n\n<p>c</p>\n", "a b\n\nc\n"),
        ("a &amp; b &lt;c&gt; &#35; &copy;", "a & b \\<c> # ©\n"),
        ("1 < 2 and 3 > 2", "1 < 2 and 3 > 2\n"),
        ("x<3", "x<3\n"),
    ]);
}

#[test]
fn tags_for_a_look_give_the_marks() {
    same(&[
        ("<b>a</b>", "**a**\n"),
        ("<strong>a</strong>", "**a**\n"),
        ("<i>a</i>", "*a*\n"),
        ("<em>a</em>", "*a*\n"),
        ("<cite>a</cite> <dfn>b</dfn> <var>c</var>", "*a b c*\n"),
        ("<u>a</u>", "<ins>a</ins>\n"),
        ("<ins>a</ins>", "<ins>a</ins>\n"),
        ("<s>a</s>", "~~a~~\n"),
        ("<del>a</del>", "~~a~~\n"),
        ("<strike>a</strike>", "~~a~~\n"),
        ("<mark>a</mark>", "<mark>a</mark>\n"),
        ("<sup>a</sup>", "<sup>a</sup>\n"),
        ("<sub>a</sub>", "<sub>a</sub>\n"),
        ("<code>a</code>", "`a`\n"),
        ("<kbd>a</kbd>", "`a`\n"),
        ("<tt>a</tt>", "`a`\n"),
        ("<samp>a</samp>", "`a`\n"),
        ("<b><i>a</i></b>", "***a***\n"),
        ("<b>a<i>b</i></b>", "**a<em>b</em>**\n"),
        ("<B>a</B>", "**a**\n"),
        ("<span>a</span><font color=red>b</font><small>c</small>", "abc\n"),
        ("<b>Hal</b><b>lo</b>", "**Hallo**\n"),
        ("<b>a <i>b</b> c</i>", "**a *b*** c\n"),
        ("<b>a</i>b</b>c", "**ab**c\n"),
        ("<code>  a  b </code>", "`a b`\n"),
    ]);
}

#[test]
fn inline_styles_give_the_marks_and_take_them_away() {
    same(&[
        ("<span style=\"font-weight:bold\">a</span>", "**a**\n"),
        ("<span style=\"font-weight:bolder\">a</span>", "**a**\n"),
        ("<span style=\"font-weight:700\">a</span>", "**a**\n"),
        ("<span style=\"font-weight:600\">a</span>", "**a**\n"),
        ("<span style=\"font-weight:500\">a</span>", "a\n"),
        ("<b><span style=\"font-weight:400\">a</span></b>", "a\n"),
        ("<b><span style=\"font-weight:normal\">a</span></b>", "a\n"),
        ("<b><span style=\"font-weight:lighter\">a</span></b>", "a\n"),
        ("<b style=\"font-weight:normal\">a</b>", "a\n"),
        ("<b><span style=\"font-weight:inherit\">a</span></b>", "**a**\n"),
        ("<span style=\"FONT-WEIGHT: BOLD !important\">a</span>", "**a**\n"),
        ("<span style=\"font-style:italic\">a</span>", "*a*\n"),
        ("<span style=\"font-style:oblique\">a</span>", "*a*\n"),
        ("<i><span style=\"font-style:normal\">a</span></i>", "a\n"),
        ("<i><span style=\"font-style:inherit\">a</span></i>", "*a*\n"),
        ("<span style=\"text-decoration:underline\">a</span>", "<ins>a</ins>\n"),
        ("<span style=\"text-decoration:line-through\">a</span>", "~~a~~\n"),
        (
            "<span style=\"text-decoration: underline line-through\">a</span>",
            "~~<ins>a</ins>~~\n",
        ),
        (
            "<span style=\"text-decoration-line:underline\">a</span>",
            "<ins>a</ins>\n",
        ),
        ("<u><span style=\"text-decoration:none\">a</span></u>", "a\n"),
        ("<s><span style=\"text-decoration:none\">a</span></s>", "a\n"),
        ("<span style=\"text-decoration:wavy\">a</span>", "a\n"),
        ("<span style=\"-webkit-text-decoration-skip:none\">a</span>", "a\n"),
        ("<span style=\"vertical-align:super\">a</span>", "<sup>a</sup>\n"),
        ("<span style=\"vertical-align:sub\">a</span>", "<sub>a</sub>\n"),
        ("<sup><span style=\"vertical-align:baseline\">a</span></sup>", "a\n"),
        ("<sub><span style=\"vertical-align:baseline\">a</span></sub>", "a\n"),
        ("<span style=\"vertical-align:2px\">a</span>", "a\n"),
        ("<span style=\"background-color:yellow\">a</span>", "a\n"),
        ("<mark style=\"background-color:yellow\">a</mark>", "<mark>a</mark>\n"),
        ("<p style=\"font-weight:bold\">a</p>", "**a**\n"),
        ("<span style=\"color\">a</span>", "a\n"),
        ("<b style='mso-bidi-font-weight:normal'>a</b>", "**a**\n"),
        ("<i style='mso-bidi-font-style:normal'>a</i>", "*a*\n"),
    ]);
}

#[test]
fn a_monospace_font_on_a_span_is_code_and_on_a_block_is_not() {
    same(&[
        ("a <span style=\"font-family:Menlo,monospace\">b</span>", "a `b`\n"),
        ("<span style='font-family:\"Courier New\"'>a</span>", "`a`\n"),
        ("<font face=\"Consolas\">a</font>", "`a`\n"),
        ("<font face=\"Arial\">a</font>", "a\n"),
        ("<span style=\"font-family:Arial\">a</span>", "a\n"),
        ("<p style=\"font-family:monospace\">a</p>", "a\n"),
    ]);
}

#[test]
fn line_breaks_break_lines_and_two_make_a_new_paragraph() {
    same(&[
        ("a<br>b", "a\\\nb\n"),
        ("a<br/>b", "a\\\nb\n"),
        ("a <br> b", "a\\\nb\n"),
        ("<br>a<br>", "a\n"),
        ("a<br><br>b", "a\n\nb\n"),
        ("a<br> <br>b", "a\n\nb\n"),
        ("<p>a<br></p>", "a\n"),
        ("<b>a<br>b</b>", "**a\\\nb**\n"),
    ]);
}

#[test]
fn what_cannot_be_seen_is_dropped_with_its_content() {
    same(&[
        ("a<script>b</script>c", "ac\n"),
        ("a<SCRIPT>b</SCRIPT>c", "ac\n"),
        ("a<style>b</style>c", "ac\n"),
        ("a<title>b</title>c", "ac\n"),
        ("a<svg><title>b</title><path/></svg>c", "ac\n"),
        ("a<textarea>b</textarea>c", "ac\n"),
        ("a<select><option>b</select>c", "ac\n"),
        ("a<button>b</button>c", "ac\n"),
        ("a<iframe>b</iframe>c", "ac\n"),
        ("a<noscript>b</noscript>c", "ac\n"),
        ("a<object>b</object>c", "ac\n"),
        ("a<template>b</template>c", "ac\n"),
        ("a<canvas>b</canvas><audio>b</audio><video>b</video>c", "ac\n"),
        ("a<xml>b</xml>c", "ac\n"),
        ("a<script src=x />b", "ab\n"),
        ("a<script>b", "a\n"),
        ("a<script>b</script", "a\n"),
        ("a<script>b</other>c", "a\n"),
        (
            "<head><meta charset=utf-8><title>x</title><style>y</style><link rel=a>text</head><body>a</body>",
            "a\n",
        ),
        ("<head><meta charset=utf-8>a<p>b", "b\n"),
        ("<head/>a", "a\n"),
        ("<html><head><script>x</script></head><body>a</body></html>", "a\n"),
        ("a<!-- b -->c", "ac\n"),
        ("a<!-- b", "a\n"),
        ("<!DOCTYPE html>a", "a\n"),
        ("<?xml version=\"1.0\"?>a", "a\n"),
        ("<![if !vml]>a<![endif]>", "a\n"),
        ("<!--[if gte mso 9]><xml><o:x>y</o:x></xml><![endif]-->a", "a\n"),
        ("<o:p></o:p>a<o:p>&nbsp;</o:p>", "a\n"),
        ("<st1:place w:st=\"on\">a</st1:place> b", "a b\n"),
        ("a<input type=text value=b>c", "ac\n"),
    ]);
}

#[test]
fn a_tag_that_is_none_stays_text() {
    same(&[
        ("a<", "a<\n"),
        ("<", "<\n"),
        ("a < b", "a < b\n"),
        ("<3", "<3\n"),
        ("</ a", "\\</ a\n"),
        ("<a href=\"x", "\\<a href=\"x\n"),
        ("ä<ö>", "ä<ö>\n"),
    ]);
}

#[test]
fn links_lead_where_they_say_and_only_where_they_may() {
    same(&[
        ("<a href=\"https://a.bc\">t</a>", "[t](https://a.bc)\n"),
        ("<a href=\" https://a.bc \">t</a>", "[t](https://a.bc)\n"),
        ("<a href=\"#a\">t</a>", "[t](#a)\n"),
        ("<a href=\"b/c.md\">t</a>", "[t](b/c.md)\n"),
        ("<a href=\"mailto:a@b.cd\">t</a>", "[t](mailto:a@b.cd)\n"),
        ("<a href=\"a b\">t</a>", "[t](<a b>)\n"),
        (
            "<a href=\"https://a.bc/?a=1&amp;b=2\">t</a>",
            "[t](https://a.bc/?a=1&b=2)\n",
        ),
        ("<a href=\"javascript:alert(1)\">t</a>", "t\n"),
        ("<a href=\"file:///etc/passwd\">t</a>", "t\n"),
        ("<a>t</a>", "t\n"),
        ("<a name=\"x\">t</a>", "t\n"),
        ("<a href=\"\">t</a>", "t\n"),
        ("<a href=\"x\"><b>t</b></a>", "**[t](x)**\n"),
        ("<b><a href=\"x\">t</a></b>", "**[t](x)**\n"),
        ("<a href=\"x\"><u>t</u></a>", "[t](x)\n"),
        (
            "<a href=\"x\"><span style=\"text-decoration:underline\">t</span></a>",
            "[t](x)\n",
        ),
        ("<a href=\"x\">a<b>b</b>c</a>", "[a**b**c](x)\n"),
        ("<a href=\"x\"><img src=\"y.png\" alt=\"z\"></a>", "[![z](y.png)](x)\n"),
        ("<a href=\"x\">a</a><a href=\"x\">b</a>", "[ab](x)\n"),
        ("<a href=\"x\"> a </a>b", " [a](x) b\n".trim_start()),
    ]);
}
