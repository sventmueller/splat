//! Large blocks come straight from the kernel and go back to it when freed.

use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::c_void;

/// The size from which the system allocator also hands out separate large blocks.
const LARGE: usize = 128 * 1024;
/// mmap guarantees page alignment; Intel Macs have 4 KB pages, Apple silicon 16 KB.
const PAGE: usize = 4 * 1024;

const PROT_READ: i32 = 1;
const PROT_WRITE: i32 = 2;
const MAP_PRIVATE: i32 = 0x0002;
const MAP_ANON: i32 = 0x1000;

unsafe extern "C" {
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> i32;
}

pub struct LargeAlloc;

fn is_large(layout: &Layout) -> bool {
    layout.size() >= LARGE && layout.align() <= PAGE
}

fn map(size: usize) -> *mut u8 {
    // SAFETY: An anonymous mapping without address hint; failure is checked below.
    let p = unsafe {
        mmap(
            std::ptr::null_mut(),
            size,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANON,
            -1,
            0,
        )
    };
    if p as isize == -1 {
        std::ptr::null_mut()
    } else {
        p.cast()
    }
}

// SAFETY: every block goes back to where it came from, which `is_large` decides from the same layout both times;
// mmap returns page-aligned memory, enough for any alignment `is_large` accepts.
unsafe impl GlobalAlloc for LargeAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if is_large(&layout) {
            map(layout.size())
        } else {
            // SAFETY: Delegates with the caller's layout, which GlobalAlloc requires to be valid.
            unsafe { System.alloc(layout) }
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if is_large(&layout) {
            // Fresh anonymous pages are zero already.
            map(layout.size())
        } else {
            // SAFETY: Delegates with the caller's layout, which GlobalAlloc requires to be valid.
            unsafe { System.alloc_zeroed(layout) }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if is_large(&layout) {
            // SAFETY: Large blocks come from `map` with this size, so the whole mapping goes back.
            unsafe { munmap(ptr.cast(), layout.size()) };
        } else {
            // SAFETY: Small blocks come from the system allocator with this layout.
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: The alignment comes from a valid layout and GlobalAlloc::realloc requires the size to fit it.
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        if !is_large(&layout) && !is_large(&new_layout) {
            // SAFETY: Both layouts are small, so the block came from the system allocator.
            return unsafe { System.realloc(ptr, layout, new_size) };
        }
        // SAFETY: The new layout is valid, see above.
        let new = unsafe { self.alloc(new_layout) };
        if !new.is_null() {
            // SAFETY: Both blocks hold at least the copied length and do not overlap; the old one is freed once.
            unsafe {
                std::ptr::copy_nonoverlapping(ptr, new, layout.size().min(new_size));
                self.dealloc(ptr, layout);
            }
        }
        new
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn large_blocks_survive_growth_and_shrinking() {
        let mut v: Vec<u8> = Vec::with_capacity(10);
        for i in 0..3_000_000u32 {
            v.push(i as u8);
        }
        assert!(v.iter().enumerate().all(|(i, &b)| b == i as u8));
        v.truncate(1000);
        v.shrink_to_fit();
        assert!(v.iter().enumerate().all(|(i, &b)| b == i as u8));
        let zeroed = vec![0u64; 1 << 20];
        assert!(zeroed.iter().all(|&x| x == 0));
    }
}
