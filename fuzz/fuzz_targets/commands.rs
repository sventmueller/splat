//! `splat_testkit::fuzz_checks::commands`.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| splat_testkit::fuzz_checks::commands(data));
