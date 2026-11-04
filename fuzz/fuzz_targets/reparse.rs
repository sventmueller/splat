//! `splat_testkit::fuzz_checks::reparse`.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| splat_testkit::fuzz_checks::reparse(data));
