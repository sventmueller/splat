//! Runs: their look, and what they hold besides text.

use super::*;

#[test]
fn a_drop_cap_stays_with_the_word_it_starts() {
    let cap = |kind: &str, letter: &str| {
        format!(
            "<w:p><w:pPr><w:framePr w:dropCap=\"{kind}\" w:lines=\"3\"/></w:pPr>{}</w:p>",
            t(letter)
        )
    };
    assert_eq!(
        markdown(&format!("{}{}", cap("drop", "D"), p(&t("rop cap.")))),
        "Drop cap.\n"
    );
    assert_eq!(
        markdown(&format!("{}{}", cap("margin", "D"), p(&t("rop cap.")))),
        "Drop cap.\n"
    );
    assert_eq!(
        markdown(&format!("{}{}", cap("none", "Frei"), p(&t("Text")))),
        "Frei\n\nText\n",
        "a frame that holds no initial"
    );
    assert_eq!(
        markdown(&cap("drop", "D")),
        "D\n",
        "an initial with nothing behind it stays"
    );
}

#[test]
fn runs_of_one_look_are_one_word() {
    assert_eq!(markdown(&p(&format!("{}{}", t("Hal"), t("lo")))), "Hallo\n");
    assert_eq!(
        markdown(&p(&format!(
            "{}{}",
            marked("<w:b/>", "Hal"),
            marked("<w:b/><w:color w:val=\"FF0000\"/>", "lo")
        ))),
        "**Hallo**\n"
    );
    assert_eq!(
        markdown(&p(&format!("{}{} und", marked("<w:b/>", "fett "), t("")))),
        "**fett**\n"
    );
}

#[test]
fn marks_become_markdown() {
    for (props, wanted) in [
        ("<w:b/>", "a **x** b"),
        ("<w:i/>", "a *x* b"),
        ("<w:strike/>", "a ~~x~~ b"),
        ("<w:dstrike/>", "a ~~x~~ b"),
        ("<w:u w:val=\"single\"/>", "a <ins>x</ins> b"),
        ("<w:highlight w:val=\"yellow\"/>", "a <mark>x</mark> b"),
        ("<w:vertAlign w:val=\"superscript\"/>", "a <sup>x</sup> b"),
        ("<w:vertAlign w:val=\"subscript\"/>", "a <sub>x</sub> b"),
        ("<w:rFonts w:ascii=\"Consolas\"/>", "a `x` b"),
        ("<w:b w:val=\"0\"/>", "a x b"),
        ("<w:vanish/>", "a  b"),
    ] {
        let body = p(&format!("{}{}{}", t("a "), marked(props, "x"), t(" b")));
        assert_eq!(markdown(&body), format!("{wanted}\n"), "{props}");
    }
}

#[test]
fn a_style_of_a_run_counts_and_toggles_against_that_of_its_paragraph() {
    let fixture = with_styles(&format!(
        "{}{}{}{}{}",
        style("Fett", "Fett", "<w:rPr><w:b/></w:rPr>"),
        "<w:style w:type=\"character\" w:styleId=\"Strong\"><w:name w:val=\"Strong\"/><w:rPr><w:b/></w:rPr></w:style>",
        "<w:style w:type=\"character\" w:styleId=\"Under\"><w:name w:val=\"Under\"/><w:rPr><w:u w:val=\"single\"/><w:rFonts w:ascii=\"Menlo\"/></w:rPr></w:style>",
        "<w:style w:type=\"character\" w:styleId=\"CodeZeichen\"><w:name w:val=\"Code Char\"/></w:style>",
        style("Kursiv", "Kursiv", "<w:rPr><w:i/></w:rPr>"),
    ));
    let run =
        |style: &str, text: &str| format!("<w:r><w:rPr><w:rStyle w:val=\"{style}\"/></w:rPr><w:t>{text}</w:t></w:r>");
    assert_eq!(
        fixture.markdown(&p(&format!("{}{}", run("Strong", "a"), t(" b")))),
        "**a** b\n"
    );
    assert_eq!(
        fixture.markdown(&styled("Fett", &format!("{}{}", t("a"), run("Strong", "b")))),
        "**a**b\n",
        "bold in both cancels out"
    );
    assert_eq!(
        fixture.markdown(&styled(
            "Fett",
            &format!("{}{}", t("a"), marked("<w:b w:val=\"0\"/>", "b"))
        )),
        "**a**b\n",
        "what the run itself says wins"
    );
    assert_eq!(
        fixture.markdown(&p(&format!("{}{}{}", t("x "), run("Under", "a"), t(" y")))),
        "x <ins>`a`</ins> y\n"
    );
    assert_eq!(
        fixture.markdown(&p(&format!("{}{}{}", t("x "), run("CodeZeichen", "a"), t(" y")))),
        "x `a` y\n"
    );
    assert_eq!(fixture.markdown(&p(&run("CodeZeichen", "only"))), "`only`\n");
    assert_eq!(fixture.markdown(&p(&run("Unknown", "a"))), "a\n");
    assert_eq!(fixture.markdown(&styled("Kursiv", &t("a"))), "*a*\n");
}

#[test]
fn what_runs_hold_besides_text() {
    assert_eq!(
        markdown(&p(
            "<w:r><w:t>a</w:t><w:tab/><w:t>b</w:t><w:br/><w:t>c</w:t><w:cr/><w:t>d</w:t><w:br w:type=\"page\"/><w:br w:type=\"column\"/><w:t>e</w:t><w:noBreakHyphen/><w:softHyphen/><w:t>f</w:t><w:sym w:font=\"Symbol\" w:char=\"F061\"/><w:sym/><w:lastRenderedPageBreak/></w:r>"
        )),
        "a\tb\\\nc\\\nde\u{2011}fα\n"
    );
    assert_eq!(
        markdown(&p("<w:r><w:t>a\nb</w:t></w:r>")),
        "a b\n",
        "a line break in a text is a space"
    );
    assert_eq!(
        markdown(&p("<w:r><w:rPr><w:vanish/></w:rPr><w:br/><w:t>x</w:t></w:r>")),
        ""
    );
}
