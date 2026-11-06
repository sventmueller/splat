//! Lists by numbering, style and indent.

use super::*;

#[test]
fn lists_nest_by_level_and_know_their_numbers() {
    let fixture = Fixture {
        numbering: Some(
            "<w:abstractNum w:abstractNumId=\"0\">\
               <w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/></w:lvl>\
               <w:lvl w:ilvl=\"1\"><w:start w:val=\"5\"/><w:numFmt w:val=\"decimal\"/></w:lvl>\
             </w:abstractNum>\
             <w:abstractNum w:abstractNumId=\"1\">\
               <w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/></w:lvl>\
               <w:lvl w:ilvl=\"1\"><w:numFmt w:val=\"none\"/></w:lvl>\
             </w:abstractNum>\
             <w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>\
             <w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num>\
             <w:num w:numId=\"3\"><w:abstractNumId w:val=\"1\"/><w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"7\"/></w:lvlOverride></w:num>"
                .to_string(),
        ),
        ..Fixture::default()
    };
    let item = |id: u32, level: u32, text: &str| {
        format!(
            "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{id}\"/></w:numPr></w:pPr>{}</w:p>",
            t(text)
        )
    };
    assert_eq!(
        fixture.markdown(&format!(
            "{}{}{}{}",
            item(1, 0, "a"),
            item(1, 1, "b"),
            item(1, 1, "c"),
            item(1, 0, "d")
        )),
        "- a\n\n  5. b\n  6. c\n- d\n"
    );
    assert_eq!(fixture.markdown(&item(3, 0, "x")), "7. x\n");
    assert_eq!(
        fixture.markdown(&format!("{}{}{}", item(2, 0, "a"), p(&t("zwischen")), item(2, 0, "b"))),
        "1. a\n\nzwischen\n\n2. b\n",
        "numbering goes on behind what stands between"
    );
    assert_eq!(
        fixture.markdown(&format!("{}{}{}", item(2, 0, "a"), p(""), item(2, 0, "b"))),
        "1. a\n2. b\n",
        "an empty paragraph does not end a list"
    );
    assert_eq!(fixture.markdown(&item(2, 1, "ohne Marke")), "ohne Marke\n");
    assert_eq!(fixture.markdown(&item(0, 0, "nein")), "nein\n");
    assert_eq!(fixture.markdown(&item(77, 0, "unbekannt")), "- unbekannt\n");
    assert_eq!(fixture.markdown(&item(1, 40, "tief")), "- tief\n");
    assert_eq!(
        fixture.markdown(&format!("{}{}", item(2, 0, "a"), item(1, 0, "b"))),
        "1. a\n\n- b\n"
    );
}

#[test]
fn a_style_can_make_a_list_and_a_paragraph_can_leave_it() {
    let fixture = Fixture {
        styles: Some(style(
            "Punkt",
            "List Bullet",
            "<w:pPr><w:numPr><w:numId w:val=\"1\"/></w:numPr></w:pPr>",
        )),
        numbering: Some(
            "<w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"bullet\"/></w:lvl></w:abstractNum>\
             <w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>"
                .to_string(),
        ),
        ..Fixture::default()
    };
    assert_eq!(fixture.markdown(&styled("Punkt", &t("a"))), "- a\n");
    assert_eq!(
        fixture.markdown(&format!(
            "<w:p><w:pPr><w:pStyle w:val=\"Punkt\"/><w:numPr><w:numId w:val=\"0\"/></w:numPr></w:pPr>{}</w:p>",
            t("b")
        )),
        "b\n"
    );
}

#[test]
fn a_checkbox_in_front_of_an_item_makes_it_a_task() {
    let item = |text: &str| {
        format!(
            "<w:p><w:pPr><w:numPr><w:numId w:val=\"5\"/></w:numPr></w:pPr>{}</w:p>",
            t(text)
        )
    };
    assert_eq!(
        markdown(&format!("{}{}{}{}", item("☐ a"), item("☑ b"), item("☒c"), item("d"))),
        "- [ ] a\n- [x] b\n- [x] c\n- d\n"
    );
    assert_eq!(markdown(&p(&t("☐ a"))), "☐ a\n");
    let code = format!(
        "<w:p><w:pPr><w:numPr><w:numId w:val=\"5\"/></w:numPr></w:pPr>{}</w:p>",
        marked("<w:rFonts w:ascii=\"Menlo\"/>", "x")
    );
    assert_eq!(markdown(&code), "- `x`\n");
}

#[test]
fn a_paragraph_indented_behind_an_item_belongs_to_it() {
    let item = "<w:p><w:pPr><w:numPr><w:numId w:val=\"5\"/></w:numPr></w:pPr><w:r><w:t>a</w:t></w:r></w:p>";
    let indented = "<w:p><w:pPr><w:ind w:left=\"720\"/></w:pPr><w:r><w:t>b</w:t></w:r></w:p>";
    assert_eq!(markdown(&format!("{item}{indented}")), "- a\n\n  b\n");
    assert_eq!(markdown(indented), "b\n");
    assert_eq!(
        markdown(&format!(
            "{item}<w:p><w:pPr><w:ind w:left=\"100\"/></w:pPr><w:r><w:t>b</w:t></w:r></w:p>"
        )),
        "- a\n\nb\n"
    );
}

#[test]
fn a_list_typed_as_text_with_tabs_becomes_a_list_by_its_margins() {
    let item = |left: u32, marker: &str, text: &str| {
        format!(
            "<w:p><w:pPr><w:ind w:left=\"{left}\" w:first-line=\"-{left}\"/></w:pPr><w:r><w:t xml:space=\"preserve\"></w:t><w:tab/>\
             <w:t>{marker}</w:t><w:tab/><w:t xml:space=\"preserve\">{text}</w:t></w:r></w:p>"
        )
    };
    assert_eq!(
        markdown(&format!(
            "{}{}{}{}{}{}",
            item(720, "•", "a"),
            item(1440, "1.", "b"),
            item(1440, "2.", "c"),
            item(720, "◦", "d"),
            p(&t("zwischen")),
            item(360, "☑", "e"),
        )),
        "- a\n  1. b\n  2. c\n- d\n\nzwischen\n\n- [x] e\n"
    );
    let hanging =
        |indent: &str, text: &str| format!("<w:p><w:pPr><w:ind w:left=\"720\" {indent}/></w:pPr>{}</w:p>", t(text));
    assert_eq!(markdown(&hanging("w:hanging=\"360\"", "a)\tb")), "1. b\n");
    assert_eq!(markdown(&hanging("w:firstLine=\"-360\"", "–\tb")), "- b\n");
    assert_eq!(
        markdown(&hanging("w:firstLine=\"360\"", "–\tb")),
        "–\tb\n",
        "no hanging indent, no list"
    );
    assert_eq!(
        markdown(&hanging("w:hanging=\"360\"", "Wort\tb")),
        "Wort\tb\n",
        "a word is no marker"
    );
    assert_eq!(markdown(&hanging("w:hanging=\"360\"", "a b\tc")), "a b\tc\n");
    assert_eq!(
        markdown(&hanging("w:hanging=\"360\"", "1.2.3.4.5.6.7.\tb")),
        "1.2.3.4.5.6.7.\tb\n"
    );
    assert_eq!(
        markdown(&hanging("w:hanging=\"360\"", "•")),
        "•\n",
        "a marker needs its tab"
    );
    assert_eq!(
        markdown(&hanging("w:hanging=\"360\"", "•\t")),
        "",
        "nothing behind the marker is no item"
    );
}
