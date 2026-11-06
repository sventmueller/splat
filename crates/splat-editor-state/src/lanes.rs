//! Worker threads, and code run so that a panic costs one result and not the app: the main thread survives a panic in
//! `contained`, and what has to end ends then as well.

use std::cell::Cell;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

type Job = Box<dyn FnOnce() + Send>;

/// Threads that run jobs one after the other, started by the first job. A job that panics ends only itself: every job
/// behind it would otherwise wait forever.
pub struct Lane {
    name: &'static str,
    workers: usize,
    stack: Option<usize>,
    jobs: OnceLock<Sender<Job>>,
}

impl Lane {
    /// A lane of one thread called `name`.
    pub const fn new(name: &'static str) -> Lane {
        Lane {
            name,
            workers: 1,
            stack: None,
            jobs: OnceLock::new(),
        }
    }

    /// The lane with `workers` threads, which take the next job as each comes free.
    pub const fn with_workers(mut self, workers: usize) -> Lane {
        self.workers = workers;
        self
    }

    /// The lane with threads of a stack of `bytes`.
    pub const fn with_stack(mut self, bytes: usize) -> Lane {
        self.stack = Some(bytes);
        self
    }

    /// Runs `job` on the lane when its turn comes.
    pub fn run(&self, job: impl FnOnce() + Send + 'static) {
        let jobs = self.jobs.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Job>();
            let rx = Arc::new(Mutex::new(rx));
            for n in 0..self.workers {
                let rx = rx.clone();
                let mut thread = std::thread::Builder::new().name(if self.workers == 1 {
                    self.name.into()
                } else {
                    format!("{}-{n}", self.name)
                });
                if let Some(bytes) = self.stack {
                    thread = thread.stack_size(bytes);
                }
                thread
                    .spawn(move || {
                        // The lock is held while waiting only, and each worker takes one job at a time.
                        while let Ok(Ok(job)) = rx.lock().map(|rx| rx.recv()) {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                        }
                    })
                    .expect("lane thread");
            }
            tx
        });
        let _ = jobs.send(Box::new(job));
    }

    /// Runs `work` here and hands what it returns, or `None` after a panic, to `answer` on this thread. Whoever waits for
    /// the answer hears back either way.
    pub fn ask<T>(&self, work: impl FnOnce() -> T + Send + 'static, answer: impl FnOnce(Option<T>) + Send + 'static) {
        self.run(move || answer(contained(work)));
    }
}

thread_local! {
    static CONTAINED: Cell<bool> = const { Cell::new(false) };
}

/// Whether the panic under way ends in `contained`, so the app goes on.
pub fn panic_is_contained() -> bool {
    CONTAINED.with(Cell::get)
}

/// Runs `f`; `None` if it panicked. A bug in parsing or layout then costs the preview of this text and a bug in a command
/// that command, not the app: all of them compute values, and the text stays safe in its view.
pub fn contained<T>(f: impl FnOnce() -> T) -> Option<T> {
    let before = CONTAINED.replace(true);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok();
    CONTAINED.set(before);
    result
}

/// Runs `f` as if no `contained` were around it, for a check of what a panic outside of one does.
#[cfg(feature = "selftest")]
pub fn uncontained<T>(f: impl FnOnce() -> T) -> T {
    let before = CONTAINED.replace(false);
    let result = f();
    CONTAINED.set(before);
    result
}

/// Raises `flag` for the length of `f` and puts back what it was, also when `f` panics, as the main thread survives that.
pub fn with_raised<R>(flag: &Cell<bool>, f: impl FnOnce() -> R) -> R {
    struct Restore<'a>(&'a Cell<bool>, bool);
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            self.0.set(self.1);
        }
    }
    let _restore = Restore(flag, flag.replace(true));
    f()
}

/// CPU work off the main thread, about as much at a time as the decoder has helpers: a picture that takes long leaves the
/// others free to go on.
pub static BACKGROUND: Lane = Lane::new("splat-render").with_workers(3);

/// Lets a value go; on a lane of its own, since a large tree that waits here must not delay a formula, nor a formula that
/// takes long hold up memory that could be free.
static FREEING: Lane = Lane::new("splat-free");

/// Frees a large value elsewhere: a whole document tree takes long enough to be felt while typing.
pub fn free_in_background<T: Send + 'static>(value: T) {
    FREEING.run(move || drop(value));
}

/// Everything that touches the user's folders: a network volume that stopped answering may hold this thread for
/// minutes, and the main thread must not wait for it.
pub static DISK: Lane = Lane::new("splat-disk");

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[test]
    fn a_lane_keeps_going_through_panics_and_loses_no_job() {
        static LANE: Lane = Lane::new("splat-test-stress").with_workers(4);
        let (done, finished) = std::sync::mpsc::channel();
        const JOBS: usize = 2000;
        for n in 0..JOBS {
            let done = done.clone();
            LANE.run(move || {
                assert!(n % 7 != 0, "a job that fails");
                done.send(n).unwrap();
            });
        }
        drop(done);
        let mut ran: Vec<usize> = finished.iter().collect();
        ran.sort_unstable();
        assert_eq!(ran, (0..JOBS).filter(|n| n % 7 != 0).collect::<Vec<_>>());
    }

    #[test]
    fn whoever_asks_a_lane_hears_back_even_after_a_panic() {
        static LANE: Lane = Lane::new("splat-test-answers").with_stack(1 << 20);
        let (answered, answers) = std::sync::mpsc::channel();
        let told = answered.clone();
        LANE.ask(
            || -> u32 { panic!("a job that fails") },
            move |answer| told.send(answer).unwrap(),
        );
        LANE.ask(|| 7, move |answer| answered.send(answer).unwrap());
        assert_eq!(answers.recv().unwrap(), None);
        assert_eq!(answers.recv().unwrap(), Some(7));
    }

    #[test]
    fn the_shared_lanes_run_what_they_are_given() {
        let (done, finished) = std::sync::mpsc::channel();
        let on_disk = done.clone();
        BACKGROUND.run(move || done.send("background").unwrap());
        DISK.run(move || on_disk.send("disk").unwrap());
        free_in_background(vec![0u8; 16]);
        let mut ran: Vec<&str> = finished.iter().take(2).collect();
        ran.sort_unstable();
        assert_eq!(ran, ["background", "disk"]);
    }

    #[test]
    fn a_panic_ends_in_contained_and_is_known_while_it_unwinds() {
        assert_eq!(contained(|| 7), Some(7));
        assert!(!panic_is_contained());
        let during = Arc::new(AtomicBool::new(false));
        let seen = during.clone();
        let previous = std::panic::take_hook();
        // Other tests panic at the same time, and their panics are not contained.
        std::panic::set_hook(Box::new(move |_| {
            seen.fetch_or(panic_is_contained(), Ordering::SeqCst);
        }));
        let result = contained(|| -> u8 { panic!("a bug in the parser") });
        std::panic::set_hook(previous);
        assert_eq!(result, None);
        assert!(during.load(Ordering::SeqCst));
        assert!(!panic_is_contained());
    }

    #[test]
    fn a_raised_flag_goes_back_to_what_it_was() {
        let flag = Cell::new(false);
        with_raised(&flag, || {
            assert!(flag.get());
            with_raised(&flag, || assert!(flag.get()));
            assert!(flag.get(), "the inner one must not lower it for the outer one");
        });
        assert!(!flag.get());
    }

    #[test]
    fn a_raised_flag_comes_down_after_a_panic() {
        let flag = Cell::new(false);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_raised(&flag, || panic!("contained"));
        }));
        assert!(outcome.is_err());
        assert!(!flag.get(), "the callbacks would ignore the user from then on");
    }
}
