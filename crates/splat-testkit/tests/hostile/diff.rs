//! Texts built to hurt the comparison of two versions.

use std::time::Duration;

use splat_git::diff::changes;
use splat_testkit::timing::{NOTICEABLE, alone, grows_too_fast, holds_to_time};

/// Lines of about fifty bytes, each different.
fn numbered_lines(word: &str, count: usize) -> Vec<String> {
    (0..count)
        .map(|line| format!("{word} {line:>8} of a text that goes on for a good while\n"))
        .collect()
}

/// The same order on every run.
fn shuffled<T>(mut items: Vec<T>) -> Vec<T> {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    for at in (1..items.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        items.swap(at, usize::try_from(state % (at as u64 + 1)).unwrap());
    }
    items
}

/// Two texts of `count` lines that make a comparison work.
fn texts_that_differ(count: usize) -> Vec<(&'static str, String, String)> {
    let lines = numbered_lines("line", count);
    let every_other: String = lines
        .iter()
        .enumerate()
        .map(|(at, line)| {
            if at % 2 == 0 {
                line.clone()
            } else {
                format!("{at}, changed\n")
            }
        })
        .collect();
    let two_kinds = |seed: usize| -> String {
        let kinds = [
            "one of two lines, as long as the other one\n",
            "two of two lines, as long as the other one\n",
        ];
        (0..count)
            .map(|at| kinds[usize::from((at * 2654435761 + seed).count_ones().is_multiple_of(2))])
            .collect()
    };
    vec![
        (
            "nothing in common",
            lines.concat(),
            numbered_lines("other", count).concat(),
        ),
        (
            "the same lines the other way round",
            lines.concat(),
            lines.iter().rev().cloned().collect(),
        ),
        (
            "the same lines shuffled",
            lines.concat(),
            shuffled(lines.clone()).concat(),
        ),
        ("every other line changed", lines.concat(), every_other),
        ("two kinds of line at random", two_kinds(1), two_kinds(7)),
    ]
}

#[test]
fn comparing_texts_built_to_hurt_takes_linear_time() {
    let cases = texts_that_differ(50_000).into_iter().zip(texts_that_differ(200_000));
    for ((name, small_old, small_new), (_, large_old, large_new)) in cases {
        let alone = alone();
        let small = alone.fastest(3, || drop(std::hint::black_box(changes(&small_old, &small_new))));
        let large = alone.fastest(3, || drop(std::hint::black_box(changes(&large_old, &large_new))));
        drop(alone);
        if holds_to_time() {
            let too_fast = grows_too_fast(NOTICEABLE, small, large, small_old.len(), large_old.len());
            assert!(!too_fast, "{name}: {small:?}, then {large:?}");
        }
    }
}

/// About 150 ms is the aim; a limit that close would only measure what else the Mac is doing.
#[test]
fn ten_megabytes_that_differ_everywhere_stay_near_150_milliseconds() {
    for (name, old, new) in texts_that_differ(200_000) {
        let took = alone().fastest(3, || drop(std::hint::black_box(changes(&old, &new))));
        eprintln!("{name}: {} bytes, {took:?}", old.len());
        if holds_to_time() {
            assert!(took < Duration::from_millis(600), "{name}: {took:?}");
        }
    }
}
