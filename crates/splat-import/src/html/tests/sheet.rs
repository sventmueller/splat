//! Style sheets written to cost memory and time: one block for many selectors, the same rule again and again, an
//! element of many classes.

use super::*;
use crate::html::style::{MOST_ELEMENT_BYTES, MOST_RULES_PER_CLASS, MOST_SHEET_BYTES, Sheet};

fn element_of(class: &str) -> Vec<(String, String)> {
    vec![("class".to_string(), class.to_string())]
}

fn block(len: usize) -> String {
    format!("font-weight: bold;{}", "x".repeat(len))
}

#[test]
fn a_block_shared_by_many_selectors_is_kept_once() {
    let declarations = block(3000);
    let css = format!("{} {{{declarations}}}", vec![".c"; 10_000].join(","));
    let mut sheet = Sheet::default();
    sheet.read(&css);
    let (rules, blocks, bytes) = sheet.kept();
    assert_eq!((rules, blocks), (1, 1), "the same class with the same block");
    assert!(bytes < 4000, "{bytes}");
    let given = sheet.declarations("span", &element_of("c"));
    assert_eq!(given.len(), declarations.len() + 1);

    let classes: Vec<String> = (0..10_000).map(|n| format!(".c{n}")).collect();
    let mut sheet = Sheet::default();
    sheet.read(&format!("{} {{{declarations}}}", classes.join(",")));
    let (rules, blocks, bytes) = sheet.kept();
    assert_eq!((rules, blocks), (10_000, 1));
    assert!(
        bytes < declarations.len() + 10_000 * 200,
        "the block counts once: {bytes}"
    );
}

#[test]
fn equal_blocks_of_different_rules_are_kept_once() {
    let declarations = block(3000);
    let css: String = (0..100)
        .map(|n| format!("p.c{n} {{ {} }}\n", declarations.to_uppercase()))
        .collect();
    let mut sheet = Sheet::default();
    sheet.read(&css);
    let (rules, blocks, bytes) = sheet.kept();
    assert_eq!((rules, blocks), (100, 1));
    assert!(bytes < declarations.len() + 100 * 200, "{bytes}");
    assert_eq!(
        sheet.declarations("p", &element_of("c99")),
        format!("{declarations};"),
        "as the first rule wrote it, in lower case"
    );
    assert_eq!(
        sheet.declarations("span", &element_of("c99")),
        "",
        "for another element"
    );
}

#[test]
fn the_same_rule_again_is_kept_once_and_a_class_keeps_only_so_many() {
    let rule = format!(".c {{{}}} ", block(1000));
    let mut sheet = Sheet::default();
    sheet.read(&rule.repeat(1000));
    assert_eq!(sheet.kept().0, 1);
    sheet.read(&format!("p.c {{{}}}", block(1000)));
    assert_eq!(sheet.kept().0, 2, "another element is another rule");

    let distinct: String = (0..1000).map(|n| format!(".d {{font-weight: {n}}} ")).collect();
    let mut sheet = Sheet::default();
    sheet.read(&distinct);
    assert_eq!(sheet.kept().0, MOST_RULES_PER_CLASS);
    let given = sheet.declarations("span", &element_of("d"));
    assert_eq!(given.matches("font-weight").count(), MOST_RULES_PER_CLASS);
}

#[test]
fn a_rule_given_again_is_in_force_as_the_later_one() {
    let mut sheet = Sheet::default();
    sheet.read(".c {font-weight: bold} .c {font-weight: normal} .c {font-weight: bold}");
    assert_eq!(sheet.kept().0, 2);
    assert_eq!(
        sheet.declarations("span", &element_of("c")),
        "font-weight: normal;font-weight: bold;"
    );
    let html = "<html><head><meta name=\"Generator\" content=\"Cocoa HTML Writer\"><style>\
                .s1 {font-weight: bold} .s1 {font-weight: normal} .s1 {font-weight: bold}</style></head>\
                <body><p><span class=\"s1\">fett</span></p></body></html>";
    assert_eq!(md(html), "**fett**\n");
}

#[test]
fn a_sheet_keeps_no_more_than_its_limit() {
    let distinct: String = (0..100_000).map(|n| format!(".c{n} {{{}}} ", block(n % 7))).collect();
    let mut sheet = Sheet::default();
    sheet.read(&distinct);
    let (rules, blocks, bytes) = sheet.kept();
    assert!(bytes <= MOST_SHEET_BYTES, "{bytes}");
    assert!(rules > 1000 && rules < 100_000, "{rules}");
    assert_eq!(blocks, 7);
    sheet.read(".late {font-weight: bold}");
    assert!(
        sheet.declarations("span", &element_of("late")).is_empty(),
        "a full sheet takes no more"
    );
}

#[test]
fn a_block_too_large_for_the_sheet_fills_it() {
    let mut sheet = Sheet::default();
    sheet.read(&format!(
        ".a {{font-weight: bold}} .b {{{}}} .c {{font-weight: bold}}",
        block(MOST_SHEET_BYTES)
    ));
    assert_eq!(sheet.kept().0, 1);
    assert_eq!(sheet.declarations("span", &element_of("a")), "font-weight: bold;");
    assert!(sheet.declarations("span", &element_of("b c")).is_empty());
}

#[test]
fn an_element_takes_no_more_than_its_limit_from_the_sheet() {
    let classes: Vec<String> = (0..1000).map(|n| format!("c{n}")).collect();
    let selectors: Vec<String> = classes.iter().map(|class| format!(".{class}")).collect();
    let mut sheet = Sheet::default();
    sheet.read(&format!("{} {{{}}}", selectors.join(","), block(1000)));
    let given = sheet.declarations("span", &element_of(&classes.join(" ")));
    assert!(
        given.len() <= MOST_ELEMENT_BYTES && given.len() > 1000,
        "{}",
        given.len()
    );
    let many = vec!["c0"; 100_000].join(" ");
    assert!(sheet.declarations("span", &element_of(&many)).len() <= MOST_ELEMENT_BYTES);
    let mut huge = Sheet::default();
    huge.read(&format!(".h {{{}}}", block(MOST_ELEMENT_BYTES)));
    assert!(
        huge.declarations("span", &element_of("h")).is_empty(),
        "a block beyond it is left out"
    );
}

#[test]
fn a_page_from_cocoa_with_a_sheet_written_to_hurt_still_reads() {
    let selectors = vec![".s1"; 20_000].join(",");
    let html = format!(
        "<html><head><meta name=\"Generator\" content=\"Cocoa HTML Writer\"><style>{selectors} {{{}}}</style></head>\
         <body>{}</body></html>",
        block(2000),
        "<p><span class=\"s1\">fett</span></p>".repeat(2000)
    );
    assert_eq!(md(&html).matches("**fett**").count(), 2000);
}
