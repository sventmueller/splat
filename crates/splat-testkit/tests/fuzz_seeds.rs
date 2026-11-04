//! The seeds in `fuzz/seeds` through the checks of the fuzz targets on every test run, without a fuzzer: what one found once,
//! and the inputs that reach deepest, stay in the way of whoever breaks them again.

use std::path::{Path, PathBuf};

use splat_markdown::commands::Edit;
use splat_markdown::text::bytes;
use splat_testkit::fuzz_checks::{self, edit_fits, replay, xml_problem};

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
