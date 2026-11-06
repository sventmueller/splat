//! Random inputs for the randomised tests. `SPLAT_FUZZ_ROUNDS` raises the rounds for a longer search,
//! `SPLAT_FUZZ_SEED` varies the inputs.

use std::cell::Cell;
use std::ops::Range;
use std::sync::{Once, mpsc};
use std::time::Duration;

use splat_markdown::text::Byte;

pub mod fuzz_checks;
pub mod fuzz_git;
pub mod fuzz_import;
pub mod fuzz_notes;

/// Rounds per test.
pub fn rounds(default: usize) -> usize {
    match std::env::var("SPLAT_FUZZ_ROUNDS") {
        Ok(v) => v
            .parse()
            .unwrap_or_else(|_| panic!("SPLAT_FUZZ_ROUNDS={v:?} is no number")),
        Err(_) => default,
    }
}

thread_local! {
    static QUIET: Cell<bool> = const { Cell::new(false) };
}

/// The panic hook belongs to the process and tests run side by side, so only this thread's panics may go unreported.
fn run_quietly<T>(f: impl FnOnce() -> T) -> T {
    static WRAPPED: Once = Once::new();
    WRAPPED.call_once(|| {
        let report = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !QUIET.get() {
                report(info);
            }
        }));
    });
    struct Loud;
    impl Drop for Loud {
        fn drop(&mut self) {
            QUIET.set(false);
        }
    }
    QUIET.set(true);
    let _loud = Loud;
    f()
}

/// Runs `test` on `text` and, if it panics, on ever smaller pieces of it, to fail with the smallest text that still
/// does: a random document of forty pieces says little about what went wrong.
pub fn with_smallest_failure(text: &str, test: impl Fn(&str) + std::panic::RefUnwindSafe) {
    let fails = |candidate: &str| run_quietly(|| std::panic::catch_unwind(|| test(candidate)).is_err());
    if !fails(text) {
        return;
    }
    let mut smallest = text.to_string();
    let mut piece = smallest.len().div_ceil(2).max(1);
    while piece > 0 {
        let mut at = 0;
        while at < smallest.len() {
            let from = smallest.floor_char_boundary(at);
            let to = smallest.ceil_char_boundary((at + piece).min(smallest.len()));
            let candidate = format!("{}{}", &smallest[..from], &smallest[to..]);
            if from < to && fails(&candidate) {
                smallest = candidate;
            } else {
                at = to.max(at + 1);
            }
        }
        piece /= 2;
    }
    eprintln!("smallest failing document: {smallest:?}");
    test(&smallest);
    panic!("it failed on {smallest:?}, reduced from {text:?}, but not when run once more");
}

/// While it lives, a panic of this thread also reports `input`.
pub struct Reporting<'a>(pub &'a str);

impl Drop for Reporting<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("failing input: {:?}", self.0);
        }
    }
}

/// While it lives, the test run ends if the test takes longer than `limit`; a hang would otherwise go on until someone
/// notices.
pub struct TimeLimit(#[expect(dead_code, reason = "dropping it tells the watcher the test ended")] mpsc::Sender<()>);

pub fn time_limit(limit: Duration) -> TimeLimit {
    let name = std::thread::current().name().unwrap_or("a test").to_string();
    let (ended, watch) = mpsc::channel();
    std::thread::spawn(move || {
        if watch.recv_timeout(limit) == Err(mpsc::RecvTimeoutError::Timeout) {
            eprintln!("{name} did not finish within {limit:?}");
            std::process::exit(101);
        }
    });
    TimeLimit(ended)
}

/// Xorshift: the same seed gives the same inputs, so a failure found once can be run again.
pub struct Rng(u64);

impl Rng {
    /// Each test passes a seed of its own; `SPLAT_FUZZ_SEED` is mixed into it.
    pub fn new(seed: u64) -> Self {
        let varied = match std::env::var("SPLAT_FUZZ_SEED") {
            Ok(v) => {
                seed ^ v
                    .parse::<u64>()
                    .unwrap_or_else(|_| panic!("SPLAT_FUZZ_SEED={v:?} is no number"))
            }
            Err(_) => seed,
        };
        // Xorshift never leaves zero.
        Rng(if varied == 0 { seed | 1 } else { varied })
    }

    fn step(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number in `0..n`, or 0 when `n` is 0.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            usize::try_from(self.step() % n as u64).unwrap_or(0)
        }
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// A position of `text` on a char boundary.
    pub fn boundary(&mut self, text: &str) -> Byte {
        let mut at = self.below(text.len() + 1);
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        Byte::new(at)
    }

    pub fn range(&mut self, text: &str) -> Range<Byte> {
        let (a, b) = (self.boundary(text), self.boundary(text));
        a.min(b)..a.max(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_is_reduced_to_what_causes_it() {
        with_smallest_failure("alles gut", |_| {});
        let failure = std::panic::catch_unwind(|| {
            with_smallest_failure("vorne ä nein hinten", |text| {
                assert!(!text.contains("nein"), "fails on {text:?}");
            });
        })
        .unwrap_err();
        let message = failure.downcast_ref::<String>().unwrap();
        assert_eq!(message, "fails on \"nein\"");
    }
}
