//! Images, SVGs and rendered formulas are decoded in a service of their own that may neither read the user's files nor
//! use the network: a flaw in a decoder that a crafted document triggers then stays inside a process that can only hand
//! back pixels. macOS starts the service, `SplatDecoder.xpc` in the app bundle, in a sandbox of its own; the app hands it
//! a socket for each helper it wants, and turns no untrusted bytes into a picture itself. What only the system's decoders
//! read, TIFF, HEIC, AVIF and PDF, goes over sockets of their own.

#[cfg(feature = "selftest")]
use std::io::Read;
use std::io::{BufReader, BufWriter, Write};
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::time::Duration;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::{NSBitmapFormat, NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage};
use objc2_foundation::{NSBundle, NSSize};

#[allow(
    dead_code,
    reason = "the app reads the answers and the service the requests; each uses its half"
)]
pub(crate) mod protocol;

pub use protocol::{Decoded, EXPORT_SIDE, MAX_BYTES, MAX_FILE_BYTES, SHOWN_SIDE};
use protocol::{Request, TIMEOUT, memory_of, most_memory, read_answer};

use crate::util::xpc::{Connection, Order};

/// What a decode can take in the worst case: both tries reach their limit.
#[cfg(feature = "selftest")]
pub const LONGEST_WAIT: Duration = Duration::from_secs(21);

/// The connection to the service, made on first use; the service itself starts with the first socket it gets.
static SERVICE: Mutex<Option<Connection>> = Mutex::new(None);

/// The app's identifier with `.decoder`, so that the self-test and debug builds each reach their own service.
fn service_name() -> Option<String> {
    Some(format!("{}.decoder", NSBundle::mainBundle().bundleIdentifier()?))
}

fn with_service<T>(f: impl FnOnce(&Connection) -> Option<T>) -> Option<T> {
    let mut service = SERVICE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if service.is_none() {
        *service = Connection::to_service(&service_name()?);
    }
    f(service.as_ref()?)
}

fn tell_service(order: Order) {
    if let Ok(service) = SERVICE.lock()
        && let Some(service) = service.as_ref()
    {
        service.tell(order);
    }
}

/// One socket to the service, which answers one request on it at a time.
struct Helper {
    pid: i32,
    input: BufWriter<UnixStream>,
    output: BufReader<UnixStream>,
}

impl Helper {
    fn start(native: bool) -> Option<Helper> {
        let (ours, theirs) = UnixStream::pair().ok()?;
        // A service that was taken over may stop answering; reading gives up a little after it should have ended itself.
        ours.set_read_timeout(Some(TIMEOUT + Duration::from_secs(1))).ok()?;
        let pid = with_service(|service| service.hand_over(theirs.into(), native))?;
        Some(Helper {
            pid,
            input: BufWriter::new(ours.try_clone().ok()?),
            output: BufReader::new(ours),
        })
    }

    fn exchange(&mut self, kind: Request, side: Option<u32>, payload: &[u8]) -> std::io::Result<Option<Decoded>> {
        let prefix = side.map(u32::to_le_bytes);
        let prefix = prefix.as_ref().map_or(&[][..], |p| &p[..]);
        self.input.write_all(&[u8::from(kind)])?;
        self.input
            .write_all(&((prefix.len() + payload.len()) as u64).to_le_bytes())?;
        self.input.write_all(prefix)?;
        self.input.write_all(payload)?;
        self.input.flush()?;
        read_answer(&mut self.output, kind, side)
    }
}

/// A few helpers, so a document full of images or formulas decodes on several cores.
pub const HELPER_COUNT: usize = 3;
static HELPERS: [Mutex<Option<Helper>>; HELPER_COUNT] = [const { Mutex::new(None) }; HELPER_COUNT];
static NATIVE_HELPERS: [Mutex<Option<Helper>>; HELPER_COUNT] = [const { Mutex::new(None) }; HELPER_COUNT];
static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A free helper if there is one, so that an image that takes long holds up nothing but itself.
fn free_helper(pool: &'static [Mutex<Option<Helper>>]) -> std::sync::MutexGuard<'static, Option<Helper>> {
    let first = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    for n in 0..pool.len() {
        if let Ok(free) = pool[(first + n) % pool.len()].try_lock() {
            return free;
        }
    }
    pool[first % pool.len()]
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Ends the helpers that are not decoding, and the service when none is; the next request starts them again.
pub fn release_idle() {
    let mut all_idle = true;
    for slot in HELPERS.iter().chain(&NATIVE_HELPERS) {
        match slot.try_lock() {
            Ok(mut idle) => *idle = None,
            Err(_) => all_idle = false,
        }
    }
    // Its fonts and decoders stay in memory for as long as it runs.
    if all_idle
        && let Ok(mut service) = SERVICE.try_lock()
        && let Some(service) = service.take()
    {
        service.tell(Order::End);
    }
}

/// Why a helper was stopped during a request.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Overrun {
    Time,
    Memory,
}

/// Waits for the end of a request and says what a helper went beyond, if anything. A limit from the outside: macOS refuses
/// `setrlimit` for memory, and in a process that can be taken over nothing inside it counts.
fn overrun(
    finished: &std::sync::mpsc::Receiver<()>,
    time: Duration,
    memory: u64,
    memory_now: impl Fn() -> Option<u64>,
) -> Option<Overrun> {
    const LOOK_EVERY: Duration = Duration::from_millis(50);
    let start = std::time::Instant::now();
    loop {
        let Some(left) = time.checked_sub(start.elapsed()) else {
            return Some(Overrun::Time);
        };
        match finished.recv_timeout(left.min(LOOK_EVERY)) {
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            _ => return None,
        }
        if memory_now().is_some_and(|now| now > memory) {
            return Some(Overrun::Memory);
        }
    }
}

/// Why a request came back without an answer of its helper.
#[derive(Clone, Copy, PartialEq, Debug)]
enum HelperFailure {
    TooLarge,
    NotStarted,
    Stopped(Overrun),
    /// Twice, so that the file ends every helper.
    Ended,
}

fn asked(kind: Request, side: Option<u32>, payload: &[u8]) -> Result<Option<Decoded>, HelperFailure> {
    if payload.len() > kind.most_bytes() {
        return Err(HelperFailure::TooLarge);
    }
    let pool: &'static [Mutex<Option<Helper>>] = if kind.is_native() { &NATIVE_HELPERS } else { &HELPERS };
    let mut guard = free_helper(pool);
    // A helper can die of what the image before did to it, so a failure gets one more try with a new one; a file that
    // ends every helper or takes too long then fails on its own.
    for _ in 0..2 {
        if guard.is_none() {
            *guard = Helper::start(kind.is_native());
        }
        let helper = guard.as_mut().ok_or(HelperFailure::NotStarted)?;
        let pid = helper.pid;
        let (done, finished) = std::sync::mpsc::channel::<()>();
        let watch = std::thread::spawn(move || {
            let overrun = overrun(&finished, TIMEOUT, most_memory(), || memory_of(pid));
            if overrun.is_some() {
                tell_service(Order::End);
            }
            overrun
        });
        let answer = helper.exchange(kind, side, payload);
        let _ = done.send(());
        let overrun = watch.join().ok().flatten();
        match answer {
            Ok(decoded) => return Ok(decoded),
            Err(_) => {
                *guard = None;
                if let Some(overrun) = overrun {
                    return Err(HelperFailure::Stopped(overrun));
                }
            }
        }
    }
    Err(HelperFailure::Ended)
}

/// What a helper answers; why there is none goes to the log, as a picture that does not show says nothing about that.
fn request(kind: Request, side: Option<u32>, payload: &[u8]) -> Option<Decoded> {
    asked(kind, side, payload).unwrap_or_else(|failure| {
        crate::diag::report("a decoder helper gave no answer", format!("{failure:?}"));
        None
    })
}

/// An image file or download as pixels no wider or higher than `side`: PNG, JPEG, GIF, WebP and BMP from one helper, an
/// SVG as a PDF from there and as pixels from the other, so that this process reads no PDF.
pub fn decode(bytes: &[u8], side: u32) -> Option<Decoded> {
    match request(Request::Image, Some(side), bytes)? {
        Decoded::Pdf(pdf) => decode_native(&pdf, side),
        Decoded::Png(_) => None,
        pixels => Some(pixels),
    }
}

/// What only the system's decoders read, as pixels no wider or higher than `side`.
pub fn decode_native(bytes: &[u8], side: u32) -> Option<Decoded> {
    request(Request::NativeImage, Some(side), bytes).filter(|answer| matches!(answer, Decoded::Rgba { .. }))
}

/// A TIFF from the pasteboard as a PNG file.
pub fn png_of_pasted_tiff(tiff: &[u8]) -> Option<Vec<u8>> {
    match request(Request::PastedTiff, None, tiff)? {
        Decoded::Png(png) => Some(png),
        _ => None,
    }
}

/// A diagram as WebKit drew it.
pub fn pdf_to_pixels(pdf: &[u8], side: u32) -> Option<Decoded> {
    decode_native(pdf, side)
}

/// A formula as rendered by WebKit: MathJax draws the glyphs of its fonts as paths and leaves other characters as text.
pub fn svg_to_pdf(svg: &str) -> Option<Vec<u8>> {
    match request(Request::FormulaSvg, None, svg.as_bytes())? {
        Decoded::Pdf(pdf) => Some(pdf),
        Decoded::Png(_) | Decoded::Rgba { .. } => None,
    }
}

/// A formula as pixels.
pub fn svg_to_pixels(svg: &str, side: u32) -> Option<Decoded> {
    pdf_to_pixels(&svg_to_pdf(svg)?, side)
}

/// Asks the service to read `path` and to open a network connection; `Some(false)` when both were refused.
#[cfg(feature = "selftest")]
pub fn self_check(native: bool, path: &str) -> Option<bool> {
    let mut helper = Helper::start(native)?;
    helper.input.write_all(&[u8::from(Request::SelfCheck)]).ok()?;
    helper.input.write_all(&(path.len() as u64).to_le_bytes()).ok()?;
    helper.input.write_all(path.as_bytes()).ok()?;
    helper.input.flush().ok()?;
    let mut answer = [0u8; 2];
    helper.output.read_exact(&mut answer).ok()?;
    (Request::try_from(answer[0]) == Ok(Request::SelfCheck)).then_some(answer[1] != 0)
}

/// Ends the service as a crash would, without telling the pool.
#[cfg(feature = "selftest")]
pub fn end_helpers() {
    tell_service(Order::End);
}

/// Stops the service as a hang would: it stays around and answers nothing.
#[cfg(feature = "selftest")]
pub fn stop_helpers() {
    tell_service(Order::Stop);
}

/// An image from decoded pixels. A PDF stays out: it has to pass the other helper first.
pub fn image(decoded: Decoded) -> Option<Retained<NSImage>> {
    match decoded {
        Decoded::Pdf(_) | Decoded::Png(_) => None,
        Decoded::Rgba {
            width,
            height,
            pixels,
            natural,
        } => {
            let (w, h) = (
                crate::util::to_isize(width as usize),
                crate::util::to_isize(height as usize),
            );
            // SAFETY: without plane pointers the image rep allocates its own buffer of the given size.
            let rep = unsafe {
                NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
                    NSBitmapImageRep::alloc(),
                    std::ptr::null_mut(),
                    w,
                    h,
                    8,
                    4,
                    true,
                    false,
                    NSDeviceRGBColorSpace,
                    NSBitmapFormat::AlphaNonpremultiplied,
                    w * 4,
                    32,
                )
            }?;
            let target = rep.bitmapData();
            if target.is_null() || pixels.len() != crate::util::to_usize(w * h * 4) {
                return None;
            }
            // SAFETY: the buffer holds width × height × 4 bytes, the length of `pixels` checked above.
            unsafe { std::ptr::copy_nonoverlapping(pixels.as_ptr(), target, pixels.len()) };
            let image = NSImage::initWithSize(
                NSImage::alloc(),
                NSSize::new(f64::from(natural.0), f64::from(natural.1)),
            );
            image.addRepresentation(&rep);
            Some(image)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_helper_is_stopped_at_its_limits_and_only_there() {
        let (done, finished) = std::sync::mpsc::channel::<()>();
        let long = Duration::from_secs(60);
        assert_eq!(overrun(&finished, long, 100, || Some(101)), Some(Overrun::Memory));
        let short = Duration::from_millis(120);
        assert_eq!(overrun(&finished, short, 100, || Some(100)), Some(Overrun::Time));
        assert_eq!(
            overrun(&finished, short, 100, || None),
            Some(Overrun::Time),
            "gone already"
        );
        done.send(()).unwrap();
        assert_eq!(
            overrun(&finished, long, 100, || Some(101)),
            None,
            "a request that has ended"
        );
    }

    /// Every Rust file under `dir`.
    fn sources(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(sources(&path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
        found
    }

    #[test]
    fn only_the_service_hands_bytes_to_the_systems_decoders() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let calls = [
            "imageRepWithData",
            "imageRepsWithData",
            "initWithData(",
            "CGImageSource",
        ];
        for file in sources(&src)
            .iter()
            .filter(|f| !f.to_string_lossy().contains("selftest"))
        {
            let text = std::fs::read_to_string(file).unwrap();
            let product = text.split("#[cfg(test)]").next().unwrap_or("");
            for call in &calls {
                let allowed =
                    file.ends_with("editor/decoder/serve.rs") && matches!(*call, "initWithData(" | "imageRepWithData");
                assert!(allowed || !product.contains(call), "{} calls {call}", file.display());
            }
        }
    }

    #[test]
    fn a_request_over_its_limit_reaches_no_helper() {
        let too_large = vec![0; MAX_FILE_BYTES + 1];
        assert_eq!(
            asked(Request::Image, Some(1), &too_large).err(),
            Some(HelperFailure::TooLarge)
        );
    }
}
