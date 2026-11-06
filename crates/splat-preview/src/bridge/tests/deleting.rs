//! Deleting, joining and typing over what is selected in the preview.

use super::*;

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

#[test]
fn deleting_whole_bold_content_removes_delimiters() {
    let (s, _) = edit("a **b** c\n", |b| b.replace(Shown::new(2)..Shown::new(3), ""));
    assert_eq!(s, "a  c\n");
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
