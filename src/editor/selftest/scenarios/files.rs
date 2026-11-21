//! Image files, the memory their pictures take, and a text in an old encoding.

use std::cell::Cell;

use block2::RcBlock;
use objc2::MainThreadOnly;
use objc2_app_kit::{NSAttachmentAttributeName, NSDocumentController, NSTextAttachment};
use objc2_foundation::{NSRange, NSURL};

use super::super::{Ctx, Step, append, applied, check, perform, settle, when};
use super::{doc_dir, documents, remember, undone};
use crate::editor::actions::Action;
use crate::editor::decoder::SHOWN_SIDE;
use crate::editor::picture::Fit;
use crate::util::{ns, storage};

/// A PNG of `side` × `side` pixels.
fn png_of_side(side: u32) -> Vec<u8> {
    png_of(side, side)
}

fn png_of(width: u32, height: u32) -> Vec<u8> {
    let mut png = Vec::new();
    let _ = image::RgbaImage::from_pixel(width, height, image::Rgba([30, 160, 60, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    png
}

/// The width of the image in `file`, once the preview has it.
fn shown_width(c: &Ctx, file: &str) -> Option<f64> {
    let path = doc_dir(c).join(file);
    c.e.pictures().store().borrow().shown_size(&path).map(|s| s.width)
}

/// The pictures of the checks on their pixels, a file and one from the web, larger than any column shows them; the
/// width their document asks for.
#[derive(Clone, Copy)]
enum Large {
    File,
    Web,
}

const LARGE_FILE: &str = "gross.png";
const ASKED_WIDTH: u32 = 200;

thread_local! {
    /// The widths both pictures were decoded to at the step before, and the zoom steps taken.
    static DECODED_BEFORE: Cell<([u32; 2], usize)> = const { Cell::new(([0; 2], 0)) };
}

/// The widths both pictures were decoded to, or 0 while one is still decoding.
fn decoded_widths(c: &Ctx) -> [u32; 2] {
    Large::BOTH.map(|large| large.decoded(c).unwrap_or_default().0)
}

/// Whether both pictures hold, and no longer decode, a width that `holds` accepts of the one they had before.
fn both_decoded(c: &Ctx, holds: impl Fn(u32, u32) -> bool) -> bool {
    let (before, _) = DECODED_BEFORE.with(Cell::get);
    Large::BOTH
        .iter()
        .zip(before)
        .all(|(large, before)| large.decoded(c).is_some_and(|(width, _)| holds(width, before)))
}

impl Large {
    const BOTH: [Large; 2] = [Large::File, Large::Web];

    fn what(self) -> &'static str {
        match self {
            Large::File => "a large picture",
            Large::Web => "a large picture from the web",
        }
    }

    fn alt(self) -> &'static str {
        match self {
            Large::File => "groß",
            Large::Web => "groß im Netz",
        }
    }

    fn address(self) -> String {
        match self {
            Large::File => LARGE_FILE.to_string(),
            Large::Web => format!("{}{}", super::super::mock_web::HOST, super::super::mock_web::LARGE),
        }
    }

    /// Its width at its own size, in points.
    fn natural(self) -> f64 {
        match self {
            Large::File => 3000.0,
            Large::Web => 1500.0,
        }
    }

    /// The pixels it was decoded to once it shows, if no larger decode is under way.
    fn decoded(self, c: &Ctx) -> Option<(u32, u32)> {
        let decoded = match self {
            Large::File => c.e.pictures().store().borrow().decoded(&doc_dir(c).join(LARGE_FILE)),
            Large::Web => crate::editor::remote::decoded(&self.address()),
        };
        match decoded? {
            (pixels, false) => Some(pixels),
            (_, true) => None,
        }
    }

    /// The width of its image, which says how large it shows at its own size.
    fn image_width(self, c: &Ctx) -> Option<f64> {
        match self {
            Large::File => shown_width(c, LARGE_FILE),
            Large::Web => crate::editor::remote::get(&self.address(), None)
                .flatten()
                .map(|r| r.image.size().width),
        }
    }

    /// How many pixels wide paper takes it.
    fn on_paper(self, c: &Ctx) -> Option<isize> {
        let paper = Fit::within(SHOWN_SIDE);
        let rendered = match self {
            Large::File => {
                let dir = doc_dir(c);
                c.e.pictures()
                    .store()
                    .borrow_mut()
                    .file(LARGE_FILE, Some(&dir), false, None, paper)
            }
            Large::Web => crate::editor::remote::get_for(&self.address(), None, paper).flatten(),
        };
        Some(rendered?.image.representations().firstObject()?.pixelsWide())
    }

    /// The width it shows at in the preview, found by its alt text.
    fn shown_width(self, c: &Ctx) -> Option<f64> {
        let text = storage(c.e.preview());
        let mut at = 0;
        while at < text.length() {
            let mut range = NSRange::new(0, 0);
            // SAFETY: `at` lies inside the text and `range` outlives the call.
            let value = unsafe { text.attribute_atIndex_effectiveRange(NSAttachmentAttributeName, at, &mut range) };
            if let Some(attachment) = value.and_then(|v| v.downcast::<NSTextAttachment>().ok())
                && attachment
                    .image()
                    .and_then(|image| image.accessibilityDescription())
                    .is_some_and(|alt| alt.to_string() == self.alt())
            {
                return Some(attachment.bounds().size.width);
            }
            at = (range.location + range.length).max(at + 1);
        }
        None
    }

    /// It holds the pixels it covers, in steps and below the limit, and shows at the size its document asks for, whatever
    /// it was decoded to.
    fn check(self, c: &Ctx, when: &str) {
        let pixels = self.decoded(c).unwrap_or_default();
        let (points, needed) = asked_width_now(c);
        check(
            &format!("{} decodes to the pixels it covers {when}", self.what()),
            f64::from(pixels.0) >= needed && pixels.0 < SHOWN_SIDE && pixels.0.is_multiple_of(256),
            &format!("{pixels:?} for {needed} pixels"),
        );
        let (shown, natural) = (self.shown_width(c), self.image_width(c));
        check(
            &format!("{} decoded smaller shows at its own size {when}", self.what()),
            shown.is_some_and(|w| (w - points).abs() < 0.5) && natural == Some(self.natural()),
            &format!("{shown:?} shown for {points}, its image {natural:?} wide"),
        );
    }
}

/// The width both pictures must show at, in points and in pixels of the window's screen.
fn asked_width_now(c: &Ctx) -> (f64, f64) {
    let zoom = c.e.model().read(|st| st.zoom());
    let points = (f64::from(ASKED_WIDTH) * zoom).min(c.e.panes().preview_image_width());
    (points, points * c.e.panes().backing_scale())
}

/// Pictures decode to the pixels they cover on screen, again when zooming in shows them larger, and with the most they may
/// have for paper.
fn pixel_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            let _ = std::fs::write(doc_dir(c).join(LARGE_FILE), png_of(3000, 2000));
            let pictures: String = Large::BOTH
                .iter()
                .map(|large| {
                    let (address, alt) = (large.address(), large.alt());
                    format!("<img src=\"{address}\" alt=\"{alt}\" width=\"{ASKED_WIDTH}\"> ")
                })
                .collect();
            append(&c.e, &format!("\n\n{pictures}\n"));
        }),
        when(
            "the large pictures to show",
            |c| {
                Large::BOTH
                    .iter()
                    .all(|large| large.decoded(c).is_some() && large.shown_width(c).is_some())
            },
            |c| {
                for large in Large::BOTH {
                    large.check(c, "where it shows");
                    let paper = large.on_paper(c);
                    check(
                        &format!("{} goes to paper with the most pixels it may have", large.what()),
                        paper == isize::try_from(SHOWN_SIDE).ok(),
                        &format!("{paper:?} pixels wide"),
                    );
                }
                let widths = decoded_widths(c);
                let mut steps = 0;
                while asked_width_now(c).1 <= f64::from(widths[0].max(widths[1])) && steps < 12 {
                    perform(&c.e, Action::ZoomIn.spec().selector, 0);
                    steps += 1;
                }
                DECODED_BEFORE.with(|before| before.set((widths, steps)));
            },
        ),
        when(
            "the large pictures to decode again for the zoom",
            |c| both_decoded(c, |now, before| now > before),
            |c| {
                for large in Large::BOTH {
                    large.check(c, "after zooming in");
                }
                let (_, steps) = DECODED_BEFORE.with(Cell::get);
                DECODED_BEFORE.with(|before| before.set((decoded_widths(c), steps)));
                for _ in 0..steps {
                    perform(&c.e, Action::ZoomOut.spec().selector, 0);
                }
                // As a move to another screen does, with the same pixels per point here.
                c.e.updater().render_pictures_again();
            },
        ),
        settle(|c| {
            let kept = both_decoded(c, |now, before| now == before);
            for large in Large::BOTH {
                check(
                    &format!("{} shown smaller again keeps its pixels", large.what()),
                    kept,
                    &format!("{:?} for {:?}", decoded_widths(c), DECODED_BEFORE.with(Cell::get).0),
                );
            }
        }),
    ]
}

pub(super) fn image_file_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            let _ = std::fs::write(doc_dir(c).join("bild.png"), png_of_side(7));
        }),
        when(
            "the replaced image to show",
            |c| shown_width(c, "bild.png") == Some(7.0),
            |c| {
                let width = shown_width(c, "bild.png");
                check(
                    "an image replaced on disk shows without reopening",
                    width == Some(7.0),
                    &format!("{width:?}"),
                );
                // Paper does not wait for a load under way.
                let _ = std::fs::write(doc_dir(c).join("druck.png"), png_of_side(3));
                let dir = doc_dir(c);
                let mut images = c.e.pictures().store().borrow_mut();
                let paper = Fit::within(SHOWN_SIDE);
                let loading = images.file("druck.png", Some(&dir), false, Some(&c.e.viewer()), paper);
                let printed = images.file("druck.png", Some(&dir), false, None, paper);
                check(
                    "printing does not leave out an image that is still loading",
                    loading.is_none() && printed.is_some(),
                    &format!("preview {}, paper {}", loading.is_some(), printed.is_some()),
                );
            },
        ),
        applied(|c| {
            remember(c);
            append(&c.e, "\n\n![spät](spaeter.png)\n");
        }),
        settle(|c| {
            check(
                "a missing image counts as failed",
                c.e.pictures().store().borrow().shown(&doc_dir(c).join("spaeter.png")) == Some(false),
                "",
            );
            let _ = std::fs::write(doc_dir(c).join("spaeter.png"), png_of_side(5));
        }),
        when(
            "the late image to show",
            |c| shown_width(c, "spaeter.png") == Some(5.0),
            |c| {
                let width = shown_width(c, "spaeter.png");
                check(
                    "an image that appears later shows",
                    width == Some(5.0),
                    &format!("{width:?}"),
                );
                let _ = std::fs::write(doc_dir(c).join("bild.png"), png_of_side(2));
            },
        ),
        undone("the image line is undone in one step".into()),
        when(
            "the image of the next checks to show",
            |c| shown_width(c, "bild.png") == Some(2.0),
            |_| {},
        ),
    ]
    .into_iter()
    .chain(pixel_steps())
    .collect()
}

pub(super) fn memory_steps() -> Vec<Step> {
    vec![applied(|c| {
        splat_render::images::memory_pressure_changed(true);
        check(
            "while memory is short, images decode smaller",
            crate::editor::picture::allowed_side() == 640,
            "",
        );
        let small = crate::editor::decoder::decode(&png_of_side(700), crate::editor::picture::allowed_side());
        check(
            "the helpers start again after they were ended for memory",
            matches!(small, Some(crate::editor::decoder::Decoded::Rgba { width: 640, .. })),
            "",
        );
        splat_render::images::memory_pressure_changed(false);
        check(
            "once memory is back, images decode at full size again",
            crate::editor::picture::allowed_side() == crate::editor::decoder::SHOWN_SIDE,
            "",
        );
        check("the shown image survives", shown_width(c, "bild.png").is_some(), "");
    })]
}

pub(super) fn old_encoding_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            let path = doc_dir(c).join("alt.md");
            let mut utf16 = vec![0xFF, 0xFE];
            utf16.extend("# Grüße\n".encode_utf16().flat_map(u16::to_le_bytes));
            let _ = std::fs::write(&path, utf16);
            let done = RcBlock::new(
                |_: *mut objc2_app_kit::NSDocument, _: objc2::runtime::Bool, _: *mut objc2_foundation::NSError| {},
            );
            NSDocumentController::sharedDocumentController(c.e.mtm())
                .openDocumentWithContentsOfURL_display_completionHandler(
                    &NSURL::fileURLWithPath(&ns(path.to_str().unwrap_or_default())),
                    true,
                    &done,
                );
        }),
        when(
            "the file in UTF-16 to open",
            |c| {
                documents(c.e.mtm())
                    .iter()
                    .any(|d| d.displayName().to_string().starts_with("alt"))
            },
            |c| {
                let other = documents(c.e.mtm())
                    .into_iter()
                    .find(|d| d.displayName().to_string().starts_with("alt"));
                check(
                    "a file in UTF-16 opens with its umlauts",
                    other.as_ref().is_some_and(|d| d.text() == "# Grüße\n"),
                    "",
                );
                if let Some(d) = other {
                    d.close();
                }
                if let Some(window) = c.e.window() {
                    window.makeKeyAndOrderFront(None);
                }
            },
        ),
    ]
}
