//! What the self-test build does differently from the app that ships, in one place. Without the `selftest` feature each
//! item here is what ships, and both branches compile in either build. What the checks only read stays beside its
//! component, behind the feature; the checks themselves live in `editor::selftest`.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use objc2::MainThreadMarker;

/// Whether the self-test is built in.
pub(crate) const SELFTEST: bool = cfg!(feature = "selftest");

/// Panes and popovers animate in the app. Without a display, as on a locked screen, an animation never ends, and the
/// self-tests run there as well.
pub(crate) const ANIMATES: bool = !SELFTEST;

/// The wait before the first new try of a web picture that failed to load; a check waits for one.
pub(crate) const FIRST_RETRY: Duration = if SELFTEST {
    Duration::from_millis(500)
} else {
    Duration::from_secs(30)
};

/// Rounds of looks at the files whose answers have been taken in, for a check that something did not happen to wait for.
pub(crate) static LOOKS_TAKEN: AtomicUsize = AtomicUsize::new(0);

/// Saves that began because someone asked for one, unlike an autosave.
pub(crate) static SAVES_ASKED_FOR: AtomicUsize = AtomicUsize::new(0);

/// Where the next save or reload panics, which nothing else can bring about on demand.
pub(crate) static FAULT_AT: Mutex<Option<&'static str>> = Mutex::new(None);

/// What the app was last asked to open in the browser, which the self-test build records instead of opening it.
pub(crate) static LAST_OPENED: Mutex<String> = Mutex::new(String::new());

/// The end of the start, where the app offers what it recovered after a crash; the self-test starts there instead, as
/// the alert would stop it, and shows the alert itself.
pub(crate) fn after_launch(mtm: MainThreadMarker) {
    #[cfg(feature = "selftest")]
    crate::editor::selftest::start(mtm);
    #[cfg(not(feature = "selftest"))]
    crate::editor::offer_recovered(mtm);
}

/// A line of the self-test's log about what no check can see from outside.
pub(crate) fn note(line: impl FnOnce() -> String) {
    if SELFTEST {
        eprintln!("SELFTEST NOTE {}", line());
    }
}

/// Counts for a check.
pub(crate) fn count(counter: &AtomicUsize) {
    if SELFTEST {
        counter.fetch_add(1, Ordering::SeqCst);
    }
}

/// Panics at `place` when a check asked for it there.
pub(crate) fn fault_at(place: &str) {
    if !SELFTEST {
        return;
    }
    let struck = FAULT_AT
        .lock()
        .ok()
        .is_some_and(|mut fault| fault.take_if(|at| *at == place).is_some());
    assert!(!struck, "a fault for the selftest at {place}");
}

/// Whether `address` stays unopened, recorded for a check; the self-test starts no browser.
pub(crate) fn keeps_from_the_browser(address: &str) -> bool {
    if !SELFTEST {
        return false;
    }
    if let Ok(mut opened) = LAST_OPENED.lock() {
        address.clone_into(&mut opened);
    }
    true
}

/// The folder a self-test run writes to, which takes the place of the user's folders; set by `cargo xtask selftest`.
pub(crate) fn output_dir() -> Option<PathBuf> {
    if SELFTEST {
        std::env::var_os("SPLAT_SELFTEST_OUT").map(PathBuf::from)
    } else {
        None
    }
}

/// Runs `f`, and in a benchmark run says how long it took.
pub(crate) fn timed<T>(what: &str, f: impl FnOnce() -> T) -> T {
    if !SELFTEST || std::env::var_os("SPLAT_BENCH").is_none() {
        return f();
    }
    let started = std::time::Instant::now();
    let out = f();
    eprintln!("BENCH {what}: {:.1} ms", started.elapsed().as_secs_f64() * 1000.0);
    out
}

/// Whether a formula editor that never showed stays open, for a check that reads it on a locked screen.
pub(crate) fn keeps_unshown_editor() -> bool {
    #[cfg(feature = "selftest")]
    return crate::editor::selftest::keeps_unshown_editor();
    #[cfg(not(feature = "selftest"))]
    false
}

/// Web pictures in the self-test come from a server inside the app, so no check depends on one somewhere.
pub(crate) fn serve_the_web_inside(config: &objc2_foundation::NSURLSessionConfiguration) {
    #[cfg(feature = "selftest")]
    // SAFETY: The array holds a subclass of NSURLProtocol.
    unsafe {
        config.setProtocolClasses(Some(&crate::editor::selftest::protocol_classes()));
    }
    #[cfg(not(feature = "selftest"))]
    let _ = config;
}
