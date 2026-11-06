//! Documents built to hurt, each small enough to arrive as an attachment: nesting that overflows a stack and patterns
//! that take quadratic time or memory. Every consumer of the parsed document, the keystrokes and the commands run on a
//! stack as small as the app's smallest. Each document runs at two sizes, whose time and memory must grow about as the
//! text does. Every document has a thread named after it, so a stack overflow, which ends the test run, names its document.

mod common;
mod diff;
mod documents;
mod git;
mod notes;

#[global_allocator]
static COUNTING: splat_testkit::memory::Counting = splat_testkit::memory::Counting;

#[test]
fn memory_is_counted() {
    assert!(splat_testkit::memory::counts());
}
