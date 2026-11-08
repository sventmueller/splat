//! The decoding service's side: it reads requests from a socket the app handed over and answers each with what the
//! decoders make of the bytes. It runs in a sandbox of its own, without the user's files and without the network, so a
//! crafted file that takes over a decoder gets no further than this process.

use std::io::{BufReader, BufWriter, Read, Write};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use objc2::AnyThread;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSDeviceRGBColorSpace, NSGraphicsContext, NSImage};
use objc2_foundation::{NSData, NSDictionary, NSPoint, NSRect, NSSize};
use usvg::fontdb;

use super::super::safe_image;
use super::protocol::{
    Decoded, EXPORT_SIDE, MAX_SIDE, NATURAL_SIDE, Request, TIMEOUT, memory_of, most_memory, read_request, write_answer,
};

/// A vector picture becomes this many pixels per point, enough for a Retina display at twice the size and for print.
const VECTOR_SCALE: f64 = 4.0;

/// Answers the requests that arrive on one socket until the app closes it. A `native` socket takes what only the
/// system's decoders read, the others what Rust decodes.
pub fn answer(input: impl Read, output: impl Write, native: bool) {
    let mut input = BufReader::new(input);
    let mut output = BufWriter::new(output);
    while let Ok((kind, payload)) = read_request(&mut input) {
        let busy = Busy::begin();
        let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode(kind, &payload, native)));
        drop(busy);
        let written = if kind == Request::SelfCheck {
            output.write_all(&[u8::from(Request::SelfCheck), u8::from(escapes(&payload))])
        } else {
            write_answer(&mut output, answer.ok().flatten())
        };
        if written.and_then(|()| output.flush()).is_err() {
            return;
        }
    }
}

fn decode(kind: Request, payload: &[u8], native: bool) -> Option<Decoded> {
    let side = |payload: &[u8]| {
        payload
            .split_first_chunk::<4>()
            .map(|(side, rest)| (u32::from_le_bytes(*side).clamp(1, EXPORT_SIDE), rest.to_vec()))
    };
    match (native, kind) {
        (false, Request::Image) => side(payload).and_then(|(side, image)| safe_image::decode(&image, fonts(), side)),
        (false, Request::FormulaSvg) => std::str::from_utf8(payload)
            .ok()
            .and_then(|svg| safe_image::formula_to_pdf(svg, fonts()))
            .map(Decoded::Pdf),
        (true, Request::NativeImage) => side(payload).and_then(|(side, image)| native_pixels(&image, side)),
        (true, Request::PastedTiff) => png_of_tiff(payload).map(Decoded::Png),
        _ => None,
    }
}

/// The fonts of the system for text in SVGs and formulas, found once.
fn fonts() -> Arc<fontdb::Database> {
    static FONTS: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            // LastResort claims every character with a placeholder, so the search for a font would stop there before emoji.
            let placeholders: Vec<_> = db
                .faces()
                .filter(|f| f.post_script_name == "LastResort")
                .map(|f| f.id)
                .collect();
            for id in placeholders {
                db.remove_face(id);
            }
            Arc::new(db)
        })
        .clone()
}

/// The requests being decoded, by when each began: one that takes too long ends the whole service, since a decoder
/// stuck in a loop cannot be stopped on its own.
static BUSY: Mutex<Vec<(u64, Instant)>> = Mutex::new(Vec::new());

struct Busy(u64);

impl Busy {
    fn begin() -> Busy {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut busy) = BUSY.lock() {
            busy.push((id, Instant::now()));
        }
        Busy(id)
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        if let Ok(mut busy) = BUSY.lock() {
            busy.retain(|(id, _)| *id != self.0);
        }
    }
}

/// Ends the service through `end` once a request takes longer than `TIMEOUT` or the service holds more memory than it
/// may; the app sees its sockets close and tries once more with a new one.
pub fn watch(end: fn(i32)) {
    std::thread::spawn(move || {
        let pid = i32::try_from(std::process::id()).unwrap_or(0);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let late = BUSY
                .lock()
                .is_ok_and(|busy| busy.iter().any(|(_, began)| began.elapsed() > TIMEOUT));
            if late || memory_of(pid).is_some_and(|now| now > most_memory()) {
                end(3);
            }
        }
    });
}

/// How many pixels a picture becomes: as many as it has, or for a vector picture what looks sharp, and no more than
/// `side` on its longer side. `None` for a size no decoder should be asked to allocate.
fn raster_size(pixels: (isize, isize), points: (f64, f64), side: u32) -> Option<(u32, u32)> {
    let (w, h) = if pixels.0 > 0 && pixels.1 > 0 {
        (pixels.0 as f64, pixels.1 as f64)
    } else {
        (points.0 * VECTOR_SCALE, points.1 * VECTOR_SCALE)
    };
    if !(w.is_finite() && h.is_finite()) || w <= 0.0 || h <= 0.0 || w.max(h) > f64::from(MAX_SIDE) {
        return None;
    }
    let scale = (f64::from(side) / w.max(h)).min(1.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "between 1 and `side`"
    )]
    let fit = |v: f64| ((v * scale).round() as u32).clamp(1, side);
    Some((fit(w), fit(h)))
}

/// A TIFF as a PNG at its full size and with its color profile, which pixels drawn for the preview would lose.
fn png_of_tiff(bytes: &[u8]) -> Option<Vec<u8>> {
    autoreleasepool(|_| {
        let rep = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(bytes))?;
        // SAFETY: An empty property dictionary, which every file type accepts.
        let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
        Some(png.to_vec())
    })
}

/// What the system's decoders make of `bytes`, drawn into pixels with straight alpha.
fn native_pixels(bytes: &[u8], side: u32) -> Option<Decoded> {
    autoreleasepool(|_| {
        let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))?;
        let size = image.size();
        let representation = image.representations().firstObject()?;
        let (width, height) = raster_size(
            (representation.pixelsWide(), representation.pixelsHigh()),
            (size.width, size.height),
            side,
        )?;
        let (w, h) = (isize::try_from(width).ok()?, isize::try_from(height).ok()?);
        // SAFETY: without plane pointers the image rep allocates its own buffer of the given size.
        let rep = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                w,
                h,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                w * 4,
                32,
            )
        }?;
        let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&context));
        image.drawInRect(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(f64::from(width), f64::from(height)),
        ));
        context.flushGraphics();
        NSGraphicsContext::restoreGraphicsState_class();
        let target = rep.bitmapData();
        if target.is_null() {
            return None;
        }
        let len = width as usize * height as usize * 4;
        // SAFETY: the buffer holds width × height × 4 bytes and lives as long as `rep`.
        let mut pixels = unsafe { std::slice::from_raw_parts(target, len) }.to_vec();
        straighten_alpha(&mut pixels);
        Some(Decoded::Rgba {
            width,
            height,
            pixels,
            natural: (whole_points(size.width), whole_points(size.height)),
        })
    })
}

fn whole_points(points: f64) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "within the range"
    )]
    let whole = points.round().clamp(1.0, f64::from(NATURAL_SIDE)) as u32;
    whole
}

/// A bitmap context keeps its colours multiplied by their alpha, and what leaves the service has them apart.
fn straighten_alpha(pixels: &mut [u8]) {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        if alpha != 0 && alpha != 255 {
            for channel in &mut pixel[..3] {
                *channel = u8::try_from((u32::from(*channel) * 255 + alpha / 2) / alpha).unwrap_or(255);
            }
        }
    }
}

/// Whether the service can read the file at `path` or open a connection, which must both be impossible. Only a refusal by
/// the sandbox counts: a closed port also fails, but with another error.
fn escapes(path: &[u8]) -> bool {
    let path = String::from_utf8_lossy(path);
    let file = std::fs::read(path.as_ref()).is_ok();
    let network = std::net::TcpStream::connect(("127.0.0.1", 9))
        .map_or_else(|e| e.kind() != std::io::ErrorKind::PermissionDenied, |_| true);
    file || network
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_keeps_its_pixels_and_a_vector_one_gets_four_per_point_within_the_limit() {
        assert_eq!(raster_size((40, 20), (40.0, 20.0), 2560), Some((40, 20)));
        assert_eq!(raster_size((4000, 2000), (4000.0, 2000.0), 2000), Some((2000, 1000)));
        assert_eq!(raster_size((0, 0), (200.0, 40.0), 2560), Some((800, 160)));
        assert_eq!(raster_size((0, 0), (2000.0, 400.0), 2560), Some((2560, 512)));
        assert_eq!(
            raster_size((3000, 1), (3000.0, 1.0), 300),
            Some((300, 1)),
            "never less than a pixel"
        );
        assert_eq!(
            raster_size((1 << 20, 10), (1.0, 1.0), 2560),
            None,
            "too large to ask for"
        );
        assert_eq!(raster_size((0, 0), (0.0, 0.0), 2560), None);
        assert_eq!(raster_size((0, 0), (f64::NAN, 1.0), 2560), None);
    }

    #[test]
    fn colours_come_apart_from_their_alpha() {
        let mut pixels = vec![128, 64, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0, 255, 255, 255, 128];
        straighten_alpha(&mut pixels);
        assert_eq!(
            pixels,
            [255, 128, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0, 255, 255, 255, 128]
        );
    }

    #[test]
    fn a_size_in_points_is_whole_and_at_least_one() {
        assert_eq!(whole_points(39.6), 40);
        assert_eq!(whole_points(0.2), 1);
        assert_eq!(whole_points(1e12), NATURAL_SIDE);
    }
}
