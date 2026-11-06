//! `splat_testkit::fuzz_import::check_import`.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| splat_testkit::fuzz_import::check_import(data));
