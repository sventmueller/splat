//! Images, SVGs and rendered formulas are decoded in helper processes that may neither read files nor use the
//! network: a flaw in a decoder that a crafted document triggers then stays inside a process that can only hand
//! back pixels or a PDF of its own making. The helpers are this executable started with `ARG`. What is left for the
//! system's decoders in the app itself, such as those PDFs and formats only the system reads, goes through
//! `system_image`, so this file names all of it.

use std::io::{BufReader, BufWriter, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::{NSBitmapFormat, NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage};
use objc2_foundation::{NSData, NSSize};
use svg2pdf::usvg::fontdb;

use super::safe_image::{self, Decoded, EXPORT_SIDE, MAX_FILE_BYTES};

/// Command line argument that turns the process into a decoding helper.
pub const ARG: &str = "--splat-decoder";

/// A decode that takes longer ends its helper, which then counts as failed.
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PDF: usize = 128 << 20;
/// Beyond the `MAX_SIDE` of `safe_image`; the helper is trusted for no more than a sane number.
const NATURAL_SIDE: u32 = 1 << 20;

const IMAGE: u8 = 1;
const FORMULA_SVG: u8 = 2;
const SELF_CHECK: u8 = 3;

const NOTHING: u8 = 0;
const PDF: u8 = 1;
const RGBA: u8 = 2;

/// Everything the helper may do once it runs untrusted data: compute, and use the pipes it was started with.
const PROFILE: &str = "(version 1)\n(deny default)\n";

unsafe extern "C" {
    fn sandbox_init(profile: *const std::ffi::c_char, flags: u64, errorbuf: *mut *mut std::ffi::c_char) -> i32;
    fn close(fd: i32) -> i32;
    fn fcntl(fd: i32, command: i32, ...) -> i32;
}

/// Runs the helper: loads the system fonts, closes itself off and then answers requests until its input ends.
pub fn serve() -> ! {
    close_inherited_descriptors();
    #[cfg(coverage)]
    open_measurement_file();
    let fonts = mapped_system_fonts();
    let profile = std::ffi::CString::new(PROFILE).expect("profile without NUL");
    let mut error = std::ptr::null_mut();
    // SAFETY: a NUL-terminated profile, flags 0 for profile text, and a valid place for the error message.
    if unsafe { sandbox_init(profile.as_ptr(), 0, &mut error) } != 0 {
        eprintln!("Splat: the image decoder could not close itself off and does not start");
        std::process::exit(2);
    }
    let mut input = BufReader::new(std::io::stdin().lock());
    let mut output = BufWriter::new(std::io::stdout().lock());
    loop {
        let Ok((kind, payload)) = read_request(&mut input) else {
            std::process::exit(0);
        };
        let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match kind {
            IMAGE => payload.split_first_chunk::<4>().and_then(|(side, image)| {
                safe_image::decode(image, fonts.clone(), u32::from_le_bytes(*side).clamp(1, EXPORT_SIDE))
            }),
            FORMULA_SVG => std::str::from_utf8(&payload)
                .ok()
                .and_then(|svg| safe_image::formula_to_pdf(svg, fonts.clone()))
                .map(Decoded::Pdf),
            _ => None,
        }));
        let written = if kind == SELF_CHECK {
            output.write_all(&[SELF_CHECK, u8::from(escapes(&payload))])
        } else {
            write_answer(&mut output, answer.ok().flatten())
        };
        if written.and_then(|()| output.flush()).is_err() {
            std::process::exit(0);
        }
    }
}

/// Files and sockets the app had open without close-on-exec arrive here open, and the sandbox only stops opening new
/// ones; nothing but the three standard streams is open before the helper starts its work.
fn close_inherited_descriptors() {
    let inherited: Vec<i32> = std::fs::read_dir("/dev/fd")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .filter(|&fd| fd > 2)
        .collect();
    for fd in inherited {
        // SAFETY: no Rust object owns these descriptors; the one of the directory listing is already closed again.
        unsafe { close(fd) };
    }
}

/// A measured build writes its coverage when it exits, into a file it would create only then; the sandbox forbids
/// that, so the helper opens its file before.
#[cfg(coverage)]
fn open_measurement_file() {
    unsafe extern "C" {
        fn fopen(path: *const std::ffi::c_char, mode: *const std::ffi::c_char) -> *mut std::ffi::c_void;
        fn __llvm_profile_set_file_object(file: *mut std::ffi::c_void, enable_merge: i32);
    }
    let Some(pattern) = std::env::var("LLVM_PROFILE_FILE").ok() else {
        return;
    };
    let Ok(path) = std::ffi::CString::new(measurement_path(&pattern, std::process::id())) else {
        return;
    };
    // SAFETY: A NUL-terminated path and mode.
    let file = unsafe { fopen(path.as_ptr(), c"wb".as_ptr()) };
    if !file.is_null() {
        // SAFETY: An open file the profile runtime keeps and writes once, at exit.
        unsafe { __llvm_profile_set_file_object(file, 0) };
    }
}

/// `LLVM_PROFILE_FILE` with its process (`%p`) and module (`%m`, `%8m`) placeholders filled in.
#[cfg(any(coverage, test))]
fn measurement_path(pattern: &str, pid: u32) -> String {
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(at) = rest.find('%') {
        out.push_str(&rest[..at]);
        let spec = &rest[at + 1..];
        let digits = spec.bytes().take_while(u8::is_ascii_digit).count();
        match spec[digits..].chars().next() {
            Some('p') => out.push_str(&pid.to_string()),
            Some('m') => out.push_str("decoder"),
            _ => {
                out.push('%');
                rest = spec;
                continue;
            }
        }
        rest = &spec[digits + 1..];
    }
    out.push_str(rest);
    out
}

/// Reading font files is not allowed later, so they are mapped into memory first.
fn mapped_system_fonts() -> Arc<fontdb::Database> {
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
    let ids: Vec<_> = db.faces().map(|f| f.id).collect();
    for id in ids {
        // SAFETY: fontdb requires that the mapped file is not changed meanwhile; system fonts sit on the read-only
        // system volume, and a user font cut short while mapped ends only this helper, with SIGBUS.
        let _ = unsafe { db.make_shared_face_data(id) };
    }
    Arc::new(db)
}

/// Whether the helper can read the file at `path`, open a connection or still holds a file the app left open, which
/// must all be impossible. Only a refusal by the sandbox counts: a closed port also fails, but with another error.
fn escapes(path: &[u8]) -> bool {
    const F_GETFD: i32 = 1;
    let path = String::from_utf8_lossy(path);
    let file = std::fs::read(path.as_ref()).is_ok();
    let network = std::net::TcpStream::connect(("127.0.0.1", 9))
        .map_or_else(|e| e.kind() != std::io::ErrorKind::PermissionDenied, |_| true);
    // SAFETY: asking for the flags of a descriptor number has no effect, whether it is open or not.
    let inherited = !cfg!(coverage) && (3..256).any(|fd| unsafe { fcntl(fd, F_GETFD) } != -1);
    file || network || inherited
}

fn read_request(input: &mut impl Read) -> std::io::Result<(u8, Vec<u8>)> {
    let mut head = [0u8; 9];
    input.read_exact(&mut head)?;
    let len = usize::try_from(u64::from_le_bytes(head[1..].try_into().expect("8 bytes"))).unwrap_or(usize::MAX);
    if len > MAX_FILE_BYTES + 4 {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut payload = vec![0; len];
    input.read_exact(&mut payload)?;
    Ok((head[0], payload))
}

fn write_answer(output: &mut impl Write, answer: Option<Decoded>) -> std::io::Result<()> {
    match answer {
        None => output.write_all(&[NOTHING]),
        Some(Decoded::Pdf(pdf)) => {
            output.write_all(&[PDF])?;
            output.write_all(&(pdf.len() as u64).to_le_bytes())?;
            output.write_all(&pdf)
        }
        Some(Decoded::Rgba {
            width,
            height,
            pixels,
            natural,
        }) => {
            output.write_all(&[RGBA])?;
            for side in [width, height, natural.0, natural.1] {
                output.write_all(&side.to_le_bytes())?;
            }
            output.write_all(&pixels)
        }
    }
}

struct Helper {
    child: Child,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Helper {
    fn start() -> Option<Helper> {
        let mut child = Command::new(std::env::current_exe().ok()?)
            .arg(ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .ok()?;
        Some(Helper {
            input: BufWriter::new(child.stdin.take()?),
            output: BufReader::new(child.stdout.take()?),
            child,
        })
    }

    fn exchange(&mut self, kind: u8, side: Option<u32>, payload: &[u8]) -> std::io::Result<Option<Decoded>> {
        let prefix = side.map(u32::to_le_bytes);
        let prefix = prefix.as_ref().map_or(&[][..], |p| &p[..]);
        self.input.write_all(&[kind])?;
        self.input
            .write_all(&((prefix.len() + payload.len()) as u64).to_le_bytes())?;
        self.input.write_all(prefix)?;
        self.input.write_all(payload)?;
        self.input.flush()?;
        read_answer(&mut self.output, side)
    }
}

/// The answer of a helper, which may have been taken over: nothing it sends is trusted beyond these bounds.
fn read_answer(input: &mut impl Read, side: Option<u32>) -> std::io::Result<Option<Decoded>> {
    let invalid = || std::io::Error::from(std::io::ErrorKind::InvalidData);
    let u32_from = |input: &mut dyn Read| -> std::io::Result<u32> {
        let mut b = [0u8; 4];
        input.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    };
    let mut tag = [0u8; 1];
    input.read_exact(&mut tag)?;
    match tag[0] {
        NOTHING => Ok(None),
        PDF => {
            let mut len = [0u8; 8];
            input.read_exact(&mut len)?;
            let len = usize::try_from(u64::from_le_bytes(len)).map_err(|_| invalid())?;
            if len > MAX_PDF {
                return Err(invalid());
            }
            let mut pdf = vec![0; len];
            input.read_exact(&mut pdf)?;
            Ok(pdf.starts_with(b"%PDF").then_some(Decoded::Pdf(pdf)))
        }
        RGBA => {
            let width = u32_from(input)?;
            let height = u32_from(input)?;
            let natural = (u32_from(input)?, u32_from(input)?);
            if !(1..=NATURAL_SIDE).contains(&natural.0) || !(1..=NATURAL_SIDE).contains(&natural.1) {
                return Err(invalid());
            }
            let allowed = 1..=side.unwrap_or(EXPORT_SIDE);
            if !allowed.contains(&width) || !allowed.contains(&height) {
                return Err(invalid());
            }
            let mut pixels = vec![0; width as usize * height as usize * 4];
            input.read_exact(&mut pixels)?;
            Ok(Some(Decoded::Rgba {
                width,
                height,
                pixels,
                natural,
            }))
        }
        _ => Err(invalid()),
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A few helpers, so a document full of images or formulas decodes on several cores.
pub const HELPER_COUNT: usize = 3;
static HELPERS: [Mutex<Option<Helper>>; HELPER_COUNT] = [const { Mutex::new(None) }; HELPER_COUNT];
static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A free helper if there is one, so that an image that takes long holds up nothing but itself.
fn free_helper() -> std::sync::MutexGuard<'static, Option<Helper>> {
    let first = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    for n in 0..HELPERS.len() {
        if let Ok(free) = HELPERS[(first + n) % HELPERS.len()].try_lock() {
            return free;
        }
    }
    HELPERS[first % HELPERS.len()]
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Ends the helpers that are not decoding; the next request starts them again.
pub fn release_idle() {
    for slot in &HELPERS {
        if let Ok(mut idle) = slot.try_lock() {
            *idle = None;
        }
    }
}

fn request(kind: u8, side: Option<u32>, payload: &[u8]) -> Option<Decoded> {
    if payload.len() > MAX_FILE_BYTES {
        return None;
    }
    let mut guard = free_helper();
    // A helper can die of what the image before did to it, so a failure gets one more try with a new one; a file that
    // ends every helper or takes too long then fails on its own.
    for _ in 0..2 {
        if guard.is_none() {
            *guard = Helper::start();
        }
        let helper = guard.as_mut()?;
        let pid = i32::try_from(helper.child.id()).ok()?;
        let timed_out = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (done, finished) = std::sync::mpsc::channel::<()>();
        let noted = timed_out.clone();
        std::thread::spawn(move || {
            if finished.recv_timeout(TIMEOUT) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
                noted.store(true, std::sync::atomic::Ordering::SeqCst);
                crate::util::end_process(pid);
            }
        });
        let answer = helper.exchange(kind, side, payload);
        let _ = done.send(());
        match answer {
            Ok(decoded) => return decoded,
            Err(_) => {
                *guard = None;
                if timed_out.load(std::sync::atomic::Ordering::SeqCst) {
                    return None;
                }
            }
        }
    }
    None
}

/// An image file or download: PNG, JPEG, GIF, WebP and BMP become pixels no wider or higher than `side`, SVG a PDF.
pub fn decode(bytes: &[u8], side: u32) -> Option<Decoded> {
    request(IMAGE, Some(side), bytes)
}

/// A formula as rendered by WebKit: MathJax draws the glyphs of its fonts as paths and leaves other characters as text.
pub fn svg_to_pdf(svg: &str) -> Option<Vec<u8>> {
    match request(FORMULA_SVG, None, svg.as_bytes())? {
        Decoded::Pdf(pdf) => Some(pdf),
        Decoded::Rgba { .. } => None,
    }
}

/// Asks a helper to read `path` and to open a network connection; `Some(false)` when both were refused.
#[cfg(feature = "selftest")]
pub fn self_check(path: &str) -> Option<bool> {
    let mut helper = Helper::start()?;
    helper.input.write_all(&[SELF_CHECK]).ok()?;
    helper.input.write_all(&(path.len() as u64).to_le_bytes()).ok()?;
    helper.input.write_all(path.as_bytes()).ok()?;
    helper.input.flush().ok()?;
    let mut answer = [0u8; 2];
    helper.output.read_exact(&mut answer).ok()?;
    (answer[0] == SELF_CHECK).then_some(answer[1] != 0)
}

/// Bytes read by the system's decoders in the app itself, outside the sandbox of the helpers. Nothing else may hand
/// bytes to `NSImage`: formats the helpers cannot read arrive here checked by `images::native_only`, vector PDFs from the
/// helper or from WebKit by their own origin.
pub fn system_image(bytes: &[u8]) -> Option<Retained<NSImage>> {
    NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))
}

/// An image from decoded pixels or from a PDF the helper wrote.
pub fn image(decoded: Decoded) -> Option<Retained<NSImage>> {
    match decoded {
        Decoded::Pdf(pdf) => system_image(&pdf),
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

    fn rgba(width: u32, height: u32, natural: (u32, u32), pixels: usize) -> Vec<u8> {
        let mut answer = vec![RGBA];
        for value in [width, height, natural.0, natural.1] {
            answer.extend_from_slice(&value.to_le_bytes());
        }
        answer.extend(std::iter::repeat_n(0, pixels));
        answer
    }

    fn read(answer: &[u8], side: Option<u32>) -> std::io::Result<Option<Decoded>> {
        read_answer(&mut std::io::Cursor::new(answer), side)
    }

    #[test]
    fn an_honest_answer_comes_through() {
        assert!(read(&[NOTHING], None).unwrap().is_none());
        let Decoded::Rgba { width, height, .. } = read(&rgba(2, 1, (2, 1), 8), Some(10)).unwrap().unwrap() else {
            panic!()
        };
        assert_eq!((width, height), (2, 1));
        let mut pdf = vec![PDF];
        pdf.extend_from_slice(&5u64.to_le_bytes());
        pdf.extend_from_slice(b"%PDF-");
        assert!(matches!(read(&pdf, None).unwrap(), Some(Decoded::Pdf(_))));
    }

    /// A helper taken over by a crafted file must get nothing out of the app, least of all gigabytes of memory.
    #[test]
    fn a_helper_taken_over_is_believed_nothing_beyond_the_limits() {
        for (name, answer) in [
            ("wider than asked", rgba(11, 1, (11, 1), 44)),
            ("higher than asked", rgba(1, 11, (1, 11), 44)),
            ("without width", rgba(0, 1, (1, 1), 0)),
            ("without height", rgba(1, 0, (1, 1), 0)),
            ("natural size without width", rgba(1, 1, (0, 1), 4)),
            ("natural size without height", rgba(1, 1, (1, 0), 4)),
            ("natural size beyond all", rgba(1, 1, (NATURAL_SIDE + 1, 1), 4)),
            ("fewer pixels than named", rgba(2, 2, (2, 2), 4)),
            ("an unknown kind", vec![9]),
            ("nothing at all", vec![]),
        ] {
            assert!(read(&answer, Some(10)).is_err(), "{name}");
        }
        let mut huge_pdf = vec![PDF];
        huge_pdf.extend_from_slice(&(MAX_PDF as u64 + 1).to_le_bytes());
        assert!(read(&huge_pdf, None).is_err(), "a PDF over the limit is not even read");
        let mut no_pdf = vec![PDF];
        no_pdf.extend_from_slice(&3u64.to_le_bytes());
        no_pdf.extend_from_slice(b"abc");
        assert!(
            read(&no_pdf, None).unwrap().is_none(),
            "what is no PDF stays out of the system's decoders"
        );
    }

    #[test]
    fn without_a_limit_of_its_own_an_answer_stays_within_the_export_size() {
        assert!(read(&rgba(EXPORT_SIDE + 1, 1, (1, 1), 0), None).is_err());
    }

    #[test]
    fn requests_and_answers_cross_the_pipe_intact() {
        let mut wire = vec![IMAGE];
        wire.extend_from_slice(&3u64.to_le_bytes());
        wire.extend_from_slice(b"abc");
        assert_eq!(read_request(&mut wire.as_slice()).unwrap(), (IMAGE, b"abc".to_vec()));
        let mut huge = vec![IMAGE];
        huge.extend_from_slice(&(MAX_FILE_BYTES as u64 + 1).to_le_bytes());
        assert!(read_request(&mut huge.as_slice()).is_err());
        let mut out = Vec::new();
        write_answer(&mut out, None).unwrap();
        write_answer(&mut out, Some(Decoded::Pdf(b"%PDF".to_vec()))).unwrap();
        write_answer(
            &mut out,
            Some(Decoded::Rgba {
                width: 1,
                height: 1,
                pixels: vec![1, 2, 3, 4],
                natural: (9, 9),
            }),
        )
        .unwrap();
        let mut expected = vec![NOTHING, PDF];
        expected.extend_from_slice(&4u64.to_le_bytes());
        expected.extend_from_slice(b"%PDF");
        expected.push(RGBA);
        for side in [1u32, 1, 9, 9] {
            expected.extend_from_slice(&side.to_le_bytes());
        }
        expected.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(out, expected);
    }

    #[test]
    fn measurements_of_each_helper_go_to_a_file_of_its_own() {
        assert_eq!(
            measurement_path("/t/splat-%p-%8m.profraw", 42),
            "/t/splat-42-decoder.profraw"
        );
        assert_eq!(measurement_path("/t/100%x-%m", 1), "/t/100%x-decoder");
    }
}
