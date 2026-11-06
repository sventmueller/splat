//! What a keystroke costs in a large document against a full update.

use splat_markdown::highlight::highlight;
use splat_markdown::incremental::reparse_and_color;
use splat_markdown::reader::parse;
use splat_preview::layout::{NoImages, layout, relayout, reusable};

use super::common::limited;

fn quick_reference_without_footnotes() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../resources/de.lproj/QuickReference.md"
    ))
    .unwrap()
    .replace("[^", "(")
    .replace("^[", "^(")
    .replace("]:", "] :")
}

/// The partial update of a 10 MB document after one keystroke, against a full one: same result, in a fraction of the time.
/// Run with `cargo test --release -- --ignored partial_updates`.
#[test]
#[ignore]
fn partial_updates_at_ten_megabytes() {
    let one = quick_reference_without_footnotes();
    let mut old = String::new();
    while old.len() < 10 << 20 {
        old.push_str(&one);
        old.push_str("\n\n");
    }
    keystroke_is_partial(&old, "Fett", "one keystroke at 10 MB", 8.0);
}

/// The quick reference with its footnotes numbered apart, so that every copy keeps its own, and without its inline note:
/// a thousand of those in one text switch them off, which only all of the text can tell.
#[test]
#[ignore]
fn partial_updates_of_the_quick_reference_as_it_ships() {
    let one = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../resources/de.lproj/QuickReference.md"
    ))
    .expect("the quick reference");
    let mut old = String::new();
    let mut copy = 0;
    while old.len() < 10 << 20 {
        old.push_str(&one.replace("[^", &format!("[^{copy}-")).replace("^[", "^("));
        old.push_str("\n\n");
        copy += 1;
    }
    keystroke_is_partial(
        &old,
        "## Listen",
        "one keystroke in the quick reference as it ships at 10 MB",
        8.0,
    );
}

/// The same with footnotes, half of them defined next to their references and half behind the text.
#[test]
#[ignore]
fn partial_updates_around_footnotes_at_ten_megabytes() {
    let one = quick_reference_without_footnotes();
    let (mut old, mut behind) = (String::new(), String::new());
    let mut note = 0;
    while old.len() + behind.len() < 10 << 20 {
        old.push_str(&one);
        old.push_str(&format!("\n\nNoted.[^n{note}]\n\n"));
        let definition = format!("[^n{note}]: A note.\n\n");
        if note % 2 == 0 {
            old.push_str(&definition);
        } else {
            behind.push_str(&definition);
        }
        note += 1;
    }
    old.push_str(&behind);
    keystroke_is_partial(&old, "Fett", "one keystroke around footnotes at 10 MB", 8.0);
}

/// The budget of a keystroke against what parsing all of the text takes, so that it does not depend on the machine.
#[test]
fn a_keystroke_in_a_megabyte_stays_far_below_a_full_update() {
    let one = quick_reference_without_footnotes();
    let mut old = String::new();
    while old.len() < 1 << 20 {
        old.push_str(&one);
        old.push_str("\n\n");
    }
    let _limit = limited();
    keystroke_is_partial(&old, "Fett", "one keystroke at 1 MB", 3.0);
}

/// `times_faster` than a full update, on the shortest of some runs.
fn keystroke_is_partial(old: &str, word: &str, what: &str, times_faster: f64) {
    let middle = old.floor_char_boundary(old.len() / 2);
    let at = old[middle..].find(word).unwrap() + middle + word.len();
    let mut new = old.to_string();
    new.insert(at, 'x');
    // Other tests run at the same time: the fastest of some runs says what the code costs, a single one what they do.
    let (mut full_ms, mut partial_ms) = (f64::MAX, f64::MAX);
    for _ in 0..3 {
        let doc = parse(old);
        let spans = highlight(&doc);
        let old_layout = layout(&doc, &NoImages, None);
        // The app works these out when it loads a text, not at the keystroke.
        doc.facts();
        let started = std::time::Instant::now();
        let full = parse(&new);
        let full_spans = highlight(&full);
        let full_layout = layout(&full, &NoImages, None);
        full_ms = full_ms.min(started.elapsed().as_secs_f64() * 1e3);
        let started = std::time::Instant::now();
        let parsed = reparse_and_color(doc, &spans, &new);
        let splice = parsed.splice.as_ref().expect("a partial parse");
        let new_layout = relayout(
            &parsed.doc,
            &NoImages,
            None,
            old_layout,
            &reusable(splice, &parsed.doc, None),
        );
        partial_ms = partial_ms.min(started.elapsed().as_secs_f64() * 1e3);
        assert_eq!(parsed.doc.blocks(), full.blocks());
        assert_eq!(parsed.spans, full_spans);
        assert_eq!(new_layout.blocks, full_layout.blocks);
    }
    eprintln!("{what}: full {full_ms:.0} ms, partial {partial_ms:.0} ms");
    assert!(
        partial_ms * times_faster < full_ms,
        "a keystroke got slower: partial {partial_ms:.0} ms, full {full_ms:.0} ms"
    );
}
