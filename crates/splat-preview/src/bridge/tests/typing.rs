//! What is typed in the preview: where it lands, and what is escaped so that it stays text.

use super::*;

#[test]
fn a_space_and_a_letter_at_the_end_of_a_windows_line_stay_in_order() {
    assert_eq!(typed_after("Wort\r\n", "Wort", " z"), "Wort z\r\n");
    assert_eq!(typed_after("Wort\r\n\r\nB\r\n", "Wort", " z"), "Wort z\r\n\r\nB\r\n");
    assert_eq!(typed_after("Wort\r", "Wort", " z"), "Wort z\r");
    assert_eq!(typed_after("Wort\n", "Wort", " z"), "Wort z\n");
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
fn typing_after_a_named_entity_lands_behind_it() {
    let t = "Caf&eacute; au lait\n";
    bridge_of!(t => d, l, index, b);
    let mut out = t.to_string();
    b.replace(Shown::new(5)..Shown::new(7), "X").unwrap().apply_to(&mut out);
    assert_eq!(out, "Caf&eacute; X lait\n");
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
fn typed_punctuation_alone_at_the_start_of_a_paragraph_needs_no_backslash() {
    assert_eq!(edited("x\n", |b| b.replace(Shown::new(0)..Shown::new(0), ".")), ".x\n");
    assert_eq!(edited("x\n", |b| b.replace(Shown::new(0)..Shown::new(0), ")")), ")x\n");
}
