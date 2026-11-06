//! How much memory a piece of work holds at once, for inputs whose memory has to grow as they do. A test binary that
//! checks memory installs [`Counting`] as its global allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// The system's allocator, counting the bytes of a thread inside [`peak_of`].
pub struct Counting;

#[derive(Clone, Copy)]
struct Count {
    on: bool,
    /// What the thread holds beyond what it held when counting began; freeing what it held before makes this negative.
    held: isize,
    peak: isize,
}

const OFF: Count = Count {
    on: false,
    held: 0,
    peak: 0,
};

thread_local! {
    // A plain number with a constant start allocates nothing and is never destroyed, as an allocator needs.
    static COUNT: Cell<Count> = const { Cell::new(OFF) };
}

fn signed(bytes: usize) -> isize {
    isize::try_from(bytes).unwrap_or(isize::MAX)
}

fn note(change: isize) {
    let _ = COUNT.try_with(|count| {
        let mut now = count.get();
        if now.on {
            now.held = now.held.saturating_add(change);
            now.peak = now.peak.max(now.held);
            count.set(now);
        }
    });
}

// SAFETY: Every call goes to the system's allocator unchanged; counting only writes a thread-local number.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller keeps the promises of `GlobalAlloc::alloc`, which are those of `System`.
        let at = unsafe { System.alloc(layout) };
        if !at.is_null() {
            note(signed(layout.size()));
        }
        at
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: As in `alloc`.
        let at = unsafe { System.alloc_zeroed(layout) };
        if !at.is_null() {
            note(signed(layout.size()));
        }
        at
    }

    unsafe fn dealloc(&self, at: *mut u8, layout: Layout) {
        // SAFETY: `at` came from this allocator, which is `System`, with `layout`.
        unsafe { System.dealloc(at, layout) };
        note(-signed(layout.size()));
    }

    unsafe fn realloc(&self, at: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `at` came from this allocator, which is `System`, with `layout`, and the caller keeps the promises about
        // `new_size`.
        let moved = unsafe { System.realloc(at, layout, new_size) };
        if !moved.is_null() {
            note(signed(new_size) - signed(layout.size()));
        }
        moved
    }
}

/// Puts back what counted before, with what was counted inside added to it, also when the work panics.
struct Resume(Count);

impl Drop for Resume {
    fn drop(&mut self) {
        let inside = COUNT.get();
        let mut outside = self.0;
        if outside.on {
            outside.peak = outside.peak.max(outside.held.saturating_add(inside.peak));
            outside.held = outside.held.saturating_add(inside.held);
        }
        COUNT.set(outside);
    }
}

/// What `work` returns, and the most bytes this thread held at once while it ran, beyond what it held before. What other
/// threads allocate does not count, nor what this one frees of theirs.
pub fn peak_of<T>(work: impl FnOnce() -> T) -> (T, usize) {
    let _resume = Resume(COUNT.get());
    COUNT.set(Count { on: true, ..OFF });
    let result = work();
    let peak = usize::try_from(COUNT.get().peak).unwrap_or(0);
    (result, peak)
}

/// Whether [`Counting`] is the allocator of this program; without it every peak is nothing.
pub fn counts() -> bool {
    const SOME: usize = 1 << 12;
    peak_of(|| drop(std::hint::black_box(Vec::<u8>::with_capacity(SOME)))).1 >= SOME
}

/// Below this a peak says little: buffers of the standard library and caches of the code outweigh the input.
pub const NOTICEABLE: usize = 16 << 20;

/// Whether memory that grows from `small` to `large` bytes with an input growing from `small_len` to `large_len` is more
/// than linear growth, with room for buffers that double.
pub fn grows_too_much(small: usize, large: usize, small_len: usize, large_len: usize) -> bool {
    const SMALLEST: usize = 1 << 20;
    const DOUBLING: f64 = 2.5;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a ratio of input lengths and of memory sizes far below 2^52"
    )]
    let (allowed, grew) = (
        large_len as f64 / small_len.max(1) as f64 * DOUBLING,
        large as f64 / small.max(SMALLEST) as f64,
    );
    large > NOTICEABLE && grew > allowed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_that_grows_faster_than_the_input_is_found() {
        const MB: usize = 1 << 20;
        assert!(!grows_too_much(10 * MB, 40 * MB, 1000, 4000), "linear");
        assert!(!grows_too_much(10 * MB, 80 * MB, 1000, 4000), "linear, past a doubling");
        assert!(grows_too_much(10 * MB, 160 * MB, 1000, 4000), "quadratic");
        assert!(!grows_too_much(MB / 100, 15 * MB, 1000, 4000), "not noticeable");
        assert!(
            grows_too_much(1000, 20 * MB, 1000, 4000),
            "a small peak counts for at least a little"
        );
    }

    /// Memory for `n` rows of `n` bytes each, all held at once.
    fn square(n: usize) -> usize {
        let rows: Vec<Vec<u8>> = (0..n).map(|_| std::hint::black_box(vec![1u8; n])).collect();
        rows.len()
    }

    /// Memory for `n` bytes, held twice over in turn.
    fn line(n: usize) -> usize {
        let first = std::hint::black_box(vec![1u8; n]);
        drop(first);
        std::hint::black_box(vec![2u8; n]).len()
    }

    #[test]
    fn what_a_thread_holds_is_counted() {
        assert!(counts(), "the unit tests count with `Counting`");
        let ((), peak) = peak_of(|| {
            line(32 << 20);
        });
        assert!(
            (32 << 20..33 << 20).contains(&peak),
            "{peak}: freed memory still counts"
        );
        let (_, nested) = peak_of(|| peak_of(|| line(8 << 20)));
        assert!(
            nested >= 8 << 20,
            "{nested}: what an inner count saw is lost to the outer one"
        );
        let (_, elsewhere) = peak_of(|| std::thread::spawn(|| line(32 << 20)).join());
        assert!(elsewhere < 1 << 20, "{elsewhere}: another thread's memory counts");
    }

    #[test]
    fn a_quadratic_allocation_is_caught_and_a_linear_one_passes() {
        let measured = |work: fn(usize) -> usize, n: usize| peak_of(|| work(n)).1;
        let (small, large) = (measured(square, 2 << 10), measured(square, 8 << 10));
        assert!(grows_too_much(small, large, 2 << 10, 8 << 10), "{small} then {large}");
        let (small, large) = (measured(line, 8 << 20), measured(line, 32 << 20));
        assert!(!grows_too_much(small, large, 8 << 20, 32 << 20), "{small} then {large}");
    }
}
