//! The seeds in `fuzz/seeds` through the checks of the fuzz targets on every test run, without a fuzzer: what one found once,
//! and the inputs that reach deepest, stay in the way of whoever breaks them again.

use std::ops::Range;
use std::path::{Path, PathBuf};

use splat_git::diff::{Change, ChangeKind, Hunk};
use splat_markdown::commands::Edit;
use splat_markdown::text::bytes;
use splat_testkit::fuzz_checks::{self, edit_fits, replay, xml_problem};
use splat_testkit::fuzz_git::{check_git, hunks_hold};
use splat_testkit::fuzz_notes;

fn seeds(target: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/seeds")
        .join(target)
}

#[test]
fn the_seeds_of_the_partial_parse_read_as_a_full_parse() {
    assert!(replay(&seeds("reparse"), fuzz_checks::reparse) > 50);
}

#[test]
fn the_seeds_of_the_outputs_make_clean_outputs() {
    assert!(replay(&seeds("outputs"), fuzz_checks::outputs) > 50);
}

#[test]
fn the_seeds_of_the_preview_make_edits_that_fit() {
    assert!(replay(&seeds("bridge"), fuzz_checks::bridge) > 50);
}

#[test]
fn the_seeds_of_the_commands_make_edits_that_fit() {
    assert!(replay(&seeds("commands"), fuzz_checks::commands) > 50);
}

#[test]
fn the_seeds_of_the_file_formats_save_as_they_opened() {
    assert!(replay(&seeds("encoding"), fuzz_checks::encoding) > 50);
}

#[test]
fn the_seeds_of_git_compare_and_parse_within_their_limits() {
    assert!(replay(&seeds("git"), check_git) > 50);
}

fn one_hunk(old_lines: Range<usize>, kind: ChangeKind, lines: Range<usize>, bytes: Range<usize>) -> Vec<Hunk> {
    vec![Hunk {
        old_lines,
        change: Change { kind, lines, bytes },
    }]
}

#[test]
fn true_hunks_hold() {
    let found = one_hunk(1..2, ChangeKind::Modified, 1..2, 2..4);
    hunks_hold("a\nb\nc\n", "a\nx\nc\n", &found, "a test");
    hunks_hold("a\nc\n", "a\nc\n", &[], "a test");
    let removed = one_hunk(1..2, ChangeKind::Removed, 1..1, 2..2);
    hunks_hold("a\nb\nc\n", "a\nc\n", &removed, "a test");
    let added = one_hunk(1..1, ChangeKind::Added, 1..2, 2..4);
    hunks_hold("a\nc\n", "a\nb\nc\n", &added, "a test");
}

#[test]
#[should_panic(expected = "a test")]
fn a_line_reported_as_unchanged_that_changed_is_found() {
    hunks_hold("a\nb\n", "a\nx\n", &[], "a test");
}

#[test]
#[should_panic(expected = "a test")]
fn a_change_of_the_wrong_kind_is_found() {
    hunks_hold("a\n", "b\n", &one_hunk(0..1, ChangeKind::Added, 0..1, 0..2), "a test");
}

#[test]
#[should_panic(expected = "a test")]
fn bytes_that_are_not_those_of_the_lines_are_found() {
    hunks_hold(
        "a\n",
        "b\n",
        &one_hunk(0..1, ChangeKind::Modified, 0..1, 0..1),
        "a test",
    );
}

#[test]
#[should_panic(expected = "a test")]
fn a_change_of_no_lines_is_found() {
    hunks_hold("a\n", "a\n", &one_hunk(0..0, ChangeKind::Added, 0..0, 0..0), "a test");
}

#[test]
#[should_panic(expected = "a test")]
fn two_changes_that_touch_are_found() {
    let mut found = one_hunk(0..1, ChangeKind::Modified, 0..1, 0..2);
    found.extend(one_hunk(1..2, ChangeKind::Modified, 1..2, 2..4));
    hunks_hold("a\nb\n", "x\ny\n", &found, "a test");
}

#[test]
#[should_panic(expected = "a test")]
fn a_different_number_of_lines_behind_the_last_change_is_found() {
    hunks_hold("a\nb\n", "a\n", &[], "a test");
}

#[test]
fn inputs_too_short_for_two_parts_and_of_any_length_are_checked() {
    check_git(b"");
    check_git(b"x");
    check_git(b"\xff\xffno second part");
    check_git(b"\0\x02a\nb\n");
}

#[test]
fn the_seeds_of_the_importers_read_without_harm() {
    assert!(replay(&seeds("import"), splat_testkit::fuzz_import::check_import) > 30);
}

#[test]
fn the_seeds_of_the_notes_keep_what_counts_metadata_search_and_renames_promise() {
    assert!(replay(&seeds("notes"), fuzz_notes::check_notes) > 50);
}

#[test]
fn what_is_no_xml_is_found() {
    assert_eq!(xml_problem(b"<a><b/></a>"), None);
    assert!(xml_problem(b"<a>").is_some());
    assert!(xml_problem(b"\xFF").is_some());
}

#[test]
#[should_panic(expected = "a test")]
fn an_edit_beyond_the_text_is_found() {
    let edit = Edit {
        range: bytes(0..9),
        replacement: String::new(),
        selection: bytes(0..0),
    };
    edit_fits("ab", &edit, "a test");
}

#[test]
#[should_panic(expected = "a test")]
fn an_edit_inside_a_letter_is_found() {
    let edit = Edit {
        range: bytes(1..1),
        replacement: String::new(),
        selection: bytes(0..0),
    };
    edit_fits("ä", &edit, "a test");
}

#[test]
fn every_format_that_opens_saves_as_it_opened() {
    for file in [
        &b"a\nb\n"[..],
        b"a\r\nb\r\n",
        b"a\rb\r",
        b"a\r\nb\n",
        b"\xEF\xBB\xBFa\r\n",
        b"\xFF\xFEa\0\r\0\n\0",
        b"\xFE\xFF\0a\0\n",
        b"\xFF",
    ] {
        fuzz_checks::encoding(file);
    }
}
