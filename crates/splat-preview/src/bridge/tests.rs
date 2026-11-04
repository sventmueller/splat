use super::escape::*;
use super::keys::*;
use super::*;
use crate::layout::RunMap;
use splat_markdown::commands::Pending;

#[test]
fn a_layout_of_another_parse_opens_no_bridge() {
    let t = "Text\n";
    let doc = splat_markdown::reader::parse(t);
    let laid_out = crate::layout::layout(&doc, &crate::layout::NoImages, None);
    let index = Index::new(&laid_out);
    assert!(Bridge::new(&doc, &laid_out, &index).is_some());
    let same_text = splat_markdown::reader::parse(t);
    assert!(Bridge::new(&same_text, &laid_out, &index).is_none());
    let nothing = Layout::nothing_of(&same_text);
    assert!(Bridge::new(&same_text, &nothing, &Index::new(&nothing)).is_some());
    assert!(Bridge::new(&doc, &Layout::default(), &Index::default()).is_none());
}
use splat_markdown::text::{Direction, bytes};

/// The document of a text, its layout, its index and the bridge between them.
macro_rules! bridge_of {
    ($text:expr => $doc:ident, $layout:ident, $index:ident, $bridge:ident) => {
        let $doc = splat_markdown::reader::parse($text);
        let $layout = crate::layout::layout(&$doc, &crate::layout::NoImages, None);
        let $index = Index::new(&$layout);
        let $bridge = Bridge::new(&$doc, &$layout, &$index).unwrap();
    };
}

/// What a text shows in the preview.
fn parse_shown(t: &str) -> Vec<String> {
    let doc = splat_markdown::reader::parse(t);
    let shown = crate::layout::layout(&doc, &crate::layout::NoImages, None);
    shown
        .blocks
        .iter()
        .flat_map(|block| &block.paras)
        .map(|para| para.runs.iter().map(|run| run.text.as_str()).collect())
        .collect()
}

/// The source after `edit` of the preview of `t`.
fn edited(t: &str, edit: impl Fn(&Bridge) -> Option<Edit>) -> String {
    bridge_of!(t => d, l, index, bridge);
    let mut out = t.to_string();
    edit(&bridge).expect("an edit").apply_to(&mut out);
    out
}

#[test]
fn joining_two_paragraphs_keeps_what_lies_hidden_between_them() {
    let join = |t: &str, p: usize| edited(t, |b| b.replace(Shown::new(p - 1)..Shown::new(p), ""));
    assert_eq!(join("A\n\n<!-- n -->\n\nB\n", 2), "AB\n\n<!-- n -->\n");
    assert_eq!(join("A\n\n[r]: http://x.y\n\nB\n", 2), "AB\n\n[r]: http://x.y\n");
    assert_eq!(
        join("A [r]\n\n[r]: http://x.y\n\nB\n", 4),
        "A [r]B\n\n[r]: http://x.y\n"
    );
    assert_eq!(join("A\r\n\r\n<!-- n -->\r\n\r\nB\r\n", 2), "AB\r\n\r\n<!-- n -->\r\n");
    assert_eq!(join("A\n\n<!-- n -->\n\nB\n\nC\n", 2), "AB\n\n<!-- n -->\n\nC\n");
}

#[test]
fn joining_two_paragraphs_keeps_the_markup_around_their_text() {
    let join = |t: &str, p: usize| edited(t, |b| b.replace(Shown::new(p - 1)..Shown::new(p), ""));
    assert_eq!(join("A **b**\n\nC\n", 4), "A **b**C\n");
    assert_eq!(join("A\n\n**b** C\n", 2), "A**b** C\n");
    assert_eq!(join("[a](http://x)\n\nB\n", 2), "[a](http://x)B\n");
    assert_eq!(join("*a*\n\n*b*\n", 2), "*ab*\n", "one span where both ends match");
    assert_eq!(join("- a\n- b\n", 4), "- ab\n");
}

#[test]
fn a_selection_across_paragraphs_goes_with_all_it_holds() {
    let after = edited("`a` b\n\nc *d* e\n", |b| b.replace(Shown::new(1)..Shown::new(6), ""));
    assert_eq!(parse_shown(&after), parse_shown("`a`*d* e\n"), "{after:?}");
}

#[test]
fn a_space_and_a_letter_at_the_end_of_a_windows_line_stay_in_order() {
    assert_eq!(typed_after("Wort\r\n", "Wort", " z"), "Wort z\r\n");
    assert_eq!(typed_after("Wort\r\n\r\nB\r\n", "Wort", " z"), "Wort z\r\n\r\nB\r\n");
    assert_eq!(typed_after("Wort\r", "Wort", " z"), "Wort z\r");
    assert_eq!(typed_after("Wort\n", "Wort", " z"), "Wort z\n");
}

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
fn the_chevron_of_an_embedded_note_flips_nothing_in_this_text() {
    struct Note;
    impl crate::layout::Resolver for Note {
        fn math_ready(&self, _: &str, _: bool) -> bool {
            false
        }
        fn diagram_ready(&self, _: &str, _: splat_markdown::doc::DiagramKind) -> bool {
            false
        }
        fn embedded_note(&self, _: &str) -> Option<(String, String)> {
            Some(("> [!NOTE]- Titel\n> Inhalt\n".to_string(), String::new()))
        }
    }
    let t = "![[Notiz]]\n";
    let d = splat_markdown::reader::parse(t);
    let l = crate::layout::layout(&d, &Note, None);
    let index = Index::new(&l);
    let bridge = Bridge::new(&d, &l, &index).unwrap();
    let chevron = l
        .blocks
        .iter()
        .flat_map(|b| &b.paras)
        .flat_map(|p| p.runs_at(index.paras[0].start))
        .find(|(_, r)| {
            matches!(
                r.map,
                RunMap::Atom {
                    atom: crate::layout::Atom::Fold { .. },
                    ..
                }
            )
        })
        .map(|(at, _)| at.start)
        .expect("a chevron");
    assert_eq!(bridge.toggle_fold(chevron), None);
}

#[test]
fn the_chevron_of_a_callout_in_this_text_flips_its_sign() {
    let t = "x\n\n> [!NOTE]- Titel\n> Inhalt\n";
    bridge_of!(t => d, l, index, bridge);
    let place = index.paras[1];
    let chevron = index
        .para(&l, 1)
        .runs_at(place.start)
        .find(|(_, r)| {
            matches!(
                r.map,
                RunMap::Atom {
                    atom: crate::layout::Atom::Fold { .. },
                    ..
                }
            )
        })
        .map(|(at, _)| at.start)
        .expect("a chevron");
    let mut out = t.to_string();
    bridge.toggle_fold(chevron).expect("an edit").apply_to(&mut out);
    assert_eq!(out, "x\n\n> [!NOTE]+ Titel\n> Inhalt\n");
}

/// What the preview shows of `t`, its paragraphs in lines.
fn shown_of(t: &str) -> String {
    let d = splat_markdown::reader::parse(t);
    let l = crate::layout::layout(&d, &crate::layout::NoImages, None);
    l.blocks
        .iter()
        .flat_map(|b| &b.paras)
        .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Types `keys` one by one behind `after` in what the preview shows, each on the text the one before left, styles
/// that wait for the next word included.
fn typed_after(t: &str, after: &str, keys: &str) -> String {
    let mut text = t.to_string();
    let shown = shown_of(t);
    let start = shown[..shown.find(after).unwrap() + after.len()].encode_utf16().count();
    let mut caret = None;
    let mut pending: Option<Pending> = None;
    for key in keys.chars() {
        let d = splat_markdown::reader::parse(&text);
        let l = crate::layout::layout(&d, &crate::layout::NoImages, caret);
        let index = Index::new(&l);
        let p = caret.map_or(start, |at| crate::map::preview_offset(&l, &index, at).get());
        let bridge = Bridge::new(&d, &l, &index).unwrap();
        let key_text = key.to_string();
        let waiting = pending
            .as_ref()
            .and_then(|w| bridge.type_with_pending(w, Shown::new(p), &key_text));
        let (edit, next) = match waiting {
            Some(typed) => typed,
            None => bridge
                .replace_keeping_styles(Shown::new(p)..Shown::new(p), &key_text)
                .unwrap_or_else(|| panic!("{key:?} refused at {p} of {text:?}")),
        };
        pending = next;
        caret = Some(edit.selection.end);
        let mut new = text.clone();
        edit.apply_to(&mut new);
        text = new;
    }
    text
}

#[test]
fn typing_never_turns_text_into_syntax() {
    for (start, after, keys, shows) in [
        ("[a] x\n", "[a]", ":", "[a]: x"),
        ("x [Link](#a)\n", "x ", "!", "x !Link"),
        ("Oktober\n", "", "3. ", "3. Oktober"),
        ("foo\nbar\n", "foo ", "# ", "foo # bar"),
        ("foo\nbar\n", "foo ", "- ", "foo - bar"),
        ("# Issue\n", "Issue", " #12", "Issue #12"),
        ("amp; x\n", "", "&", "&amp; x"),
        ("smile: x\n", "smile", ":", "smile:: x"),
        ("[a] x\n", "[a]", "(b)", "[a](b) x"),
    ] {
        let text = typed_after(start, after, keys);
        assert_eq!(shown_of(&text), shows, "{start:?} + {keys:?} -> {text:?}");
    }
}

#[test]
fn a_space_at_the_end_of_a_span_leaves_it_and_the_next_word_takes_up_its_style() {
    assert_eq!(typed_after("**ab** c\n", "ab", " "), "**ab**  c\n");
    let text = typed_after("**ab** c\n", "ab", " de");
    assert_eq!(text, "**ab** **de** c\n");
    assert_eq!(shown_of(&text), "ab de c");
    assert_eq!(typed_after("*ab*\n", "ab", " "), "*ab* \n");
}

#[test]
fn edits_find_their_place_where_the_parser_is_wrong_about_it() {
    assert_eq!(
        typed_after("[a]: b\nfoo **bar** baz\n", "bar", "x"),
        "[a]: b\nfoo **barx** baz\n"
    );
    assert_eq!(typed_after("a\rb\n\n**c** d\n", "c", "x"), "a\rb\n\n**cx** d\n");
    assert_eq!(
        typed_after("x\r\ry\r\r# H\r\rsome **bold** word\r", "bold", "x"),
        "x\r\ry\r\r# H\r\rsome **boldx** word\r"
    );
    assert_eq!(
        typed_after("> [a](b\n> \"c\") **d** e\n", "d", "x"),
        "> [a](b\n> \"c\") **dx** e\n"
    );
}

#[test]
fn deleting_a_little_removes_just_that() {
    let delete = |t: &str, what: &str| {
        let shown = shown_of(t);
        let at = shown.find(what).unwrap();
        let from = shown[..at].encode_utf16().count();
        let to = from + what.encode_utf16().count();
        edited(t, |b| b.replace(Shown::new(from)..Shown::new(to), ""))
    };
    assert_eq!(delete("😀\r\rWort zwei 😀", "Wor"), "😀\r\rt zwei 😀");
    assert_eq!(delete("[a]x(b)\n", "x"), "[a]\\(b)\n");
    assert_eq!(delete("x# a\n", "x"), "\\# a\n");
    assert_eq!(delete("**a b** c\n", "b"), "**a**  c\n");
    assert_eq!(
        delete("vor\n**fett** und\n![x](y.png)text\nnach\n", "\u{FFFC}text"),
        "vor\n**fett** und\nnach\n",
        "a line goes as a whole"
    );
}

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
fn several_lines_pasted_into_a_quote_stay_in_it() {
    let t = "> ```\n> x\n> ```\n";
    assert_eq!(
        edited(t, |b| b.replace(Shown::new(1)..Shown::new(1), "a\nb")),
        "> ```\n> xa\n> b\n> ```\n"
    );
}

#[test]
fn a_pipe_in_a_code_span_stays_inside_its_cell() {
    let t = "| a |\n|---|\n| `x` |\n";
    assert_eq!(
        edited(t, |b| b.replace(Shown::new(3)..Shown::new(3), "|")),
        "| a |\n|---|\n| `x\\|` |\n"
    );
}

#[test]
fn toggling_open_looks_at_attributes_and_not_into_their_values() {
    assert_eq!(
        toggle_open("<details title=\"a open b\" open>", true),
        Some((25..30, String::new()))
    );
    assert_eq!(toggle_open("<details title=\"a open b\">", true), None);
    assert_eq!(
        toggle_open("<details open=\"open\">", true),
        Some((8..20, String::new()))
    );
}

#[test]
fn typed_pipes_and_line_breaks_stay_inside_a_table_cell() {
    let t = "| a | b |\n|---|---|\n| x | y |\n";
    let at = 2 + 2; // Behind the header cells, each followed by a paragraph break, is the cell `x`.
    assert_eq!(
        edited(t, |b| b.replace(Shown::new(at)..Shown::new(at), "|")),
        "| a | b |\n|---|---|\n| \\|x | y |\n"
    );
    assert_eq!(
        edited(t, |b| b.replace(Shown::new(at)..Shown::new(at), "n\nm")),
        "| a | b |\n|---|---|\n| n mx | y |\n"
    );
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
fn typed_text_that_would_start_a_block_stays_text() {
    for (typed, source) in [
        ("1. ", "1\\. x\n"),
        ("12) ", "12\\) x\n"),
        ("- ", "\\- x\n"),
        ("# ", "\\# x\n"),
        ("> ", "\\> x\n"),
        ("1 ", "1 x\n"),
    ] {
        assert_eq!(
            edited("x\n", |b| b.replace(Shown::new(0)..Shown::new(0), typed)),
            source,
            "{typed:?}"
        );
    }
    assert_eq!(
        edited("x\n", |b| b.replace(Shown::new(1)..Shown::new(1), "1. ")),
        "x1. \n",
        "only at the start"
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
fn checkboxes_and_chevrons_find_their_own_place_further_down() {
    let t = "Text\n\n- [ ] a\n- [x] b\n";
    assert_eq!(
        edited(t, |b| b.toggle_task(Shown::new(5))),
        "Text\n\n- [x] a\n- [x] b\n"
    );
    assert_eq!(
        edited(t, |b| b.toggle_task(Shown::new(9))),
        "Text\n\n- [ ] a\n- [ ] b\n"
    );
    let t = "x\n\n> [!note]- Titel\n> Inhalt\n\ny\n";
    let chevron = 2 + 1 + " Titel".len();
    assert_eq!(
        edited(t, |b| b.toggle_fold(Shown::new(chevron))),
        "x\n\n> [!note]+ Titel\n> Inhalt\n\ny\n"
    );
    let t = "x\n\n> [!note]+ Titel\n> Inhalt\n\ny\n";
    assert_eq!(
        edited(t, |b| b.toggle_fold(Shown::new(chevron))),
        "x\n\n> [!note]- Titel\n> Inhalt\n\ny\n"
    );
}

#[test]
fn nothing_but_the_chevron_folds_and_nothing_but_a_checkbox_toggles() {
    let t = "x\n\n> [!note]- Titel\n> Inhalt\n\n- [ ] y\n";
    bridge_of!(t => d, l, index, b);
    assert_eq!(b.toggle_fold(Shown::new(0)), None);
    assert_eq!(b.toggle_fold(Shown::new(3)), None, "the title itself");
    assert_eq!(b.toggle_task(Shown::new(0)), None);
}

#[test]
fn the_caret_stays_behind_what_a_click_changed() {
    let t = "- [ ] a\n";
    bridge_of!(t => d, l, index, b);
    let edit = b.toggle_task(Shown::new(0)).unwrap();
    assert_eq!(edit.range, bytes(3..4));
    assert_eq!(edit.selection, bytes(4..4));
}

#[test]
fn the_open_attribute_goes_and_comes_whatever_way_it_is_written() {
    let toggled = |tag: &str, open: bool| {
        let (range, replacement) = toggle_open(tag, open).unwrap();
        format!("{}{replacement}{}", &tag[..range.start], &tag[range.end..])
    };
    assert_eq!(toggled("<details>", false), "<details open>");
    assert_eq!(toggled("<details/>", false), "<details open/>");
    assert_eq!(toggled("<details class=\"a\">", false), "<details class=\"a\" open>");
    assert_eq!(toggled("<details open>", true), "<details>");
    assert_eq!(toggled("<DETAILS OPEN>", true), "<DETAILS>");
    assert_eq!(toggled("<details open class=\"a\">", true), "<details class=\"a\">");
    assert_eq!(toggled("<details open=\"\">", true), "<details>");
    assert_eq!(toggled("<details open='open' id=x>", true), "<details id=x>");
    assert_eq!(toggled("<details open=open id=x>", true), "<details id=x>");
    assert_eq!(toggled("<details open = \"\">", true), "<details>");
    assert_eq!(
        toggled("<details class=\"open\" open>", true),
        "<details class=\"open\">"
    );
    assert_eq!(toggled("<details data-open open>", true), "<details data-open>");
    assert_eq!(toggled("<details open=\"a\" b>", true), "<details b>");
    assert_eq!(toggled("<details open='a' b>", true), "<details b>");
    assert_eq!(toggled("<details openly open>", true), "<details openly>");
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
fn typed_text_is_escaped_only_where_it_would_mean_something() {
    let typed = |t: &str, p: usize, typed: &str| edited(t, |b| b.replace(Shown::new(p)..Shown::new(p), typed));
    assert_eq!(typed("ab\n", 1, "_"), "a_b\n", "inside a word");
    assert_eq!(typed("a b\n", 1, "_"), "a\\_ b\n", "at the end of a word");
    assert_eq!(typed("a b\n", 2, "_"), "a \\_b\n", "at the start of a word");
    assert_eq!(typed("ab\n", 1, "{"), "a{b\n", "in a paragraph");
    assert_eq!(
        typed("# ab\n", 1, "{"),
        "# a\\{b\n",
        "in a heading, where it could start attributes"
    );
    assert_eq!(typed("ab\n", 1, "=="), "a\\=\\=b\n");
    assert_eq!(typed("ab\n", 1, "="), "a=b\n");
    assert_eq!(typed("a=b\n", 2, "="), "a=\\=b\n", "next to one there already");
    assert_eq!(
        typed("ab\n", 1, "\n"),
        "a\nb\n",
        "only a table cell turns it into a space"
    );
    assert_eq!(typed("ab\n", 0, "1"), "1ab\n", "digits alone start nothing");
    assert_eq!(typed(":smile\n", 6, ":"), ":smile\\:\n", "it would close a shortcode");
    assert_eq!(typed(":\n", 1, ":"), "::\n", "there is no name to close");
    assert_eq!(typed("ab\n", 0, "1"), "1ab\n", "digits without a delimiter");
    assert_eq!(typed("ab\n", 0, "12x"), "12xab\n");
    assert_eq!(typed("smile\n", 5, ":"), "smile:\n");
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
fn typing_over_formatting_goes_where_the_selection_starts() {
    // A replacement: the text stays one edit, so it is undone in one step.
    let replaced = |t: &str, from: usize, to: usize| edited(t, |b| b.replace(Shown::new(from)..Shown::new(to), "X"));
    assert_eq!(replaced("a **bc** d\n", 3, 4), "a **bX** d\n");
    assert_eq!(
        replaced("a **bc** d\n", 2, 4),
        "a X d\n",
        "all of it goes, with its markup"
    );
    assert_eq!(replaced("a **bc** d\n", 1, 3), "aX**c** d\n");
    assert_eq!(
        replaced("a **bc** d\n", 3, 6),
        "a **bX**\n",
        "over the end of the formatting"
    );
    assert_eq!(replaced("ab\n\ncd\n", 1, 4), "aXd\n");
    assert_eq!(replaced("a ![b](c.png) d\n", 2, 3), "a X d\n");
}

#[test]
fn the_caret_ends_behind_what_was_typed_over() {
    let t = "a **bc** d\n";
    bridge_of!(t => d, l, index, b);
    let edit = b.replace(Shown::new(1)..Shown::new(3), "XY").unwrap();
    let caret = edit.selection.start;
    let mut out = t.to_string();
    edit.apply_to(&mut out);
    assert_eq!(out.upto(caret), "aXY");
    let edit = b.replace(Shown::new(3)..Shown::new(3), "Z").unwrap();
    let caret = edit.selection.start;
    let mut out = t.to_string();
    edit.apply_to(&mut out);
    assert_eq!(out.upto(caret), "a **bZ");
}

/// The source range of a selection of the preview of `t`.
fn selected(t: &str, from: usize, to: usize) -> String {
    bridge_of!(t => d, l, index, b);
    t.at(b.source_selection(Shown::new(from)..Shown::new(to))).to_string()
}

#[test]
fn a_selection_of_the_preview_starts_in_its_first_character_and_counts_utf16() {
    let t = "ä😀 **fett** wort ![b](c.png) ende\n";
    // In the preview: ä, 😀 as two units, space, then `fett`.
    assert_eq!(selected(t, 0, 1), "ä");
    assert_eq!(selected(t, 1, 3), "😀");
    assert_eq!(selected(t, 4, 8), "fett");
    assert_eq!(selected(t, 6, 8), "tt");
    assert_eq!(selected(t, 9, 13), "** wort".trim_start_matches("** "));
    assert_eq!(selected(t, 14, 15), "![b](c.png)");
}

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
fn a_container_goes_with_its_content_only_when_all_of_it_is_deleted() {
    assert_eq!(
        edited("a **b** **c** d\n", |b| b.replace(Shown::new(2)..Shown::new(3), "")),
        "a  **c** d\n"
    );
    assert_eq!(
        edited("a [b](c) d\n", |b| b.replace(Shown::new(2)..Shown::new(3), "")),
        "a  d\n"
    );
    assert_eq!(
        edited("x **a** y\n\nz **b** w\n", |b| b
            .replace(Shown::new(2)..Shown::new(3), "")),
        "x  y\n\nz **b** w\n"
    );
    assert_eq!(
        edited("# **a**\n", |b| b.replace(Shown::new(0)..Shown::new(1), "x")),
        "# x\n"
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
fn typed_character_references_stay_text() {
    assert_eq!(
        edited("x\n", |b| b.replace(Shown::new(1)..Shown::new(1), "&amp;")),
        "x\\&amp;\n"
    );
    assert_eq!(
        edited("x &amp\n", |b| b.replace(Shown::new(6)..Shown::new(6), ";")),
        "x &amp\\;\n"
    );
    assert_eq!(
        edited("x\n", |b| b.replace(Shown::new(1)..Shown::new(1), "& b; &keins;")),
        "x& b; &keins;\n"
    );
}

#[test]
fn replacing_all_of_a_document_leaves_no_breaks_behind() {
    for t in [
        "Eins\n\nZwei\n\nDrei\n",
        "---\ntitle: x\n---\n\nEins\n\nZwei\n",
        "Eins\n\n---\n\nZwei\n",
        "Eins\n\n$$x$$\n",
    ] {
        let d = splat_markdown::reader::parse(t);
        let l = crate::layout::layout(&d, &crate::layout::NoImages, None);
        let shown: usize = l.blocks.iter().map(|b| b.len_u16).sum();
        let out = edited(t, |b| b.replace(Shown::new(0)..Shown::new(shown), "neu"));
        assert_eq!(out.trim_end(), "neu", "{t:?}");
    }
}

#[test]
fn a_task_checkbox_toggles_both_ways() {
    assert_eq!(edited("- [ ] a\n", |b| b.toggle_task(Shown::new(0))), "- [x] a\n");
    assert_eq!(edited("- [x] a\n", |b| b.toggle_task(Shown::new(0))), "- [ ] a\n");
}

#[test]
fn deleting_a_picture_or_formula_removes_its_whole_source() {
    assert_eq!(
        edited("a ![b](c.png) d\n", |b| b.replace(Shown::new(2)..Shown::new(3), "")),
        "a  d\n"
    );
    assert_eq!(
        edited("a $x^2$ d\n", |b| b.replace(Shown::new(2)..Shown::new(7), "")),
        "a  d\n"
    );
}

#[test]
fn deleting_all_of_a_container_takes_its_markup_along() {
    assert_eq!(
        edited("a **b** c\n", |b| b.replace(Shown::new(2)..Shown::new(3), "")),
        "a  c\n"
    );
    assert_eq!(
        edited("a ***b*** c\n", |b| b.replace(Shown::new(2)..Shown::new(3), "")),
        "a  c\n"
    );
    assert_eq!(
        edited("a `b` c\n", |b| b.replace(Shown::new(2)..Shown::new(3), "")),
        "a  c\n"
    );
    assert_eq!(
        edited("a **bc** d\n", |b| b.replace(Shown::new(2)..Shown::new(3), "")),
        "a **c** d\n"
    );
}

#[test]
fn the_chevron_of_an_indented_details_changes_its_opening_tag() {
    for (t, want) in [
        (
            "  <details>\n  <summary>Mehr</summary>\n  Inhalt\n  </details>\n",
            "  <details open>\n  <summary>Mehr</summary>\n  Inhalt\n  </details>\n",
        ),
        (
            " <details>\n<summary>Mehr</summary>\n\nInhalt\n</details>\n",
            " <details open>\n<summary>Mehr</summary>\n\nInhalt\n</details>\n",
        ),
    ] {
        bridge_of!(t => d, l, index, b);
        let mut out = t.to_string();
        b.toggle_fold(Shown::new(0)).unwrap().apply_to(&mut out);
        assert_eq!(out, want);
    }
}

#[test]
fn only_a_whole_details_tag_is_ever_toggled() {
    assert_eq!(toggle_open("<details>", false), Some((8..8, " open".to_string())));
    assert_eq!(toggle_open("<details>\n</details>", false), None);
    assert_eq!(toggle_open("<summary>", false), None);
    assert_eq!(toggle_open("nichts", false), None);
}

#[test]
fn typing_after_a_named_entity_lands_behind_it() {
    let t = "Caf&eacute; au lait\n";
    bridge_of!(t => d, l, index, b);
    let mut out = t.to_string();
    b.replace(Shown::new(5)..Shown::new(7), "X").unwrap().apply_to(&mut out);
    assert_eq!(out, "Caf&eacute; X lait\n");
}

#[test]
fn replacing_many_formatted_pieces_stays_one_edit_and_drops_their_markup() {
    let t: String = (0..3000).map(|i| format!("w{i} **b{i}** ")).collect::<String>() + "\n";
    bridge_of!(&t => d, l, index, b);
    let shown: usize = l.blocks[0].paras[0]
        .runs
        .iter()
        .map(|r| r.text.encode_utf16().count())
        .sum();
    let mut out = t.clone();
    b.replace(Shown::new(0)..Shown::new(shown), "x")
        .unwrap()
        .apply_to(&mut out);
    assert!(!out.contains("**"), "{}", &out[..out.len().min(80)]);
    assert!(out.starts_with('x'));
}
use splat_markdown::reader::parse;

#[test]
fn details_open_and_close_in_their_tag() {
    let apply = |tag: &str, open: bool| {
        let (r, with) = toggle_open(tag, open).unwrap();
        format!("{}{with}{}", &tag[..r.start], &tag[r.end..])
    };
    assert_eq!(apply("<details>", false), "<details open>");
    assert_eq!(apply("<details open>", true), "<details>");
    assert_eq!(
        apply("<details class=\"x\" open=\"open\">", true),
        "<details class=\"x\">"
    );
    assert_eq!(apply("<details OPEN data-opener=1>", true), "<details data-opener=1>");
    assert!(closes_shortcode("so :smile", ""));
    assert!(!closes_shortcode("Uhr", ""));
}
use crate::layout::{NoImages, layout};

fn edit(t: &str, f: impl Fn(&Bridge) -> Option<Edit>) -> (String, usize) {
    let d = parse(t);
    let l = layout(&d, &NoImages, None);
    let i = Index::new(&l);
    let b = Bridge::new(&d, &l, &i).unwrap();
    let e = f(&b).expect("edit");
    let mut s = t.to_string();
    e.apply_to(&mut s);
    (s, e.selection.start.get())
}

#[test]
fn typing_in_the_preview_at_pending_styles_wraps_the_word() {
    let pending = Pending {
        at: Byte::new(8),
        styles: vec![splat_markdown::commands::InlineStyle::Bold],
    };
    let typed = |p: usize, typed: &str| {
        let t = "ein Wort\n";
        let d = parse(t);
        let l = layout(&d, &NoImages, None);
        let i = Index::new(&l);
        let b = Bridge::new(&d, &l, &i).unwrap();
        b.type_with_pending(&pending, Shown::new(p), typed).map(|(e, next)| {
            let mut s = t.to_string();
            e.apply_to(&mut s);
            (s, next.map(|n| n.at))
        })
    };
    assert_eq!(typed(8, "x"), Some(("ein Wort**x**\n".into(), None)));
    assert_eq!(typed(8, " "), Some(("ein Wort \n".into(), Some(Byte::new(9)))));
    assert_eq!(typed(8, "\n"), None);
    assert_eq!(typed(3, "x"), None);
}

#[test]
fn typing_escapes_markdown_characters() {
    let (s, caret) = edit("ab\n", |b| b.replace(Shown::new(1)..Shown::new(1), "*"));
    assert_eq!(s, "a\\*b\n");
    assert_eq!(caret, 3);
    let (s, _) = edit("x\n", |b| b.replace(Shown::new(0)..Shown::new(0), "# "));
    assert_eq!(s, "\\# x\n");
    let (s, _) = edit("snake\n", |b| b.replace(Shown::new(5)..Shown::new(5), "_case"));
    assert_eq!(s, "snake_case\n");
    // Typed in the preview, none of the newer markup may take effect.
    for (typed, stored) in [
        ("==x== a = b", "\\=\\=x\\=\\= a = b"),
        ("x^2^", "x\\^2\\^"),
        ("||s|| a|b", "\\|\\|s\\|\\| a|b"),
        ("%%c%% 5%", "\\%\\%c\\%\\% 5%"),
        (":smile: 12:30", ":smile\\: 12:30"),
    ] {
        let (s, _) = edit("A\n", |b| b.replace(Shown::new(1)..Shown::new(1), typed));
        assert_eq!(s, format!("A{stored}\n"));
        let parsed = parse(&s);
        let l = layout(&parsed, &NoImages, None);
        let shown: String = l.blocks[0].paras[0].runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(shown, format!("A{typed}"), "{s}");
    }
    let (s, _) = edit("# Titel\n", |b| b.replace(Shown::new(5)..Shown::new(5), " {#x}"));
    assert_eq!(s, "# Titel \\{#x}\n");
}

#[test]
fn typing_after_a_space_at_the_paragraph_end_keeps_the_space() {
    let t = "Wort \n";
    let d = parse(t);
    let l = layout(&d, &NoImages, Some(Byte::new(5)));
    let i = Index::new(&l);
    let b = Bridge::new(&d, &l, &i).unwrap();
    let mut s = t.to_string();
    b.replace(Shown::new(5)..Shown::new(5), "z")
        .expect("edit")
        .apply_to(&mut s);
    assert_eq!(s, "Wort z\n");
}

#[test]
fn deleting_whole_bold_content_removes_delimiters() {
    let (s, _) = edit("a **b** c\n", |b| b.replace(Shown::new(2)..Shown::new(3), ""));
    assert_eq!(s, "a  c\n");
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
fn selection_maps_to_source() {
    let t = "a **fett** b\n";
    let d = parse(t);
    let l = layout(&d, &NoImages, None);
    let i = Index::new(&l);
    let b = Bridge::new(&d, &l, &i).unwrap();
    let r = b.source_selection(Shown::new(2)..Shown::new(6));
    assert_eq!(t.at(r), "fett");
}

fn edit_of(t: &str, edit: impl Fn(&Bridge) -> Option<Edit>) -> Option<Edit> {
    let d = splat_markdown::reader::parse(t);
    let l = crate::layout::layout(&d, &crate::layout::NoImages, None);
    let index = Index::new(&l);
    edit(&Bridge::new(&d, &l, &index).unwrap())
}

/// The source after `edit` of the preview of `t`, with `|` for the caret.
fn with_caret(t: &str, edit: impl Fn(&Bridge) -> Option<Edit>) -> String {
    let edit = edit_of(t, edit).expect("an edit");
    let mut out = t.to_string();
    edit.apply_to(&mut out);
    out.insert(edit.selection.start.get(), '|');
    out
}

#[test]
fn a_backtick_typed_in_a_code_span_lengthens_the_span_it_is_in() {
    let typed = |t: &str, p: usize| with_caret(t, |b| b.replace(Shown::new(p)..Shown::new(p), "`"));
    assert_eq!(typed("`ab` c", 0), "`` `|ab `` c", "at its start");
    assert_eq!(typed("xx `ab` yy", 4), "xx ``a`|b`` yy", "inside");
    assert_eq!(typed("`a` `b`", 1), "`` a`| `` `b`", "not the next one");
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

#[test]
fn a_click_on_details_leaves_the_caret_behind_the_change() {
    let t = "x\n\n<details>\n<summary>Mehr</summary>\n\nInhalt\n\n</details>\n";
    let chevron = 2;
    assert_eq!(
        with_caret(t, |b| b.toggle_fold(Shown::new(chevron))),
        "x\n\n<details open|>\n<summary>Mehr</summary>\n\nInhalt\n\n</details>\n"
    );
}

#[test]
fn a_click_before_a_chevron_folds_nothing() {
    let t = "x\n\n> [!note]- Titel\n> Inhalt\n";
    let chevron = 2 + 1 + " Titel".len();
    assert_eq!(edit_of(t, |b| b.toggle_fold(Shown::new(2))), None, "the icon");
    assert_eq!(
        edit_of(t, |b| b.toggle_fold(Shown::new(chevron - 1))),
        None,
        "the title's end"
    );
}

#[test]
#[expect(clippy::single_range_in_vec_init, reason = "lists of one deletion")]
fn an_insertion_inside_a_deletion_lands_at_its_start_and_later_ones_stay_where_they_are() {
    let composed = |deletions: &[Range<usize>], at: usize| {
        let deletions: Vec<Range<Byte>> = deletions.iter().cloned().map(bytes).collect();
        let edit = compose("abcdefgh", &deletions, Byte::new(at), "X");
        let mut out = "abcdefgh".to_string();
        edit.apply_to(&mut out);
        out.insert(edit.selection.start.get(), '|');
        out
    };
    assert_eq!(composed(&[1..3, 5..6], 2), "aX|degh", "in the first of two");
    assert_eq!(composed(&[1..2], 4), "acdX|efgh", "behind it");
    assert_eq!(composed(&[3..5], 1), "aX|bcfgh", "before it");
}

#[test]
fn typed_punctuation_alone_at_the_start_of_a_paragraph_needs_no_backslash() {
    assert_eq!(edited("x\n", |b| b.replace(Shown::new(0)..Shown::new(0), ".")), ".x\n");
    assert_eq!(edited("x\n", |b| b.replace(Shown::new(0)..Shown::new(0), ")")), ")x\n");
}

#[test]
fn a_quoted_open_value_goes_with_its_spaces() {
    let (range, replacement) = toggle_open("<details open=\"a b\" c>", true).unwrap();
    assert_eq!((range, replacement.as_str()), (8..19, ""));
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
