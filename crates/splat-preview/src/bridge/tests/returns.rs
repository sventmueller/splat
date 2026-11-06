//! Return and Shift-Return: splitting paragraphs, continuing lists and quotes, breaking lines.

use super::*;

#[test]
fn return_in_an_alert_continues_it() {
    assert_eq!(
        edited("> [!NOTE]\n> ab\n", |b| {
            let shown = shown_of("> [!NOTE]\n> ab\n");
            b.newline(Shown::new(
                shown[..shown.find("ab").unwrap()].encode_utf16().count() + 1,
            ))
        }),
        "> [!NOTE]\n> a\n>\n> b\n"
    );
}

#[test]
fn a_line_break_stays_in_the_one_line_of_a_cell_or_heading() {
    let table = "| a |\n|---|\n| xy |\n";
    assert_eq!(
        edited(table, |b| b.line_break(Shown::new(3))),
        "| a |\n|---|\n| x<br>y |\n"
    );
    assert_eq!(edited("# ab\n", |b| b.line_break(Shown::new(1))), "# a<br>b\n");
}

#[test]
fn return_in_a_code_block_adds_a_line_to_it() {
    assert_eq!(
        edited("```\nab\n```\n", |b| b.newline(Shown::new(1))),
        "```\na\nb\n```\n"
    );
    assert_eq!(
        edited("> ```\n> ab\n> ```\n", |b| b.newline(Shown::new(1))),
        "> ```\n> a\n> b\n> ```\n"
    );
}

#[test]
fn a_shift_return_breaks_the_line_and_keeps_the_prefix() {
    assert_eq!(edited("ab\n", |b| b.line_break(Shown::new(1))), "a\\\nb\n");
    assert_eq!(edited("- ab\n", |b| b.line_break(Shown::new(3))), "- a\\\n  b\n");
    assert_eq!(edited("> ab\n", |b| b.line_break(Shown::new(1))), "> a\\\n> b\n");
    assert_eq!(
        edited("```\nab\n```\n", |b| b.line_break(Shown::new(1))),
        "```\na\nb\n```\n"
    );
    assert_eq!(
        edited("x\n\n  ab\n", |b| b.line_break(Shown::new(3))),
        "x\n\n  a\\\n  b\n"
    );
}

#[test]
fn return_splits_a_paragraph_and_keeps_formatting_on_both_sides() {
    assert_eq!(edited("ab\n", |b| b.newline(Shown::new(1))), "a\n\nb\n");
    assert_eq!(edited("**ab** c\n", |b| b.newline(Shown::new(1))), "**a**\n\n**b** c\n");
    assert_eq!(edited("**ab** c\n", |b| b.newline(Shown::new(0))), "\n\n**ab** c\n");
    assert_eq!(edited("**ab** c\n", |b| b.newline(Shown::new(2))), "**ab**\n\n c\n");
    assert_eq!(
        edited("[ab](c) d\n", |b| b.newline(Shown::new(1))),
        "[a](c)\n\n[b](c) d\n"
    );
    assert_eq!(edited("`ab` c\n", |b| b.newline(Shown::new(1))), "`a`\n\n`b` c\n");
    assert_eq!(edited("> ab\n", |b| b.newline(Shown::new(1))), "> a\n>\n> b\n");
    assert_eq!(
        edited("x\n\n> ab\n", |b| b.newline(Shown::new(3))),
        "x\n\n> a\n>\n> b\n"
    );
    assert_eq!(edited("- [ ] ab\n", |b| b.newline(Shown::new(3))), "- [ ] a\n- [ ] b\n");
}

#[test]
fn return_inside_nested_formatting_closes_and_reopens_it_inside_out() {
    assert_eq!(
        edited("***ab*** c\n", |b| b.newline(Shown::new(1))),
        "***a***\n\n***b*** c\n"
    );
    assert_eq!(
        edited("**a *bc* d**\n", |b| b.newline(Shown::new(3))),
        "**a *b***\n\n***c* d**\n"
    );
    assert_eq!(
        edited("**a *b* c**\n", |b| b.newline(Shown::new(2))),
        "**a **\n\n***b* c**\n"
    );
    assert_eq!(edited("x **ab**\n", |b| b.newline(Shown::new(2))), "x \n\n**ab**\n");
    assert_eq!(edited("x **ab**\n", |b| b.newline(Shown::new(4))), "x **ab**\n\n\n");
    assert_eq!(edited("- **ab**\n", |b| b.newline(Shown::new(3))), "- **a**\n- **b**\n");
}

#[test]
fn a_split_list_item_and_a_quote_keep_what_stands_before_them() {
    assert_eq!(
        edited("x\n\n  - ab\n", |b| b.newline(Shown::new(5))),
        "x\n\n  - a\n  - b\n"
    );
    assert_eq!(
        edited("x\n\n- ab\n\ny\n", |b| b.newline(Shown::new(5))),
        "x\n\n- a\n- b\n\ny\n"
    );
    assert_eq!(
        edited("x\n\n```\nab\n```\n", |b| b.newline(Shown::new(3))),
        "x\n\n```\na\nb\n```\n"
    );
    assert_eq!(edited("x\n\n> \n", |b| b.newline(Shown::new(2))), "x\n\n\n");
}

#[test]
fn return_on_what_cannot_be_split_goes_behind_it() {
    assert_eq!(edited("---\n\nx\n", |b| b.newline(Shown::new(0))), "---\n\n\n\nx\n");
    assert_eq!(
        edited("<div>a</div>\n\nx\n", |b| b.newline(Shown::new(0))),
        "<div>a</div>\n\n\n\nx\n"
    );
    assert_eq!(edited("$$x$$\n\ny\n", |b| b.newline(Shown::new(0))), "$$x$$\n\n\n\ny\n");
}

#[test]
fn return_numbers_the_next_item_and_ends_a_list_on_an_empty_one() {
    assert_eq!(edited("1. a\n", |b| b.newline(Shown::new(4))), "1. a\n2. \n");
    assert_eq!(edited("7) a\n", |b| b.newline(Shown::new(4))), "7) a\n8) \n");
    assert_eq!(edited("x\n\n3. a\n", |b| b.newline(Shown::new(6))), "x\n\n3. a\n4. \n");
    assert_eq!(edited("- a\n- \n", |b| b.newline(Shown::new(6))), "- a\n\n");
    assert_eq!(
        edited("x\n\n  - a\n  - \n", |b| b.newline(Shown::new(8))),
        "x\n\n  - a\n\n"
    );
    assert_eq!(
        edited("> a\n>\n> \n", |b| b.replace(Shown::new(1)..Shown::new(1), "")),
        "> a\n>\n> \n"
    );
}

#[test]
fn enter_continues_lists_and_splits_bold() {
    let (s, caret) = edit("- eins\n", |b| b.newline(Shown::new(6)));
    assert_eq!(s, "- eins\n- \n");
    assert_eq!(caret, 9);
    let (s, _) = edit("1. a\n", |b| b.newline(Shown::new(4)));
    assert_eq!(s, "1. a\n2. \n");
    let (s, _) = edit("x **fett**\n", |b| b.newline(Shown::new(4)));
    assert_eq!(s, "x **fe**\n\n**tt**\n");
    let (s, _) = edit("- [x] a\n", |b| b.newline(Shown::new(3)));
    assert_eq!(s, "- [x] a\n- [ ] \n");
}

#[test]
fn return_adds_visible_empty_lines_and_backspace_removes_them() {
    let (s, caret) = edit("A\n\nB\n", |b| b.newline(Shown::new(1)));
    assert_eq!((s.as_str(), caret), ("A\n\n\n\nB\n", 3));
    let (s, caret) = edit(&s, |b| b.newline(Shown::new(2)));
    assert_eq!((s.as_str(), caret), ("A\n\n\n\n\n\nB\n", 5));
    let (s, caret) = edit(&s, |b| b.delete_at_block_start(Shown::new(3)));
    assert_eq!((s.as_str(), caret), ("A\n\n\n\nB\n", 3));
    let (s, _) = edit(&s, |b| b.replace(Shown::new(2)..Shown::new(2), "neu"));
    assert_eq!(s, "A\n\nneu\n\nB\n");
}

#[test]
fn enter_on_empty_item_leaves_list() {
    let (s, _) = edit("- a\n- \n", |b| b.newline(Shown::new(6)));
    assert_eq!(s, "- a\n\n");
}

#[test]
fn enter_in_quote_and_table() {
    let (s, _) = edit("> ab\n", |b| b.newline(Shown::new(1)));
    assert_eq!(s, "> a\n>\n> b\n");
    let (s, caret) = edit("| a | b |\n|---|---|\n| 1 | 2 |\n", |b| b.newline(Shown::new(5)));
    assert_eq!(s.lines().count(), 4);
    assert!(caret > s.find("| 1").unwrap());
}

#[test]
fn return_in_an_empty_item_or_quote_clears_the_whole_line() {
    let enter = |t: &str, p: usize| with_caret(t, |b| b.newline(Shown::new(p)));
    assert_eq!(
        enter("x\n\n-   \n\nnext\n", 4),
        "x\n\n|\n\nnext\n",
        "spaces behind the marker"
    );
    assert_eq!(enter("x\n\n> - \n\nnext\n", 4), "x\n\n>|\n\nnext\n", "the quote stays");
    assert_eq!(enter("x\n\n>   \n\nnext\n", 2), "x\n\n|\n\nnext\n", "an empty quote");
}

#[test]
fn return_inside_bold_in_a_list_leaves_the_caret_before_the_new_markers() {
    assert_eq!(
        with_caret("- **ab**\n", |b| b.newline(Shown::new(3))),
        "- **a**\n- |**b**\n"
    );
}
