#[cfg(feature = "selftest")]
pub use splat_editor_state::lanes::uncontained;
pub use splat_editor_state::lanes::{
    BACKGROUND, DISK, Lane, contained, free_in_background, panic_is_contained, with_raised,
};

/// What a callback from the system does, with `fallback` if a bug in it panics. A panic would otherwise unwind through the
/// system's frames; the text stays safe in its view, and the next update starts from there.
pub fn guarded<R>(fallback: R, f: impl FnOnce() -> R) -> R {
    contained(f).unwrap_or(fallback)
}

/// Runs `f` on the main thread when its turn comes. dispatch2 calls it through `extern "C"`, so a panic that left it would
/// end the app.
pub fn main_soon(f: impl FnOnce() + Send + 'static) {
    dispatch2::DispatchQueue::main().exec_async(move || {
        contained(f);
    });
}

/// `main_soon` after `delay`.
pub fn main_after(delay: std::time::Duration, f: impl FnOnce() + Send + 'static) {
    if let Ok(when) = dispatch2::DispatchTime::try_from(delay) {
        let _ = dispatch2::DispatchQueue::main().after(when, move || {
            contained(f);
        });
    }
}

/// What `f` returns on the main thread; `None` if it panicked. For a thread the main thread never waits for.
pub fn on_main<T: Send>(f: impl FnOnce() -> T + Send) -> Option<T> {
    if objc2::MainThreadMarker::new().is_some() {
        return contained(f);
    }
    let mut out = None;
    dispatch2::DispatchQueue::main().exec_sync(|| out = contained(f));
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::main_after;

    #[test]
    fn a_delay_too_long_for_dispatch_holds_on_to_nothing() {
        let work = Arc::new(());
        let held = work.clone();
        main_after(Duration::from_secs(u64::MAX / 2), move || drop(held));
        assert_eq!(Arc::strong_count(&work), 1);
    }
}
