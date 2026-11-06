//! What Word, Google Docs, Confluence, Excel, Notes and browsers copy.

use super::*;

#[test]
fn word_lists_come_from_their_paragraphs() {
    let bullet = |level: u32, list: u32, text: &str| {
        format!(
            "<p class=MsoListParagraphCxSpFirst style='margin-left:.5in;text-indent:-.25in;mso-list:l{list} level{level} lfo{list}'>\
             <![if !supportLists]><span style='font-family:Symbol;mso-fareast-font-family:Symbol'><span style='mso-list:Ignore'>·\
             <span style='font:7.0pt \"Times New Roman\"'>&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp; </span></span></span><![endif]>{text}<o:p></o:p></p>\n"
        )
    };
    let html = format!(
        "<!--StartFragment--><p class=MsoNormal>Vorher<o:p></o:p></p>\n{}{}{}{}<p class=MsoNormal><o:p>&nbsp;</o:p></p>\n{}<p class=MsoNormal>Nachher</p><!--EndFragment-->",
        bullet(1, 0, "Eins"),
        bullet(2, 0, "<b>Zwei</b>"),
        bullet(2, 0, "Drei"),
        bullet(1, 0, "Vier"),
        bullet(1, 1, "Fünf"),
    );
    assert_eq!(
        md(&html),
        "Vorher\n\n- Eins\n  - **Zwei**\n  - Drei\n- Vier\n\n* Fünf\n\nNachher\n"
    );
}

#[test]
fn word_numbers_decide_the_kind_of_list_and_where_it_starts() {
    let item = |level: u32, marker: &str, text: &str| {
        format!(
            "<p class=MsoListParagraph style='mso-list:l1 level{level} lfo2'><![if !supportLists]><span style='mso-list:Ignore'>{marker}\
             <span style='font:7.0pt \"Times New Roman\"'>&nbsp; </span></span><![endif]>{text}</p>"
        )
    };
    assert_eq!(
        md(&(item(1, "1.", "a")
            + &item(1, "2.", "b")
            + &item(2, "a)", "c")
            + &item(2, "b)", "d")
            + &item(1, "3.", "e"))),
        "1. a\n2. b\n   1. c\n   2. d\n3. e\n"
    );
    assert_eq!(md(&item(1, "4)", "a")), "4. a\n");
    assert_eq!(md(&item(1, "(c)", "a")), "3. a\n");
    assert_eq!(md(&item(1, "i.", "a")), "1. a\n");
    assert_eq!(md(&item(1, "iv.", "a")), "1. a\n");
    assert_eq!(md(&item(1, "1.2.5.", "a")), "5. a\n");
    assert_eq!(md(&item(1, "7", "a")), "7. a\n");
    assert_eq!(md(&item(1, "o", "a")), "- a\n");
    assert_eq!(md(&item(1, "§", "a")), "- a\n");
    assert_eq!(md(&item(1, ".", "a")), "- a\n");
    assert_eq!(md(&item(1, "ab.c.", "a")), "3. a\n");
    assert_eq!(md(&item(1, "ü.", "a")), "- a\n");
    assert_eq!(md(&item(1, "☐", "a")), "- [ ] a\n");
    assert_eq!(md(&item(1, "☑", "a")), "- [x] a\n");
    assert_eq!(md(&item(1, "☒", "a")), "- [x] a\n");
}

#[test]
fn word_marks_a_marker_in_more_ways_than_one() {
    same(&[
        (
            "<p class=MsoListParagraph style='mso-list:l0 level1 lfo1'><!--[if !supportLists]--><span>1.&nbsp;</span><!--[endif]-->a</p>",
            "1. a\n",
        ),
        (
            "<p class=MsoListParagraph><span style='mso-list:Ignore'>1.</span>a</p>",
            "1. a\n",
        ),
        (
            "<p class=MsoListParagraph><![if !supportLists]>·<![endif]>a</p>",
            "- a\n",
        ),
        (
            "<p class=MsoListParagraph style='mso-list:l0 level1 lfo1'>a</p>",
            "- a\n",
        ),
        ("<p class=MsoListParagraph>a</p>", "a\n"),
        ("<p class=MsoListParagraph style='mso-list:skip'>a</p>", "a\n"),
        (
            "<p style='mso-list:l0 level99 lfo1'><![if !supportLists]><![endif]>a</p>",
            "- a\n",
        ),
        (
            "<p style='mso-list:l0 level1 lfo1'><![if !supportLists]><img src=\"b.png\" alt=\"c\"><br><![endif]>a</p>",
            "- a\n",
        ),
        ("<p><![if !supportLists]>never ended</p><p>a</p>", "a\n"),
        (
            "<h1 style='mso-list:l0 level1 lfo1'><![if !supportLists]>1.<![endif]>a</h1>",
            "# a\n",
        ),
        (
            "<ul><li class=MsoListParagraph style='mso-list:l0 level1 lfo1'>a</li></ul>",
            "- a\n",
        ),
        (
            "<table><tr><td><p style='mso-list:l0 level1 lfo1'><![if !supportLists]>1.<![endif]>a</p></td><td>b</td></tr></table>",
            "| a | b |\n|---|---|\n",
        ),
        (
            "<p style='mso-list:l0 level1 lfo1'>a</p><blockquote>b</blockquote>",
            "- a\n\n> b\n",
        ),
        (
            "<p style='mso-list:l0 level1 lfo1'>a</p><table><tr><td>b</td><td>c</td></tr></table>",
            "- a\n\n| b | c |\n|---|---|\n",
        ),
        (
            "<p style='mso-list:l0 level1 lfo1'>a</p><ul><li>b</li></ul>",
            "- a\n\n* b\n",
        ),
        (
            "<blockquote><p style='mso-list:l0 level1 lfo1'>a</p></blockquote>",
            "> - a\n",
        ),
        ("<p style='mso-list:l0 level1 lfo1'>a<br><br>b</p>", "- a\n- b\n"),
    ]);
}

#[test]
fn word_lists_on_a_filtered_page_come_from_their_markers_and_margins() {
    let item = |margin: &str, marker: &str, text: &str| {
        format!(
            "<p class=MsoListParagraphCxSpMiddle style='{margin}text-indent:-18.0pt'>{marker}<span \
             style='font:7.0pt \"Times New Roman\"'>&nbsp;&nbsp;&nbsp; </span>{text}</p>\n"
        )
    };
    let html = format!(
        "<p class=MsoNormal>Vorher</p>{}{}{}{}<p class=MsoNormal>Dazwischen</p>{}{}",
        item("", "<span style='font-family:Symbol'>·</span>", "Eins"),
        item("margin-left:1.0in;", "a)", "Zwei"),
        item("margin-left:2.54cm;", "b)", "<b>Drei</b>"),
        item("margin-left:36.0pt;", "•", "Vier"),
        item("margin-left:18pt;", "1.", "Fünf"),
        item("margin-left:36pt;", "–", "Sechs"),
    );
    assert_eq!(
        md(&html),
        "Vorher\n\n- Eins\n  1. Zwei\n  2. **Drei**\n- Vier\n\nDazwischen\n\n1. Fünf\n   - Sechs\n"
    );
    same(&[
        (
            "<h1 style='margin-left:21.6pt;text-indent:-21.6pt'>1.<span style='font:7.0pt \"Times New Roman\"'>&nbsp; </span>Einleitung</h1>",
            "# Einleitung\n",
        ),
        (
            "<p class=MsoNormal>a<span style='font:7.0pt \"Times New Roman\"'> </span>b</p>",
            "a b\n",
        ),
        (
            "<p style='text-indent:-18pt'>Ein Satz davor<span style='font:7.0pt \"Times New Roman\"'> </span>b</p>",
            "Ein Satz davor b\n",
        ),
        (
            "<p style='text-indent:-18pt'><img src=\"a.png\"><span style='font:7.0pt \"Times New Roman\"'> </span>b</p>",
            "![](a.png) b\n",
        ),
        (
            "<p style='text-indent:-18pt'><span style='font:7.0pt \"Times New Roman\"'> </span>b</p>",
            "b\n",
        ),
    ]);
}

#[test]
fn word_footnotes_become_footnotes_in_the_order_they_are_referred_to() {
    let reference = |kind: &str, number: u32, label: &str| {
        format!(
            "<a style='mso-footnote-id:{kind}{number}' href=\"#_{kind}{number}\" name=\"_{kind}ref{number}\" title=\"\">\
             <span class=MsoFootnoteReference><span style='mso-special-character:footnote'><![if !supportFootnotes]>\
             <span class=MsoFootnoteReference>{label}</span><![endif]></span></span></a>"
        )
    };
    let note = |kind: &str, number: u32, label: &str, text: &str| {
        format!(
            "<div style='mso-element:note' id={kind}{number}><p class=MsoFootnoteText><a href=\"#_{kind}ref{number}\" \
             name=\"_{kind}{number}\"><span class=MsoFootnoteReference>{label}</span></a> {text}</p></div>"
        )
    };
    let html = format!(
        "<p class=MsoNormal>Erst{} dann{} und{}.</p>\n<div style='mso-element:footnote-list'><![if !supportFootnotes]><br clear=all>\
         <hr align=left size=1 width=\"33%\"><![endif]>{}{}</div>{}",
        reference("ftn", 2, "[2]"),
        reference("ftn", 1, "[1]"),
        reference("edn", 1, "[i]"),
        note("ftn", 1, "[1]", "Die <i>erste</i>."),
        note(
            "ftn",
            2,
            "[2]",
            "Die zweite.<p class=MsoFootnoteText>Noch ein Absatz.</p>"
        ),
        note("edn", 1, "[i]", "Am Ende."),
    );
    assert_eq!(
        md(&html),
        "Erst[^1] dann[^2] und[^3].\n\n[^1]: Die zweite.\n\n    Noch ein Absatz.\n\n[^2]: Die *erste*.\n\n[^3]: Am Ende.\n"
    );
    same(&[
        ("<p>a<a href=\"#_ftn1\" name=\"_ftnref1\">[1]</a> b</p>", "a[1] b\n"),
        (
            "<p>a</p><hr><div id=ftn1><p><a href=\"#_ftnref1\" name=\"_ftn1\">[1]</a> Notiz</p></div>",
            "a\n\n---\n\nNotiz\n",
        ),
        (
            "<p>a<a href=\"#_ftnX\">[1]</a></p><div id=ftn><p>b</p></div>",
            "a[\\[1&#93;](#_ftnX)\n\nb\n",
        ),
    ]);
}

#[test]
fn word_headings_come_from_tags_and_classes() {
    same(&[
        (
            "<h1><a name=\"_Toc1\"></a><span lang=DE>Titel<o:p></o:p></span></h1>",
            "# Titel\n",
        ),
        ("<p class=MsoHeading2>a</p>", "## a\n"),
        ("<p class=MsoHeading8>a</p>", "###### a\n"),
        ("<p class=MsoHeading0>a</p>", "# a\n"),
        ("<p class=MsoHeadingX>a</p>", "a\n"),
        ("<p class=MsoHeading1>a<p>b", "# a\n\nb\n"),
    ]);
}

#[test]
fn a_page_copied_from_word_reads_cleanly() {
    let html = "<html xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns=\"http://www.w3.org/TR/REC-html40\">\
        <head><meta http-equiv=Content-Type content=\"text/html; charset=utf-8\"><meta name=Generator content=\"Microsoft Word 15\">\
        <!--[if gte mso 9]><xml><o:OfficeDocumentSettings><o:AllowPNG/></o:OfficeDocumentSettings></xml><![endif]-->\
        <style><!-- p.MsoNormal {margin:0cm;} --></style></head>\n\
        <body lang=DE style='tab-interval:35.4pt'><!--StartFragment--><p class=MsoNormal>Ein <b style='mso-bidi-font-weight:normal'>fetter</b> und \
        <i style='mso-bidi-font-style:normal'>schr\u{e4}ger</i> Text<span style='mso-spacerun:yes'>   </span>mit <u>Strich</u>.<o:p></o:p></p>\n\
        <p class=MsoNormal><o:p>&nbsp;</o:p></p>\n<p class=MsoNormal><span style='font-family:\"Courier New\"'>code()</span> \
        <a href=\"https://example.com/\"><span style='color:windowtext;text-decoration:none'>Link</span></a><o:p></o:p></p><!--EndFragment--></body></html>";
    assert_eq!(
        md(html),
        "Ein **fetter** und *schr\u{e4}ger* Text mit <ins>Strich</ins>.\n\n`code()` [Link](https://example.com/)\n"
    );
}

#[test]
fn what_google_docs_copies_reads_cleanly() {
    let html = "<meta charset='utf-8'><meta charset=\"utf-8\"><b style=\"font-weight:normal;\" id=\"docs-internal-guid-1234-abcd\">\
        <h1 dir=\"ltr\" style=\"line-height:1.38;\"><span style=\"font-size:20pt;font-family:Arial,sans-serif;font-weight:400;font-style:normal;text-decoration:none;vertical-align:baseline;white-space:pre;white-space:pre-wrap;\">Titel</span></h1>\
        <p dir=\"ltr\" style=\"line-height:1.38;margin-top:0pt;margin-bottom:0pt;\"><span style=\"font-size:11pt;font-family:Arial,sans-serif;color:#000000;background-color:transparent;font-weight:400;font-style:normal;font-variant:normal;text-decoration:none;vertical-align:baseline;white-space:pre-wrap;\">normal </span>\
        <span style=\"font-size:11pt;font-weight:700;font-style:normal;text-decoration:none;vertical-align:baseline;\">fett</span>\
        <span style=\"font-weight:400;font-style:italic;text-decoration:none;vertical-align:baseline;\"> kursiv</span>\
        <span style=\"font-weight:400;text-decoration:underline;-webkit-text-decoration-skip:none;text-decoration-skip-ink:none;vertical-align:baseline;\"> unter</span>\
        <span style=\"font-weight:400;text-decoration:line-through;vertical-align:baseline;\"> durch</span>\
        <span style=\"font-weight:400;vertical-align:super;\"> hoch</span>\
        <a href=\"https://a.bc/\" style=\"text-decoration:none;\"><span style=\"color:#1155cc;text-decoration:underline;\"> Link</span></a></p><br />\
        <ul style=\"margin-top:0;margin-bottom:0;padding-inline-start:48px;\">\
        <li dir=\"ltr\" style=\"list-style-type:disc;font-size:11pt;font-family:Arial,sans-serif;white-space:pre;\" aria-level=\"1\"><p dir=\"ltr\" role=\"presentation\" style=\"line-height:1.38;\"><span style=\"font-weight:400;\">eins</span></p></li>\
        <ul style=\"margin-top:0;\"><li dir=\"ltr\" aria-level=\"2\"><p dir=\"ltr\" role=\"presentation\"><span style=\"font-weight:400;\">unten</span></p></li></ul>\
        <li dir=\"ltr\" aria-level=\"1\"><p dir=\"ltr\" role=\"presentation\"><span style=\"font-weight:700;\">zwei</span></p></li></ul>\
        <div dir=\"ltr\" style=\"margin-left:0pt;\" align=\"left\"><table style=\"border:none;border-collapse:collapse;\"><colgroup><col width=\"312\" /><col width=\"312\" /></colgroup><tbody>\
        <tr style=\"height:0pt\"><td style=\"border-left:solid #000000 1pt;\"><p dir=\"ltr\" style=\"line-height:1.2;\"><span style=\"font-weight:400;\">A</span></p></td><td><p dir=\"ltr\"><span style=\"font-weight:400;\">B</span></p></td></tr>\
        <tr style=\"height:0pt\"><td><p dir=\"ltr\"><span style=\"font-weight:400;\">1</span></p></td><td><p dir=\"ltr\"><span style=\"font-weight:400;\">2</span></p></td></tr></tbody></table></div></b>";
    assert_eq!(
        md(html),
        "# Titel\n\nnormal **fett** *kursiv* <ins>unter</ins> ~~durch~~ <sup>hoch</sup> [Link](https://a.bc/)\n\n\
         - eins\n  - unten\n- **zwei**\n\n| A | B |\n|---|---|\n| 1 | 2 |\n"
    );
}

#[test]
fn what_confluence_copies_reads_cleanly() {
    let html = "<div class=\"table-wrap\"><table class=\"confluenceTable\"><colgroup><col /><col /></colgroup><tbody>\
        <tr><th class=\"confluenceTh\"><p>Name</p></th><th class=\"confluenceTh\" colspan=\"1\"><p><strong>Wert</strong></p></th></tr>\
        <tr><td class=\"confluenceTd\"><p>a</p></td><td class=\"confluenceTd\"><ul><li>x</li><li>y</li></ul></td></tr></tbody></table></div>\
        <div class=\"confluence-information-macro\"><p class=\"title\">Hinweis</p><div class=\"confluence-information-macro-body\"><p>Text</p></div></div>";
    assert_eq!(
        md(html),
        "| Name | Wert |\n|---|---|\n| a | x<br>y |\n\nHinweis\n\nText\n"
    );
}

#[test]
fn what_excel_copies_gives_a_clean_table() {
    let html = "<html xmlns:v=\"urn:schemas-microsoft-com:vml\"><head><meta name=Generator content=\"Microsoft Excel 15\"><style>\
        <!--table {mso-displayed-decimal-separator:\"\\,\";} .xl65 {font-weight:700; text-decoration:underline;} --></style></head><body link=\"#0563C1\">\
        <table border=0 cellpadding=0 cellspacing=0 width=128 style='border-collapse:collapse;width:96pt'>\n\
        <!--StartFragment-->\n <col width=64 span=2 style='width:48pt'>\n\
        <tr height=20 style='height:15.0pt'>\n  <td height=20 class=xl65 width=64 style='height:15.0pt;width:48pt'>Name</td>\n  <td class=xl65 width=64 style='width:48pt'>Wert</td>\n </tr>\n\
        <tr height=20 style='height:15.0pt'>\n  <td height=20 style='height:15.0pt'>a | b</td>\n  <td align=right>1,5</td>\n </tr>\n\
        <tr height=20 style='height:15.0pt'>\n  <td height=20 class=xl66 style='height:15.0pt'>&nbsp;</td>\n  <td></td>\n </tr>\n\
        <tr height=20 style='height:15.0pt'>\n  <td colspan=2 height=20 class=xl67>Summe</td>\n </tr>\n<!--EndFragment-->\n</table></body></html>";
    assert_eq!(
        md(html),
        "<table>\n<tr><th>Name</th><th>Wert</th></tr>\n<tr><td>a | b</td><td>1,5</td></tr>\n<tr><td colspan=\"2\">Summe</td></tr>\n</table>\n"
    );
    let plain = "<table><!--StartFragment--><col width=64><tr><td class=xl65>Name</td><td class=xl65>Wert</td></tr>\
        <tr><td class=xl66>a</td><td class=xl66 x:num=\"1.5\">1,5</td></tr><!--EndFragment--></table>";
    assert_eq!(md(plain), "| Name | Wert |\n|---|---|\n| a | 1,5 |\n");
}

#[test]
fn what_notes_copies_reads_cleanly() {
    let html = "<!DOCTYPE html PUBLIC \"-//W3C//DTD HTML 4.01//EN\"><html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=UTF-8\">\
        <meta name=\"Generator\" content=\"Cocoa HTML Writer\"><style type=\"text/css\">\np.p1 {margin: 0.0px 0.0px 0.0px 0.0px; font: 12.0px 'Helvetica Neue'}\n\
        span.Apple-converted-space {white-space: pre}\n</style></head><body>\n<p class=\"p1\"><b>Titel</b></p>\n<p class=\"p2\"><br></p>\n\
        <p class=\"p1\">Text<span class=\"Apple-converted-space\">&nbsp; </span>mehr</p>\n<ul class=\"ul1\">\n<li class=\"li1\">eins<br></li>\n<li class=\"li1\"><i>zwei</i></li>\n</ul>\n</body></html>\n";
    assert_eq!(md(html), "**Titel**\n\nText\u{a0} mehr\n\n- eins\n- *zwei*\n");
}

#[test]
fn what_textedit_writes_keeps_the_looks_of_its_style_sheet() {
    let sheet = "<style type=\"text/css\">\n<!-- p.p1 {margin: 0.0px; font: 12.0px Times}\n p.p2 {text-align: right; font: 12.0px Times}\n\
        span.s1 {text-decoration: line-through}\n span.s2 {font: 12.0px 'Courier New'}\n .s3, span.s4 {font-weight: bold}\n\
        td.td1 {text-align: center}\n div p.p9 {font-weight: bold}\n span.s5 {font-style: italic -->\n</style>";
    let body = "<body><p class=\"p1\">Mit <span class=\"s1\">weg</span>, <span class=\"s2\">code</span>, \
        <span class=\"s3\">fett</span> und <span class=\"s4 s1\">beides</span>.</p>\
        <p class=\"p9\">nicht</p><p class=\"p1\"><span class=\"s5\">auch nicht</span></p>\
        <table><tr><td class=\"td1\"><p class=\"p1\">a</p></td><td><p class=\"p2\">b</p></td></tr><tr><td>1</td><td>2</td></tr></table></body>";
    let cocoa =
        format!("<html><head><meta name=\"Generator\" content=\"Cocoa HTML Writer\">{sheet}</head>{body}</html>");
    assert_eq!(
        md(&cocoa),
        "Mit ~~weg~~, `code`, **fett** und **~~beides~~**.\n\nnicht\n\nauch nicht\n\n| a | b |\n|:---:|---:|\n| 1 | 2 |\n"
    );
    let other =
        format!("<html><head><meta name=\"Generator\" content=\"Microsoft Excel 15\">{sheet}</head>{body}</html>");
    assert_eq!(
        md(&other),
        "Mit weg, code, fett und beides.\n\nnicht\n\nauch nicht\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
        "only Cocoa's sheets count"
    );
}

#[test]
fn what_a_browser_copies_from_a_page_reads_cleanly() {
    let html = "<h2 dir=\"auto\" tabindex=\"-1\"><a class=\"anchor\" href=\"#x\"><svg class=\"octicon\"><path d=\"M\"></path></svg></a>Titel</h2>\
        <nav>Menü</nav><p dir=\"auto\">Text mit <code class=\"notranslate\">code</code> und <kbd>Cmd</kbd>.</p>\
        <div class=\"highlight highlight-source-rust\"><pre><span class=\"pl-k\">fn</span> <span class=\"pl-en\">a</span>() {}</pre></div>\
        <button>Kopieren</button><details><summary>Mehr</summary><p>Inhalt</p></details>";
    assert_eq!(
        md(html),
        "## Titel\n\nMenü\n\nText mit `code` und `Cmd`.\n\n```rust\nfn a() {}\n```\n\nMehr\n\nInhalt\n"
    );
}
