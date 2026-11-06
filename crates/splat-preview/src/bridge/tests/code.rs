//! Typing in code spans and code blocks.

use super::*;

#[test]
fn what_is_typed_in_code_stays_as_it_is() {
    bridge_of!("```\nab\n```\n\ncd\n" => d, l, index, b);
    assert_eq!(b.escaped(Shown::new(1), "*", false).unwrap(), "*", "in a code block");
    assert_eq!(
        b.escaped(Shown::new(4), "*", true).unwrap(),
        "*",
        "when the caller knows it is code"
    );
    assert_eq!(b.escaped(Shown::new(4), "*", false).unwrap(), "\\*", "in running text");
}

#[test]
fn the_caret_stays_behind_a_backtick_typed_in_a_code_span() {
    let t = "a `bc` d\n";
    bridge_of!(t => d, l, index, b);
    let edit = b.replace(Shown::new(3)..Shown::new(3), "`").unwrap();
    let mut out = t.to_string();
    let caret = edit.selection.start;
    edit.apply_to(&mut out);
    assert_eq!(out.upto(caret), "a ``b`");
    let at_edge = b.replace(Shown::new(2)..Shown::new(2), "`").unwrap();
    let mut out = t.to_string();
    at_edge.apply_to(&mut out);
    assert!(out.contains('`'), "{out}");
    assert_eq!(
        b.backticks_in_code_span(Byte::new(0), Byte::new(0), "`"),
        None,
        "outside the span"
    );
    assert_eq!(
        b.backticks_in_code_span(Byte::new(t.find('`').unwrap()), Byte::new(4), "`"),
        None,
        "on its delimiter"
    );
}

#[test]
fn a_backtick_typed_in_a_code_span_lengthens_its_delimiters() {
    assert_eq!(
        edited("a `bc` d\n", |b| b.replace(Shown::new(3)..Shown::new(3), "`")),
        "a ``b`c`` d\n"
    );
    assert_eq!(
        edited("a ``b`c`` d\n", |b| b.replace(Shown::new(4)..Shown::new(4), "`")),
        "a ```b``c``` d\n"
    );
    assert_eq!(
        edited("a `bc` d\n", |b| b.replace(Shown::new(3)..Shown::new(4), "`")),
        "a `` b` `` d\n"
    );
}

#[test]
fn a_backtick_typed_in_a_code_span_lengthens_the_span_it_is_in() {
    let typed = |t: &str, p: usize| with_caret(t, |b| b.replace(Shown::new(p)..Shown::new(p), "`"));
    assert_eq!(typed("`ab` c", 0), "`` `|ab `` c", "at its start");
    assert_eq!(typed("xx `ab` yy", 4), "xx ``a`|b`` yy", "inside");
    assert_eq!(typed("`a` `b`", 1), "`` a`| `` `b`", "not the next one");
}

#[test]
fn typing_and_deleting_at_the_end_of_code_stays_inside_the_block() {
    let typed = |t: &str, p: usize| edited(t, |b| b.replace(Shown::new(p)..Shown::new(p), "c"));
    assert_eq!(typed("```\nab\n```\n", 2), "```\nabc\n```\n");
    assert_eq!(typed("~~~\nab\n~~~\n", 2), "~~~\nabc\n~~~\n");
    assert_eq!(typed("> ```\n> ab\n> ```\n", 2), "> ```\n> abc\n> ```\n");
    assert_eq!(typed("- ```\n  ab\n  ```\n", 2 + 2), "- ```\n  abc\n  ```\n");
    assert_eq!(typed("    ab\n", 2), "    abc\n");
    assert_eq!(typed("```\nab\n", 2), "```\nabc\n", "without a closing fence");
    let deleted = |t: &str, r: Range<usize>| edited(t, |b| b.replace(Shown::new(r.start)..Shown::new(r.end), ""));
    assert_eq!(deleted("```\nab\n```\n", 1..2), "```\na\n```\n");
    assert_eq!(deleted("```\nab\ncd\n```\n", 4..5), "```\nab\nc\n```\n");
    assert_eq!(deleted("x\n\n```\nab\n```\n\ny\n", 3..4), "x\n\n```\na\n```\n\ny\n");
    assert_eq!(deleted("> ```\n> ab\n> ```\n", 1..2), "> ```\n> a\n> ```\n");
    assert_eq!(
        edited("```\nab\n```\n", |b| b.newline(Shown::new(2))),
        "```\nab\n\n```\n"
    );
}
