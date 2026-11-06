//! Which text a text search finds: case, folding and normal forms.

use super::*;

#[test]
fn text_is_found_with_its_byte_ranges_in_the_file() {
    let text = "Eins zwei\nGrüße zwei\r\nnoch\rzwei";
    let query = Query::Text {
        needle: "ZWEI".into(),
        case_sensitive: false,
    };
    let hits = hits_in_text(text, &query, Path::new("/n.md"), &tree());
    let lines: Vec<usize> = hits.hits.iter().map(|hit| hit.line).collect();
    assert_eq!(lines, [0, 1, 3]);
    for hit in &hits.hits {
        assert_eq!(&text[hit.range.clone()], "zwei");
        assert_eq!(&hit.context[hit.in_context.clone()], "zwei");
    }
    assert_eq!(hits.hits[1].context, "Grüße zwei");
    assert_eq!(hits.hits[2].context, "zwei");
    assert!(!hits.more);
    assert_eq!(hits.path, Path::new("/n.md"));
}

#[test]
fn case_sensitivity_is_asked_for() {
    assert_eq!(matches("Aa aA", "aa", false).len(), 2);
    assert_eq!(matches("Aa aA", "Aa", true), ["Aa"]);
    assert_eq!(matches("Müller MÜLLER", "müller", true), Vec::<String>::new());
    assert_eq!(matches("Müller MÜLLER müller", "müller", true), ["müller"]);
    assert_eq!(folded("Müller MÜLLER", "müller"), ["Müller", "MÜLLER"]);
}

#[test]
fn sharp_s_and_double_s_find_each_other() {
    assert_eq!(folded("Straße", "STRASSE"), ["Straße"]);
    assert_eq!(folded("STRASSE strasse", "straße"), ["STRASSE", "strasse"]);
    assert_eq!(folded("ẞ", "ss"), ["ẞ"], "the capital sharp s");
    assert_eq!(folded("ß", "s"), Vec::<String>::new(), "half of a character");
    assert_eq!(folded("ßs", "ss"), ["ß"]);
    assert_eq!(folded("sß", "ss"), ["ß"], "the first s only looks like a start");
    assert_eq!(folded("ßß", "ss"), ["ß", "ß"]);
    assert_eq!(folded("ß", "sss"), Vec::<String>::new());
    assert_eq!(
        matches("Straße STRASSE", "straße", true),
        Vec::<String>::new(),
        "no folding when case counts"
    );
    assert_eq!(matches("Straße", "Straße", true), ["Straße"]);
}

#[test]
fn the_turkish_letters_follow_the_default_rules() {
    assert_eq!(folded("I i", "i"), ["I", "i"]);
    assert_eq!(
        folded("ı", "i"),
        Vec::<String>::new(),
        "the dotless i is a letter of its own"
    );
    assert_eq!(folded("ı", "I"), Vec::<String>::new());
    assert_eq!(folded("ı I i", "ı"), ["ı"]);
    assert_eq!(
        folded("İ", "i"),
        Vec::<String>::new(),
        "the capital with a dot folds to two characters"
    );
    assert_eq!(folded("İ", "i\u{307}"), ["İ"]);
    assert_eq!(folded("i\u{307}", "İ"), ["i\u{307}"]);
}

#[test]
fn composed_and_decomposed_forms_find_each_other() {
    for case_sensitive in [false, true] {
        assert_eq!(matches("Müller", "Mu\u{308}ller", case_sensitive), ["Müller"]);
        assert_eq!(matches("Mu\u{308}ller", "Müller", case_sensitive), ["Mu\u{308}ller"]);
        assert_eq!(matches("ä a\u{308}", "ä", case_sensitive), ["ä", "a\u{308}"]);
        assert_eq!(matches("한글", "한", case_sensitive), ["한"]);
        assert_eq!(
            matches("\u{1112}\u{1161}\u{11AB}글", "한", case_sensitive),
            ["\u{1112}\u{1161}\u{11AB}"]
        );
        assert_eq!(matches("한글", "\u{1112}\u{1161}\u{11AB}", case_sensitive), ["한"]);
    }
}

#[test]
fn a_match_does_not_end_in_front_of_what_combines_with_its_last_letter() {
    for text in ["ä", "a\u{308}"] {
        assert_eq!(folded(text, "a"), Vec::<String>::new(), "{text:?}");
        assert_eq!(folded(text, "\u{308}"), Vec::<String>::new(), "{text:?}");
    }
    assert_eq!(folded("한", "\u{1112}\u{1161}"), Vec::<String>::new());
    assert_eq!(
        folded("\u{1112}\u{1161}\u{11AB}", "\u{1112}\u{1161}"),
        Vec::<String>::new()
    );
    assert_eq!(folded("äb", "b"), ["b"]);
    assert_eq!(folded("aäa", "a"), ["a", "a"]);
}

#[test]
fn marks_in_another_order_are_the_same_text() {
    let (dot_below_then_above, dot_above_then_below) = ("a\u{323}\u{307}", "a\u{307}\u{323}");
    assert_eq!(
        folded(dot_below_then_above, dot_above_then_below),
        [dot_below_then_above]
    );
    assert_eq!(
        folded(dot_above_then_below, dot_below_then_above),
        [dot_above_then_below]
    );
    assert_eq!(
        folded("ȧ\u{323}", "ạ\u{307}"),
        ["ȧ\u{323}"],
        "composed in different ways"
    );
    assert_eq!(folded("ȧ\u{323}x", "ȧ\u{323}x"), ["ȧ\u{323}x"]);
    assert_eq!(folded("ΑΙ", "αι"), ["ΑΙ"]);
    assert_eq!(folded("ᾳ", "α\u{345}"), ["ᾳ"], "a mark that also has a case fold");
}

#[test]
fn other_letters_with_a_special_fold_find_each_other() {
    assert_eq!(folded("K", "k"), ["K"], "a Kelvin sign");
    assert_eq!(folded("Å", "å"), ["Å"], "an Angstrom sign");
    assert_eq!(folded("σς", "ΣΣ"), ["σς"]);
    assert_eq!(folded("ﬁ", "FI"), ["ﬁ"], "a ligature");
    assert_eq!(folded("Страна", "СТРАНА"), ["Страна"]);
    assert_eq!(folded("日本語", "本"), ["本"]);
    assert_eq!(folded("😀a", "😀A"), ["😀a"]);
}

#[test]
fn hits_do_not_overlap_but_a_failed_start_does_not_end_the_search() {
    assert_eq!(folded("aaaa", "aa").len(), 2);
    assert_eq!(folded("abababa", "aba").len(), 2);
    assert_eq!(folded("aaab", "aab"), ["aab"]);
    assert_eq!(folded("abcabd", "abd"), ["abd"]);
    assert_eq!(folded("xaay aaaz", "aaz"), ["aaz"]);
    assert_eq!(folded("aä", "a"), ["a"]);
}

#[test]
fn nothing_is_found_for_nothing() {
    assert_eq!(folded("abc", ""), Vec::<String>::new());
    assert_eq!(folded("", "a"), Vec::<String>::new());
}

#[test]
fn a_match_may_stand_at_the_very_end_and_go_over_lines() {
    assert_eq!(folded("xab", "ab"), ["ab"]);
    assert_eq!(folded("xä", "ä"), ["ä"]);
    assert_eq!(folded("xa\u{308}", "ä"), ["a\u{308}"]);
    assert_eq!(folded("a\nb", "a\nb"), ["a\nb"]);
    let query = Query::Text {
        needle: "b\nc".into(),
        case_sensitive: true,
    };
    let hit = &hits_in_text("a\nb\nc\n", &query, Path::new("/n.md"), &tree()).hits[0];
    assert_eq!((hit.line, hit.context.as_str(), hit.in_context.clone()), (1, "b", 0..1));
}

#[test]
fn a_byte_order_mark_is_not_part_of_the_first_line() {
    let text = "\u{FEFF}Eins\nzwei";
    let query = Query::Text {
        needle: "eins".into(),
        case_sensitive: false,
    };
    let hit = &hits_in_text(text, &query, Path::new("/n.md"), &tree()).hits[0];
    assert_eq!((hit.range.clone(), hit.line, hit.context.as_str()), (3..7, 0, "Eins"));
}

#[test]
fn a_needle_starting_with_something_else_than_ascii_runs_ahead_as_well() {
    assert_eq!(folded(&format!("{}日本", "x".repeat(1000)), "日本"), ["日本"]);
    assert_eq!(folded("xxxx", "日"), Vec::<String>::new());
    assert_eq!(
        Pattern::new("ä", true).first_ascii(),
        Some(b'a'),
        "by its decomposition"
    );
    assert_eq!(Pattern::new("日", true).first_ascii(), None);
    assert_eq!(Pattern::new("", true).first_ascii(), None);
}

#[test]
fn a_long_run_of_marks_is_put_in_order() {
    let marks = "\u{301}\u{323}".repeat(5000);
    let text = format!("a{marks}b");
    assert_eq!(folded(&text, &text).len(), 1);
}
