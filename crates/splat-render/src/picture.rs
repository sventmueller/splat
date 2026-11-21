//! A picture as the previews hold it, whoever made it, and what all of them together may take of the memory.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use objc2::rc::Retained;
use objc2_app_kit::NSImage;
use objc2_foundation::NSSize;

pub use splat_decoder::Fit;
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
    pub(crate) fit: Fit,
    /// Read for more pixels than a picture already shown has: without new ones, that picture stays.
    pub(crate) grows: bool,
}

impl ImageFile {
    /// What a load that ended in a panic leaves: nothing to show, and no reason to wait for it any longer.
    pub(crate) fn failed(file: PathBuf) -> ImageFile {
        ImageFile {
            loaded: None,
            file,
            seen: Stamp::UNREACHABLE,
            fit: Fit::within(allowed_side()),
            grows: false,
        }
    }
}

/// A picture as it shows, from what a decoder made of its file.
pub(crate) fn rendered(loaded: Decoded) -> Option<Rendered> {
    let image = splat_decoder::image(loaded)?;
    let size = image.size();
    Some(Rendered {
        image,
        sizing: Sizing::Points { size, descent: 0.0 },
    })
}

/// Pixels are decoded in steps of this many, so that a window resized a little keeps the pictures it has.
const STEP: u32 = 256;

/// The pixels a picture needs to look sharp in the preview: the size in points the document asks for, or for one that asks
/// for none the width of the column, which no picture there goes beyond, at `zoom` and `scale` pixels per point.
pub fn needed(asked: (Option<u32>, Option<u32>), zoom: f64, column: f64, scale: f64) -> Fit {
    on_screen(asked, zoom, column, scale, allowed_side())
}

fn on_screen(asked: (Option<u32>, Option<u32>), zoom: f64, column: f64, scale: f64, side: u32) -> Fit {
    let (width, height) = match (asked.0.map(f64::from), asked.1.map(f64::from)) {
        (Some(w), Some(h)) if w * zoom > column => (column, h * column / w),
        (Some(w), Some(h)) => (w * zoom, h * zoom),
        (Some(w), None) => ((w * zoom).min(column), 0.0),
        (None, Some(h)) => (0.0, h * zoom),
        (None, None) => (column, 0.0),
    };
    let pixels = |points: f64| {
        let steps = (points * scale / f64::from(STEP)).ceil();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "steps of a picture, of which `side` takes the most"
        )]
        let steps = if steps.is_finite() && steps > 0.0 {
            steps.min(f64::from(side)) as u32
        } else {
            0
        };
        steps.saturating_mul(STEP).min(side)
    };
    Fit {
        width: pixels(width),
        height: pixels(height),
        side,
    }
}

/// What a picture was decoded for, and the pixels that came of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Decode {
    pub(crate) fit: Fit,
    pub(crate) pixels: (u32, u32),
}

impl Decode {
    /// The decode of pixels from `decoded`, asked for with `fit`.
    pub(crate) fn of(decoded: &Decoded, fit: Fit) -> Option<Decode> {
        match decoded {
            Decoded::Rgba { width, height, .. } => Some(Decode {
                fit,
                pixels: (*width, *height),
            }),
            Decoded::Pdf(_) | Decoded::Png(_) => None,
        }
    }

    /// Whether these pixels serve `need` as well as decoding again would: they cover it, a new decode may have no longer
    /// side, or they fell short of what they were asked for below their limit, and so are all the file holds.
    pub(crate) fn serves(self, need: Fit) -> bool {
        let (width, height) = self.pixels;
        let longest = width.max(height);
        let covers = width >= need.width && height >= need.height;
        let whole = longest < self.fit.side && (width < self.fit.width || height < self.fit.height);
        covers || longest >= need.side || whole
    }
}

/// The most pixels the views of a picture asked for, raised as they ask and read when its decode begins.
#[derive(Default, Debug)]
pub(crate) struct Wanted {
    width: AtomicU32,
    height: AtomicU32,
}

impl Wanted {
    pub(crate) fn of(need: Fit) -> Arc<Wanted> {
        let wanted = Arc::new(Wanted::default());
        wanted.raise(need);
        wanted
    }

    pub(crate) fn raise(&self, need: Fit) {
        self.width.fetch_max(need.width, Ordering::Relaxed);
        self.height.fetch_max(need.height, Ordering::Relaxed);
    }

    /// What a decode asks for, within `side`.
    pub(crate) fn fit(&self, side: u32) -> Fit {
        Fit {
            width: self.width.load(Ordering::Relaxed),
            height: self.height.load(Ordering::Relaxed),
            side,
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

    fn fit(width: u32, height: u32, side: u32) -> Fit {
        Fit { width, height, side }
    }

    #[test]
    fn a_picture_needs_the_pixels_of_the_column_in_steps() {
        let column = |width: f64, scale: f64| on_screen((None, None), 1.0, width, scale, SHOWN_SIDE);
        assert_eq!(column(700.0, 2.0), fit(1536, 0, SHOWN_SIDE), "1400 pixels, a step up");
        assert_eq!(column(640.0, 2.0), fit(1280, 0, SHOWN_SIDE), "on a step");
        assert_eq!(column(641.0, 2.0), fit(1536, 0, SHOWN_SIDE));
        assert_eq!(
            column(700.0, 1.0),
            fit(768, 0, SHOWN_SIDE),
            "on a screen without Retina"
        );
        assert_eq!(
            column(690.0, 2.0),
            column(700.0, 2.0),
            "a little narrower decodes nothing"
        );
        assert_eq!(
            column(1500.0, 2.0),
            fit(SHOWN_SIDE, 0, SHOWN_SIDE),
            "never above the limit"
        );
        assert_eq!(
            on_screen((None, None), 1.0, 700.0, 2.0, 1280),
            fit(1280, 0, 1280),
            "nor above the one of short memory"
        );
        assert_eq!(
            on_screen((None, None), 3.0, 700.0, 2.0, SHOWN_SIDE),
            column(700.0, 2.0),
            "a zoom shows no picture wider than the column"
        );
    }

    #[test]
    fn a_size_the_document_asks_for_follows_the_zoom_up_to_the_column() {
        let asked = |asked, zoom| on_screen(asked, zoom, 700.0, 2.0, SHOWN_SIDE);
        assert_eq!(asked((Some(200), None), 1.0), fit(512, 0, SHOWN_SIDE));
        assert_eq!(asked((Some(200), None), 1.331), fit(768, 0, SHOWN_SIDE), "zoomed in");
        assert_eq!(
            asked((Some(2000), None), 1.0),
            fit(1536, 0, SHOWN_SIDE),
            "within the column"
        );
        assert_eq!(asked((None, Some(300)), 1.0), fit(0, 768, SHOWN_SIDE), "by its height");
        assert_eq!(
            asked((Some(100), Some(400)), 1.0),
            fit(256, 1024, SHOWN_SIDE),
            "stretched"
        );
        assert_eq!(
            asked((Some(1400), Some(700)), 1.0),
            fit(1536, 768, SHOWN_SIDE),
            "stretched, then made as narrow as the column"
        );
        assert_eq!(
            asked((Some(u32::MAX), None), 3.0),
            fit(1536, 0, SHOWN_SIDE),
            "any number a document gives"
        );
        assert_eq!(
            on_screen((None, None), 1.0, f64::NAN, 2.0, SHOWN_SIDE),
            fit(0, 0, SHOWN_SIDE)
        );
    }

    #[test]
    fn pixels_decoded_large_enough_are_kept_and_others_decoded_again() {
        let column = fit(1536, 0, SHOWN_SIDE);
        let photo = Decode {
            fit: column,
            pixels: (1536, 1152),
        };
        assert!(photo.serves(column), "as large as needed");
        assert!(photo.serves(fit(1280, 0, SHOWN_SIDE)), "a narrower window");
        assert!(!photo.serves(fit(2048, 0, SHOWN_SIDE)), "a wider column");
        assert!(!photo.serves(fit(0, 1280, SHOWN_SIDE)), "a size asking for more height");
        assert!(
            !photo.serves(Fit::within(SHOWN_SIDE)),
            "paper takes the most a picture may have"
        );
        assert!(
            photo.serves(fit(2048, 0, 1280)),
            "while memory is short, a new decode would be smaller"
        );
        let small = Decode {
            fit: column,
            pixels: (800, 600),
        };
        assert!(small.serves(Fit::within(SHOWN_SIDE)), "the file holds no more");
        let capped = Decode {
            fit: fit(1536, 0, 640),
            pixels: (640, 480),
        };
        assert!(capped.serves(fit(1536, 0, 640)), "the limit still holds");
        assert!(!capped.serves(column), "memory is back");
        let full = Decode {
            fit: Fit::within(SHOWN_SIDE),
            pixels: (SHOWN_SIDE, 1920),
        };
        assert!(full.serves(Fit::within(SHOWN_SIDE)) && full.serves(column));
    }

    #[test]
    fn a_decode_asks_for_the_most_any_view_wanted() {
        let wanted = Wanted::of(fit(512, 0, SHOWN_SIDE));
        wanted.raise(fit(256, 768, 640));
        assert_eq!(wanted.fit(SHOWN_SIDE), fit(512, 768, SHOWN_SIDE));
        assert_eq!(wanted.fit(640).side, 640, "the side is the one of the moment it begins");
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
