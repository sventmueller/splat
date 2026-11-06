//! Where a hit stands, the line around it, and how many a note reports.

use super::*;

#[test]
fn a_long_line_is_shortened_around_the_match_on_letters() {
    let line = format!("{}FUND{}", "ä".repeat(500), "ö".repeat(500));
    let query = Query::Text {
        needle: "fund".into(),
        case_sensitive: false,
    };
    let hit = &hits_in_text(&line, &query, Path::new("/n.md"), &tree()).hits[0];
    assert_eq!(hit.context.chars().count(), MOST_CONTEXT_CHARS);
    assert_eq!(&hit.context[hit.in_context.clone()], "FUND");
    let before = hit.context[..hit.in_context.start].chars().count();
    assert!(before.abs_diff(MOST_CONTEXT_CHARS / 2) <= 4, "{before}");
    assert_eq!(hit.range.start, 1000);
    for (text, what) in [
        (format!("FUND{}", "x".repeat(500)), "at the start"),
        (format!("{}FUND", "x".repeat(500)), "at the end"),
        (format!("x\nFUND{}\ny", "x".repeat(500)), "in a line of its own"),
    ] {
        let hit = &hits_in_text(&text, &query, Path::new("/n.md"), &tree()).hits[0];
        assert_eq!(hit.context.chars().count(), MOST_CONTEXT_CHARS, "{what}");
        assert_eq!(&hit.context[hit.in_context.clone()], "FUND", "{what}");
    }
}

#[test]
fn a_match_longer_than_the_context_fills_it() {
    let needle = "ü".repeat(MOST_CONTEXT_CHARS * 2);
    let text = format!("a{needle}b");
    let query = Query::Text {
        needle,
        case_sensitive: true,
    };
    let hit = &hits_in_text(&text, &query, Path::new("/n.md"), &tree()).hits[0];
    assert_eq!(hit.context.chars().count(), MOST_CONTEXT_CHARS);
    assert_eq!(hit.in_context, 0..hit.context.len());
    assert_eq!(hit.range.len(), MOST_CONTEXT_CHARS * 4);
}

#[test]
fn more_hits_than_a_file_may_show_are_only_said_to_be_there() {
    let query = Query::Text {
        needle: "x".into(),
        case_sensitive: true,
    };
    let at_limit = hits_in_text(&"x ".repeat(MOST_HITS_PER_FILE), &query, Path::new("/n.md"), &tree());
    assert_eq!((at_limit.hits.len(), at_limit.more), (MOST_HITS_PER_FILE, false));
    let over = hits_in_text(
        &"x ".repeat(MOST_HITS_PER_FILE + 1),
        &query,
        Path::new("/n.md"),
        &tree(),
    );
    assert_eq!((over.hits.len(), over.more), (MOST_HITS_PER_FILE, true));
    let at_the_end = format!("{}x", "x ".repeat(MOST_HITS_PER_FILE));
    assert!(
        hits_in_text(&at_the_end, &query, Path::new("/n.md"), &tree()).more,
        "the last one at the very end"
    );
    let lines = "#a #a\n".repeat(MOST_HITS_PER_FILE);
    let tags = hits_in_text(&lines, &Query::Tag("a".into()), Path::new("/n.md"), &tree());
    assert_eq!((tags.hits.len(), tags.more), (MOST_HITS_PER_FILE, true));
}

#[test]
fn many_hits_in_one_line_take_linear_time() {
    let query = Query::Text {
        needle: "x".into(),
        case_sensitive: false,
    };
    let line = "x".repeat(5_000_000);
    let started = std::time::Instant::now();
    let found = hits_in_text(&line, &query, Path::new("/n.md"), &tree());
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert_eq!((found.hits.len(), found.more), (MOST_HITS_PER_FILE, true));
}

#[test]
fn a_thousand_hits_in_a_line_each_look_at_no_more_than_their_context() {
    let line = format!("{} ", "x ".repeat(1000)).repeat(2000);
    let places = |text: &str| {
        let mut places = Places::new(text);
        let started = std::time::Instant::now();
        for at in (0..2000).map(|n| n * text.len() / 2000) {
            let at = text.floor_char_boundary(at);
            places.hits.clear();
            places.add(at..at + 1);
        }
        started.elapsed()
    };
    let (small, large) = (places(&line[..line.len() / 4]), places(&line));
    assert!(
        large < small * 6 + std::time::Duration::from_millis(50),
        "{small:?}, then {large:?}"
    );
}

#[test]
fn a_needle_full_of_repeats_does_not_go_quadratic() {
    let needle = format!("{}b", "a".repeat(5000));
    let found = |len: usize| {
        let text = "a".repeat(len);
        let started = std::time::Instant::now();
        assert_eq!(folded(&text, &needle), Vec::<String>::new());
        started.elapsed()
    };
    let (small, large) = (found(1 << 20), found(1 << 22));
    assert!(
        large < small * 8 + std::time::Duration::from_millis(100),
        "{small:?}, then {large:?}"
    );
}

#[test]
fn a_context_never_leaves_its_line_or_letters() {
    for (text, range, line_start) in [
        ("aä\nb", 3..4, 0),
        ("ä", 0..2, 0),
        ("a\r\nb", 3..4, 3),
        ("xy", 2..2, 0),
        ("a\r\nb", 2..3, 0),
    ] {
        let (context, inside) = context_of(text, &range, line_start);
        assert!(
            text.contains(&context) && !context.contains(['\n', '\r']),
            "{text:?}: {context:?}"
        );
        assert!(inside.end <= context.len(), "{text:?}");
    }
}
