//! `splat_testkit::fuzz_git::check_git`.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| splat_testkit::fuzz_git::check_git(data));
