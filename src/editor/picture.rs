//! A picture as the previews hold it, whoever made it, and what all of them together may take of the memory.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use objc2::rc::Retained;
use objc2_app_kit::NSImage;
use objc2_foundation::NSSize;

use super::safe_image::SHOWN_SIDE;
use crate::util::Lane;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sizing {
    /// Fixed size in points; `descent` is how far the image reaches below the baseline.
    Points { size: NSSize, descent: f64 },
    /// MathJax metrics in ex of the surrounding font, so formulas scale with the text.
    Ex { width: f64, height: f64, valign: f64 },
}

#[derive(Clone)]
pub struct Rendered {
    pub image: Retained<NSImage>,
    pub sizing: Sizing,
}

/// Decoded pixels held by all open documents; attachments in the previews keep them alive as well.
static DECODED: AtomicUsize = AtomicUsize::new(0);
static LOW_MEMORY: AtomicBool = AtomicBool::new(false);

pub(super) fn count_decoded(bytes: isize) {
    if bytes >= 0 {
        DECODED.fetch_add(crate::util::to_usize(bytes), Ordering::Relaxed);
    } else {
        DECODED.fetch_sub(bytes.unsigned_abs(), Ordering::Relaxed);
    }
}

/// The longest side an image decodes to: smaller as pixels pile up, so a document with hundreds of photos stays
/// usable instead of driving the Mac into swapping.
fn side_for(decoded: usize, budget: usize, low_memory: bool) -> u32 {
    if low_memory || decoded >= budget {
        640
    } else if decoded >= budget / 2 {
        1280
    } else {
        SHOWN_SIDE
    }
}

fn budget() -> usize {
    static BUDGET: OnceLock<usize> = OnceLock::new();
    *BUDGET.get_or_init(|| (objc2_foundation::NSProcessInfo::processInfo().physicalMemory() / 8) as usize)
}

pub(super) fn allowed_side() -> u32 {
    side_for(
        DECODED.load(Ordering::Relaxed),
        budget(),
        LOW_MEMORY.load(Ordering::Relaxed),
    )
}

fn has_room(decoded: usize, budget: usize, low_memory: bool) -> bool {
    decoded < if low_memory { budget } else { budget.saturating_mul(2) }
}

/// Whether one more picture may decode. A document can name any number of them, so smaller pictures alone set no limit;
/// what is shown stays, and what comes on top shows no picture until there is room again.
pub(super) fn room_for_more() -> bool {
    has_room(
        DECODED.load(Ordering::Relaxed),
        budget(),
        LOW_MEMORY.load(Ordering::Relaxed),
    )
}

/// The system says that memory is short, or no longer so.
pub(super) fn set_low_memory(low: bool) {
    LOW_MEMORY.store(low, Ordering::Relaxed);
}

/// Image files and downloads are decoded by as many threads as there are decoder helpers, so a document with hundreds
/// of pictures holds only a few of the files in memory at a time.
pub(super) static LOADERS: Lane = Lane::new("splat-image").with_workers(super::decoder::HELPER_COUNT);

pub(super) fn load_in_background(job: impl FnOnce() + Send + 'static) {
    LOADERS.run(job);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_decode_smaller_as_pixels_pile_up() {
        let budget = 1 << 30;
        assert_eq!(side_for(0, budget, false), SHOWN_SIDE);
        assert_eq!(side_for(budget / 2, budget, false), 1280);
        assert_eq!(side_for(budget, budget, false), 640);
        assert_eq!(side_for(0, budget, true), 640);
    }

    #[test]
    fn beyond_twice_the_budget_no_picture_decodes_and_beyond_it_none_while_memory_is_short() {
        let budget = 1 << 30;
        assert!(has_room(0, budget, false));
        assert!(has_room(2 * budget - 1, budget, false));
        assert!(!has_room(2 * budget, budget, false));
        assert!(has_room(budget - 1, budget, true));
        assert!(!has_room(budget, budget, true));
    }
}
