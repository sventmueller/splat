//! A picture as the previews hold it, whoever made it, and what all of them together may take of the memory.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use objc2::rc::Retained;
use objc2_app_kit::NSImage;
use objc2_foundation::NSSize;

use splat_decoder::{Decoded, SHOWN_SIDE};
use splat_file_state::Stamp;

use crate::lanes::Lane;
use crate::volumes::{LanesByVolume, lane_of_volume, own_lane};

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

/// What became of reading an image file.
pub struct ImageFile {
    pub(crate) loaded: Option<Decoded>,
    pub(crate) file: PathBuf,
    pub(crate) seen: Stamp,
    pub(crate) side: u32,
}

impl ImageFile {
    /// What a load that ended in a panic leaves: nothing to show, and no reason to wait for it any longer.
    pub(crate) fn failed(file: PathBuf) -> ImageFile {
        ImageFile {
            loaded: None,
            file,
            seen: Stamp::UNREACHABLE,
            side: allowed_side(),
        }
    }
}

/// Decoded pixels held by all open documents; attachments in the previews keep them alive as well.
static DECODED: AtomicUsize = AtomicUsize::new(0);
static LOW_MEMORY: AtomicBool = AtomicBool::new(false);

pub fn count_decoded(bytes: isize) {
    if bytes >= 0 {
        DECODED.fetch_add(crate::appkit::to_usize(bytes), Ordering::Relaxed);
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

pub fn allowed_side() -> u32 {
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
pub fn room_for_more() -> bool {
    has_room(
        DECODED.load(Ordering::Relaxed),
        budget(),
        LOW_MEMORY.load(Ordering::Relaxed),
    )
}

/// The system says that memory is short, or no longer so.
pub fn set_low_memory(low: bool) {
    LOW_MEMORY.store(low, Ordering::Relaxed);
}

/// Image files and downloads are decoded by as many threads as there are decoder helpers, so a document with hundreds
/// of pictures holds only a few of the files in memory at a time.
pub(crate) static LOADERS: Lane = Lane::new("splat-image").with_workers(splat_decoder::HELPER_COUNT);

/// Where an image file on `volume` loads. One that may stop answering gets threads of its own, as many as `LOADERS`, so that
/// a hang there holds up only its own pictures and not the others.
pub(crate) fn loaders_of(volume: &str) -> &'static Lane {
    static LANES: LanesByVolume = Mutex::new(None);
    if volume == "/" {
        return &LOADERS;
    }
    own_lane(&LANES, volume, || {
        Lane::new("splat-image-volume").with_workers(splat_decoder::HELPER_COUNT)
    })
    .unwrap_or_else(|| lane_of_volume(volume))
}

pub(crate) fn load_in_background(job: impl FnOnce() + Send + 'static) {
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

    #[test]
    fn pictures_on_another_volume_load_side_by_side_and_a_hang_there_holds_up_only_its_own() {
        use std::sync::mpsc::channel;
        use std::sync::{Arc, Condvar};
        use std::time::Duration;
        let wait = Duration::from_secs(5);
        let started = Arc::new((Mutex::new(0), Condvar::new()));
        let (seen, all_seen) = channel();
        for _ in 0..splat_decoder::HELPER_COUNT {
            let (started, seen) = (started.clone(), seen.clone());
            loaders_of("/Volumes/Fotos").run(move || {
                let (count, changed) = &*started;
                let mut count = count.lock().unwrap();
                *count += 1;
                changed.notify_all();
                let (count, _) = changed
                    .wait_timeout_while(count, wait, |n| *n < splat_decoder::HELPER_COUNT)
                    .unwrap();
                seen.send(*count).unwrap();
            });
        }
        for _ in 0..splat_decoder::HELPER_COUNT {
            assert_eq!(
                all_seen.recv_timeout(wait * 2),
                Ok(splat_decoder::HELPER_COUNT),
                "one loads while the others load"
            );
        }

        let (release, hung) = channel::<()>();
        let hung = Arc::new(Mutex::new(hung));
        for _ in 0..splat_decoder::HELPER_COUNT {
            let hung = hung.clone();
            loaders_of("/Volumes/Haengt").run(move || {
                let _ = hung.lock().unwrap().recv();
            });
        }
        let (done, finished) = channel();
        for volume in ["/Volumes/Anderes", "/"] {
            let done = done.clone();
            loaders_of(volume).run(move || done.send(()).unwrap());
        }
        for _ in 0..2 {
            finished.recv_timeout(wait).expect("pictures elsewhere go on");
        }
        assert!(std::ptr::eq(loaders_of("/"), &LOADERS));
        assert!(std::ptr::eq(
            loaders_of("/Volumes/Haengt"),
            loaders_of("/Volumes/Haengt")
        ));
        drop(release);
    }
}
