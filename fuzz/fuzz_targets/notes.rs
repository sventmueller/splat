//! `splat_testkit::fuzz_notes::check_notes`.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| splat_testkit::fuzz_notes::check_notes(data));
