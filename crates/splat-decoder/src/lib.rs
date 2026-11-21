//! Images, SVGs and rendered formulas are decoded in a service of their own that may neither read the user's files nor
//! use the network: a flaw in a decoder that a crafted document triggers then stays inside a process that can only hand
//! back pixels. macOS starts the service from the app bundle (`Contents/XPCServices/`, identifier: the app's with
//! `.decoder`) in a sandbox of its own; the app hands it a socket for each helper it wants, and turns no untrusted bytes
//! into a picture itself. What only the system's decoders read, TIFF, HEIC, AVIF and PDF, goes over sockets of their
//! own. The service's program calls `serve` and `xpc`; the app calls the functions of this module.

#[cfg(feature = "selftest")]
use std::io::Read;
use std::io::{BufReader, BufWriter, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::{NSBitmapFormat, NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage};
use objc2_foundation::{NSBundle, NSSize};

pub mod protocol;
mod safe_image;
pub mod serve;
pub mod xpc;

pub use protocol::{Decoded, EXPORT_SIDE, Fit, MAX_BYTES, MAX_FILE_BYTES, SHOWN_SIDE};
use protocol::{Request, TIMEOUT, memory_of, most_memory, read_answer};

use xpc::{Connection, Order};

/// What a decode can take in the worst case: both tries reach their limit.
#[cfg(feature = "selftest")]
pub const LONGEST_WAIT: Duration = Duration::from_secs(21);

/// Where what fails out of the user's sight goes: what failed and a detail, never any text of a document.
pub type Report = fn(&str, &dyn std::fmt::Display);

static REPORT: std::sync::OnceLock<Report> = std::sync::OnceLock::new();

/// Sends what fails here, and in the crates built on this one, to `report`; before that it goes to standard error.
pub fn report_to(report: Report) {
    let _ = REPORT.set(report);
}

/// Reports `what` failed, with `detail`.
pub fn report(what: &str, detail: impl std::fmt::Display) {
    match REPORT.get() {
        Some(report) => report(what, &detail),
        None => eprintln!("{what}: {detail}"),
    }
}

/// The whole program of a service that measures nothing: launchd starts it and it answers each socket the app hands
/// over; with `--stdio`, or `--stdio-native` for the system's decoders, it answers on its standard streams, as tests run it.
pub fn service_main() {
    match std::env::args().nth(1).as_deref() {
        Some("--stdio") => serve::answer(std::io::stdin().lock(), std::io::stdout().lock(), false),
        Some("--stdio-native") => serve::answer(std::io::stdin().lock(), std::io::stdout().lock(), true),
        _ => {
            serve::watch(|code| std::process::exit(code));
            xpc::serve(answer_socket, |code| std::process::exit(code));
        }
    }
}

fn answer_socket(stream: std::os::fd::OwnedFd, native: bool) {
    let stream = UnixStream::from(stream);
    if let Ok(output) = stream.try_clone() {
        serve::answer(stream, output, native);
    }
}

/// The connection to the service, made on first use; the service itself starts with the first socket it gets.
static SERVICE: Mutex<Option<Connection>> = Mutex::new(None);

/// After this long without a request the service ends, and with it the memory its fonts and decoders hold.
const SERVICE_IDLE: Duration = Duration::from_secs(60);

/// When a request last began or ended, noted while its helper's slot is held.
static LAST_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);
/// Wakes the watch of the service before its time, as a check that shortens the time does.
static WAKE: Condvar = Condvar::new();
/// The number of the connection made last; the watch of an earlier one leaves it to its own.
static CONNECTIONS: AtomicU64 = AtomicU64::new(0);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The app's identifier with `.decoder`, so that the self-test and debug builds each reach their own service.
fn service_name() -> Option<String> {
    Some(format!("{}.decoder", NSBundle::mainBundle().bundleIdentifier()?))
}

fn with_service<T>(f: impl FnOnce(&Connection) -> Option<T>) -> Option<T> {
    let mut service = lock(&SERVICE);
    if service.is_none() {
        *service = Connection::to_service(&service_name()?);
        if service.is_some() {
            used();
            watch_idle(CONNECTIONS.fetch_add(1, Ordering::SeqCst) + 1);
        }
    }
    f(service.as_ref()?)
}

fn used() {
    *lock(&LAST_REQUEST) = Some(Instant::now());
}

fn quiet_for(last: Option<Instant>) -> Duration {
    last.map_or(Duration::ZERO, |at| at.elapsed())
}

#[cfg(feature = "selftest")]
static IDLE_FOR_CHECKS: Mutex<Option<Duration>> = Mutex::new(None);

fn idle_time() -> Duration {
    #[cfg(feature = "selftest")]
    if let Some(time) = *lock(&IDLE_FOR_CHECKS) {
        return time;
    }
    SERVICE_IDLE
}

/// Ends the service after `time` without a request instead of `SERVICE_IDLE`, or after that again with `None`.
#[cfg(feature = "selftest")]
pub fn end_unused_after(time: Option<Duration>) {
    // Held, so that a watch about to wait does so for the new time.
    let _last = lock(&LAST_REQUEST);
    *lock(&IDLE_FOR_CHECKS) = time;
    WAKE.notify_all();
}

/// Whether the app holds a connection to the service, which it lets go when the service ends.
#[cfg(feature = "selftest")]
pub fn service_runs() -> bool {
    lock(&SERVICE).is_some()
}

/// What a look of the watch of the service found.
enum Look {
    /// The service ended, or a newer connection has a watch of its own.
    Done,
    /// A request runs; it notes its end, from which the time counts anew.
    Busy,
    /// A request ran since the time was read.
    Early,
}

/// Ends the service of connection `number` after `idle_time()` without a request, on a thread that ends with it.
fn watch_idle(number: u64) {
    let watch = move || {
        loop {
            {
                let last = lock(&LAST_REQUEST);
                let left = idle_time().saturating_sub(quiet_for(*last));
                if !left.is_zero() {
                    drop(WAKE.wait_timeout(last, left));
                    continue;
                }
            }
            match end_if_unused(number) {
                Look::Done => return,
                Look::Busy => drop(WAKE.wait_timeout(lock(&LAST_REQUEST), idle_time())),
                Look::Early => {}
            }
        }
    };
    let _ = std::thread::Builder::new()
        .name("decoder service watch".into())
        .spawn(watch);
}

/// The slot, unless a request holds it.
fn hold_if_free(slot: &'static Mutex<Option<Helper>>) -> Option<MutexGuard<'static, Option<Helper>>> {
    match slot.try_lock() {
        Ok(free) => Some(free),
        Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

/// Ends the service of connection `number` unless a request runs or ran within `idle_time()`. Every helper's slot stays
/// held until the service has gone, so a request that starts meanwhile waits and then starts a new one.
fn end_if_unused(number: u64) -> Look {
    let Some(mut slots) = HELPERS
        .iter()
        .chain(&NATIVE_HELPERS)
        .map(hold_if_free)
        .collect::<Option<Vec<_>>>()
    else {
        return Look::Busy;
    };
    if quiet_for(*lock(&LAST_REQUEST)) < idle_time() {
        return Look::Early;
    }
    let connection = {
        let mut service = match SERVICE.try_lock() {
            Ok(service) => service,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return Look::Busy,
        };
        if CONNECTIONS.load(Ordering::SeqCst) != number {
            return Look::Done;
        }
        let Some(connection) = service.take() else {
            return Look::Done;
        };
        connection
    };
    let mut pids: Vec<i32> = slots
        .iter_mut()
        .filter_map(|slot| slot.take().map(|helper| helper.pid))
        .collect();
    pids.sort_unstable();
    pids.dedup();
    connection.tell(Order::End);
    for pid in pids {
        wait_for_end(pid);
    }
    drop(connection);
    Look::Done
}

/// Waits a moment for process `pid` to go, so that the request after it reaches a new service, not the one ending.
fn wait_for_end(pid: i32) {
    let asked = Instant::now();
    while memory_of(pid).is_some() && asked.elapsed() < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_millis(5));
    }
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

    fn exchange(&mut self, kind: Request, fit: Option<Fit>, payload: &[u8]) -> std::io::Result<Option<Decoded>> {
        let prefix = fit.map(Fit::to_bytes);
        let prefix = prefix.as_ref().map_or(&[][..], |p| &p[..]);
        self.input.write_all(&[u8::from(kind)])?;
        self.input
            .write_all(&((prefix.len() + payload.len()) as u64).to_le_bytes())?;
        self.input.write_all(prefix)?;
        self.input.write_all(payload)?;
        self.input.flush()?;
        read_answer(&mut self.output, kind, fit.map(|fit| fit.side))
    }
}

/// A few helpers, so a document full of images or formulas decodes on several cores.
pub const HELPER_COUNT: usize = 3;
static HELPERS: [Mutex<Option<Helper>>; HELPER_COUNT] = [const { Mutex::new(None) }; HELPER_COUNT];
static NATIVE_HELPERS: [Mutex<Option<Helper>>; HELPER_COUNT] = [const { Mutex::new(None) }; HELPER_COUNT];
static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A free helper if there is one, so that an image that takes long holds up nothing but itself.
fn free_helper(pool: &'static [Mutex<Option<Helper>>]) -> MutexGuard<'static, Option<Helper>> {
    let first = NEXT.fetch_add(1, Ordering::Relaxed);
    for n in 0..pool.len() {
        if let Ok(free) = pool[(first + n) % pool.len()].try_lock() {
            return free;
        }
    }
    lock(&pool[first % pool.len()])
}

/// Ends the helpers that are not decoding, and the service when none is; the next request starts them again. The slots
/// stay held until the service has been told, so a request that starts meanwhile finds neither gone under it.
pub fn release_idle() {
    let mut held = Vec::new();
    let mut all_idle = true;
    for slot in HELPERS.iter().chain(&NATIVE_HELPERS) {
        match hold_if_free(slot) {
            Some(mut idle) => {
                *idle = None;
                held.push(idle);
            }
            None => all_idle = false,
        }
    }
    // Its fonts and decoders stay in memory for as long as it runs.
    if all_idle
        && let Ok(mut service) = SERVICE.try_lock()
        && let Some(service) = service.take()
    {
        service.tell(Order::End);
    }
    drop(held);
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

fn asked(kind: Request, fit: Option<Fit>, payload: &[u8]) -> Result<Option<Decoded>, HelperFailure> {
    if payload.len() > kind.most_bytes() {
        return Err(HelperFailure::TooLarge);
    }
    let pool: &'static [Mutex<Option<Helper>>] = if kind.is_native() { &NATIVE_HELPERS } else { &HELPERS };
    let mut guard = free_helper(pool);
    used();
    let answer = answer_in(&mut guard, kind, fit, payload);
    used();
    answer
}

/// What the helper in `slot` answers, started if there is none.
fn answer_in(
    slot: &mut Option<Helper>,
    kind: Request,
    fit: Option<Fit>,
    payload: &[u8],
) -> Result<Option<Decoded>, HelperFailure> {
    // A helper can die of what the image before did to it, so a failure gets one more try with a new one; a file that
    // ends every helper or takes too long then fails on its own.
    for _ in 0..2 {
        if slot.is_none() {
            *slot = Helper::start(kind.is_native());
        }
        let helper = slot.as_mut().ok_or(HelperFailure::NotStarted)?;
        let pid = helper.pid;
        let (done, finished) = std::sync::mpsc::channel::<()>();
        let watch = std::thread::spawn(move || {
            let overrun = overrun(&finished, TIMEOUT, most_memory(), || memory_of(pid));
            if overrun.is_some() {
                tell_service(Order::End);
            }
            overrun
        });
        let answer = helper.exchange(kind, fit, payload);
        let _ = done.send(());
        let overrun = watch.join().ok().flatten();
        match answer {
            Ok(decoded) => return Ok(decoded),
            Err(_) => {
                *slot = None;
                if let Some(overrun) = overrun {
                    return Err(HelperFailure::Stopped(overrun));
                }
            }
        }
    }
    Err(HelperFailure::Ended)
}

/// What a helper answers; why there is none goes to the log, as a picture that does not show says nothing about that.
fn request(kind: Request, fit: Option<Fit>, payload: &[u8]) -> Option<Decoded> {
    asked(kind, fit, payload).unwrap_or_else(|failure| {
        report("a decoder helper gave no answer", format!("{failure:?}"));
        None
    })
}

/// An image file or download as pixels no wider or higher than `side`.
pub fn decode(bytes: &[u8], side: u32) -> Option<Decoded> {
    decode_to(bytes, Fit::within(side))
}

/// An image file or download as the pixels `fit` asks for: PNG, JPEG, GIF, WebP and BMP from one helper, an SVG as a PDF
/// from there and as pixels from the other, so that this process reads no PDF.
pub fn decode_to(bytes: &[u8], fit: Fit) -> Option<Decoded> {
    match request(Request::Image, Some(fit), bytes)? {
        Decoded::Pdf(pdf) => decode_native_to(&pdf, fit),
        Decoded::Png(_) => None,
        pixels => Some(pixels),
    }
}

/// What only the system's decoders read, as pixels no wider or higher than `side`.
pub fn decode_native(bytes: &[u8], side: u32) -> Option<Decoded> {
    decode_native_to(bytes, Fit::within(side))
}

/// What only the system's decoders read, as the pixels `fit` asks for.
pub fn decode_native_to(bytes: &[u8], fit: Fit) -> Option<Decoded> {
    request(Request::NativeImage, Some(fit), bytes).filter(|answer| matches!(answer, Decoded::Rgba { .. }))
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
                isize::try_from(width).unwrap_or(isize::MAX),
                isize::try_from(height).unwrap_or(isize::MAX),
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
            if target.is_null() || pixels.len() != usize::try_from(w * h * 4).unwrap_or(0) {
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

    #[test]
    fn a_request_over_its_limit_reaches_no_helper() {
        let too_large = vec![0; MAX_FILE_BYTES + 1];
        assert_eq!(
            asked(Request::Image, Some(Fit::within(1)), &too_large).err(),
            Some(HelperFailure::TooLarge)
        );
    }
}
