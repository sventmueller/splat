//! Tab and Shift-Tab on list items, and Backspace at the start of a block.

use super::*;

#[test]
fn a_quote_inside_an_item_moves_with_it_and_one_around_the_list_stays_in_front() {
    let tab = |t: &str, at: usize| edited(t, |b| b.tab(Shown::new(at), Direction::Forward));
    let t = "- p\n- a\n  > q\n";
    let at = shown_of(t).find('a').unwrap();
    assert_eq!(tab(t, at), "- p\n  - a\n    > q\n");
    let t = "> - p\n> - a\n>   b\n";
    let at = shown_of(t).find('a').unwrap();
    assert_eq!(tab(t, at), "> - p\n>   - a\n>     b\n");
}

#[test]
fn tab_moves_a_list_item_with_what_belongs_to_it() {
    let t = "- a\n- b\n  - c\n\n    d\n- e\n";
    let shown = shown_of(t);
    let at = shown[..shown.find('b').unwrap()].encode_utf16().count();
    assert_eq!(
        edited(t, |b| b.tab(Shown::new(at), Direction::Forward)),
        "- a\n  - b\n    - c\n\n      d\n- e\n"
    );
}

#[test]
fn shift_tab_moves_a_list_item_with_what_belongs_to_it() {
    let t = "- a\n  - b\n    - c\n- e\n";
    let shown = shown_of(t);
    let at = shown[..shown.find('b').unwrap()].encode_utf16().count();
    assert_eq!(
        edited(t, |b| b.tab(Shown::new(at), Direction::Backward)),
        "- a\n- b\n  - c\n- e\n"
    );
}

#[test]
fn tab_moves_nested_lines_behind_the_marks_of_a_quote() {
    let t = "> - a\n> - b\n>   - c\n";
    let shown = shown_of(t);
    let at = shown[..shown.find('b').unwrap()].encode_utf16().count();
    assert_eq!(
        edited(t, |b| b.tab(Shown::new(at), Direction::Forward)),
        "> - a\n>   - b\n>     - c\n"
    );
}

#[test]
fn tab_indents_a_list_item_behind_the_marks_of_a_quote() {
    let t = "> - a\n> - b\n";
    let at = shown_of(t).find('b').unwrap();
    let shown = shown_of(t);
    let at = shown[..at].encode_utf16().count();
    assert_eq!(
        edited(t, |b| b.tab(Shown::new(at), Direction::Forward)),
        "> - a\n>   - b\n"
    );
}

#[test]
fn tab_indents_a_list_item_under_the_one_before_and_back() {
    assert_eq!(
        edited("- a\n- b\n", |b| b.tab(Shown::new(6), Direction::Forward)),
        "- a\n  - b\n"
    );
    assert_eq!(
        edited("x\n\n1. a\n2. b\n", |b| b.tab(Shown::new(8), Direction::Forward)),
        "x\n\n1. a\n   2. b\n"
    );
    assert_eq!(
        edited("- a\n  - b\n", |b| b.tab(Shown::new(6), Direction::Backward)),
        "- a\n- b\n"
    );
    assert_eq!(
        edited("x\n\n- a\n  - b\n", |b| b.tab(Shown::new(8), Direction::Backward)),
        "x\n\n- a\n- b\n"
    );
    assert_eq!(
        edited("- a\n  - b\n    - c\n", |b| b.tab(Shown::new(10), Direction::Backward)),
        "- a\n  - b\n  - c\n"
    );
}

#[test]
fn tab_keeps_the_caret_on_its_character_and_leaves_what_cannot_move() {
    let caret = |t: &str, p: usize, way: Direction| {
        bridge_of!(t => d, l, index, b);
        b.tab(Shown::new(p), way).map(|edit| {
            let at = edit.selection.start;
            assert_eq!(edit.selection.end, at);
            let mut out = t.to_string();
            edit.apply_to(&mut out);
            out.from(at).chars().next()
        })
    };
    assert_eq!(caret("- a\n- bc\n", 7, Direction::Forward), Some(Some('c')));
    assert_eq!(caret("- a\n  - bc\n", 7, Direction::Backward), Some(Some('c')));
    assert_eq!(
        caret("- a\n", 2, Direction::Forward),
        None,
        "the first item has none to go under"
    );
    assert_eq!(
        caret("- a\n", 2, Direction::Backward),
        None,
        "nor a level to go back to"
    );
    assert_eq!(caret("Text\n", 2, Direction::Forward), None);
}

#[test]
fn backspace_at_the_start_of_a_block_takes_its_formatting_away() {
    let backspace = |t: &str, p: usize| edited(t, |b| b.delete_at_block_start(Shown::new(p)));
    assert_eq!(backspace("x\n\n# Titel\n", 2), "x\n\nTitel\n");
    assert_eq!(backspace("x\n\n> Zitat\n", 2), "x\n\nZitat\n");
    assert_eq!(backspace("x\n\n>Zitat\n", 2), "x\n\nZitat\n");
    assert_eq!(backspace("x\n\n- Punkt\n", 2 + 2), "x\n\nPunkt\n");
    assert_eq!(backspace("x\n\n- [ ] Aufgabe\n", 2 + 2), "x\n\nAufgabe\n");
    assert_eq!(backspace("x\n\n1. Eins\n", 2 + 3), "x\n\nEins\n");
    // Inside a table or code it stays where it is, without a step to undo.
    assert_eq!(backspace("```\nab\n```\n", 0), "```\nab\n```\n");
    assert_eq!(backspace("| a |\n|---|\n| b |\n", 2), "| a |\n|---|\n| b |\n");
}

#[test]
fn backspace_inside_a_block_or_in_plain_text_is_left_to_the_ordinary_deletion() {
    let t = "x\n\n# Titel\n\nText\n";
    bridge_of!(t => d, l, index, b);
    assert_eq!(b.delete_at_block_start(Shown::new(3)), None, "inside the heading");
    assert_eq!(
        b.delete_at_block_start(Shown::new(8)),
        None,
        "at the start of a plain paragraph"
    );
}

#[test]
fn backspace_at_start_removes_block_prefix() {
    let (s, _) = edit("## Titel\n", |b| b.delete_at_block_start(Shown::new(0)));
    assert_eq!(s, "Titel\n");
    let (s, _) = edit("- a\n", |b| b.delete_at_block_start(Shown::new(2)));
    assert_eq!(s, "a\n");
}

#[test]
fn task_toggle_and_tab_indent() {
    let (s, _) = edit("- [ ] a\n", |b| b.toggle_task(Shown::new(0)));
    assert_eq!(s, "- [x] a\n");
    let (s, _) = edit("- a\n- b\n", |b| b.tab(Shown::new(6), Direction::Forward));
    assert_eq!(s, "- a\n  - b\n");
    let (s, _) = edit("- a\n  - b\n", |b| b.tab(Shown::new(6), Direction::Backward));
    assert_eq!(s, "- a\n- b\n");
}

#[test]
fn backspace_takes_a_quoted_heading_marker_and_leaves_quote_marks_in_code() {
    assert_eq!(
        with_caret("x\n\n> # Titel\n", |b| b.delete_at_block_start(Shown::new(2))),
        "x\n\n> |Titel\n"
    );
    assert_eq!(
        edited("```\n> a\n```\n", |b| b.delete_at_block_start(Shown::new(0))),
        "```\n> a\n```\n"
    );
}
