//! What the app and the decoding service say to each other over a socket: a request names its kind and carries its
//! bytes, an answer names its kind and carries a PDF, a PNG or pixels. The app believes an answer nothing beyond the limits
//! of the kind it asked for, as the service decodes untrusted data and may have been taken over.

use std::io::{Read, Write};
use std::time::Duration;

/// Larger downloads and SVG sources are not shown.
pub const MAX_BYTES: usize = 10 << 20;
/// Larger image files are not shown; camera photos stay well below.
pub const MAX_FILE_BYTES: usize = 64 << 20;
/// Larger images are not decoded at all.
pub const MAX_SIDE: u32 = 16_384;
/// Longest side of a decoded image in the preview and on paper, where 2480 pixels fill an A4 page at 300 dpi; larger
/// ones are scaled down.
pub const SHOWN_SIDE: u32 = 2560;
/// Longest side of an image converted for an export, which outlives the screen it is first seen on.
pub const EXPORT_SIDE: u32 = 4096;

#[derive(Debug)]
pub enum Decoded {
    Pdf(Vec<u8>),
    /// A picture from the pasteboard as a file to keep, with its full size and colors.
    Png(Vec<u8>),
    Rgba {
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        /// The size the file gives, upright: the preview shows an image scaled down for memory no smaller.
        natural: (u32, u32),
    },
}

impl Decoded {
    /// How much memory it holds.
    pub fn bytes(&self) -> usize {
        match self {
            Decoded::Pdf(bytes) | Decoded::Png(bytes) | Decoded::Rgba { pixels: bytes, .. } => bytes.len(),
        }
    }
}

/// A decode that takes longer ends its service, which then counts as failed.
pub const TIMEOUT: Duration = Duration::from_secs(10);
pub const MAX_PDF: usize = 128 << 20;
/// Beyond `MAX_SIDE`; the service is trusted for no more than a sane number.
pub const NATURAL_SIDE: u32 = 1 << 20;

/// What the app asks a helper for, in the first byte of a request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Request {
    /// A picture file as pixels, or an SVG as a PDF.
    Image,
    /// A formula's SVG as a PDF.
    FormulaSvg,
    /// Whether the helper can reach a file or the network.
    SelfCheck,
    /// What only the system's decoders read, as pixels.
    NativeImage,
    /// A TIFF from the pasteboard as a PNG.
    PastedTiff,
}

/// What kind of answer follows, in the first byte of an answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    Nothing,
    Pdf,
    Rgba,
    Png,
}

/// A first byte that names no request or answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnknownCode(u8);

impl From<UnknownCode> for std::io::Error {
    fn from(UnknownCode(code): UnknownCode) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("unknown code {code}"))
    }
}

impl From<Request> for u8 {
    fn from(request: Request) -> u8 {
        match request {
            Request::Image => 1,
            Request::FormulaSvg => 2,
            Request::SelfCheck => 3,
            Request::NativeImage => 4,
            Request::PastedTiff => 5,
        }
    }
}

impl TryFrom<u8> for Request {
    type Error = UnknownCode;

    fn try_from(code: u8) -> Result<Request, UnknownCode> {
        match code {
            1 => Ok(Request::Image),
            2 => Ok(Request::FormulaSvg),
            3 => Ok(Request::SelfCheck),
            4 => Ok(Request::NativeImage),
            5 => Ok(Request::PastedTiff),
            _ => Err(UnknownCode(code)),
        }
    }
}

impl From<Answer> for u8 {
    fn from(answer: Answer) -> u8 {
        match answer {
            Answer::Nothing => 0,
            Answer::Pdf => 1,
            Answer::Rgba => 2,
            Answer::Png => 3,
        }
    }
}

impl TryFrom<u8> for Answer {
    type Error = UnknownCode;

    fn try_from(code: u8) -> Result<Answer, UnknownCode> {
        match code {
            0 => Ok(Answer::Nothing),
            1 => Ok(Answer::Pdf),
            2 => Ok(Answer::Rgba),
            3 => Ok(Answer::Png),
            _ => Err(UnknownCode(code)),
        }
    }
}

impl Request {
    /// The most bytes a request of this kind may carry, without the side it names.
    pub fn most_bytes(self) -> usize {
        match self {
            Request::PastedTiff => MAX_PASTED_BYTES,
            Request::Image | Request::FormulaSvg | Request::SelfCheck | Request::NativeImage => MAX_FILE_BYTES,
        }
    }

    /// Whether the helper for what only the system's decoders read answers it.
    pub fn is_native(self) -> bool {
        match self {
            Request::NativeImage | Request::PastedTiff => true,
            Request::Image | Request::FormulaSvg | Request::SelfCheck => false,
        }
    }

    /// Which kind of answer a request can have, so that a helper gets only as much memory out of the app as that kind
    /// takes.
    pub fn may_have(self, answer: Answer) -> bool {
        match (self, answer) {
            (_, Answer::Nothing)
            | (Request::Image, Answer::Pdf | Answer::Rgba)
            | (Request::FormulaSvg, Answer::Pdf)
            | (Request::NativeImage, Answer::Rgba)
            | (Request::PastedTiff, Answer::Png) => true,
            (Request::Image, Answer::Png)
            | (Request::FormulaSvg, Answer::Rgba | Answer::Png)
            | (Request::NativeImage, Answer::Pdf | Answer::Png)
            | (Request::PastedTiff, Answer::Pdf | Answer::Rgba)
            | (Request::SelfCheck, Answer::Pdf | Answer::Rgba | Answer::Png) => false,
        }
    }
}

/// What the service may take of the memory while it decodes: enough for a pasted picture at `MAX_PASTED_BYTES` and its
/// pixels.
pub fn most_memory() -> u64 {
    (objc2_foundation::NSProcessInfo::processInfo().physicalMemory() / 4).max(3 << 30)
}

/// The memory a process holds as the system counts it, or `None` once it has gone.
pub fn memory_of(pid: i32) -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage_info_v4>::uninit();
    // SAFETY: The buffer has the size of the kind of answer asked for.
    let read = unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, usage.as_mut_ptr().cast()) };
    // SAFETY: On success the call has filled the buffer.
    (read == 0).then(|| unsafe { usage.assume_init() }.ri_phys_footprint)
}

/// A screenshot of a large display can come as an uncompressed TIFF far beyond any image file a document holds.
pub const MAX_PASTED_BYTES: usize = 1 << 30;

/// The longest side a request for pixels names, in front of its file.
pub const SIDE_BYTES: usize = 4;

/// A request as the helper reads it. A request it does not know ends it, as the app sends none.
pub fn read_request(input: &mut impl Read) -> std::io::Result<(Request, Vec<u8>)> {
    let mut head = [0u8; 9];
    input.read_exact(&mut head)?;
    let kind = Request::try_from(head[0])?;
    let len = usize::try_from(u64::from_le_bytes(head[1..].try_into().expect("8 bytes"))).unwrap_or(usize::MAX);
    let most = match kind {
        Request::PastedTiff => MAX_PASTED_BYTES,
        Request::Image | Request::FormulaSvg | Request::SelfCheck | Request::NativeImage => MAX_FILE_BYTES + SIDE_BYTES,
    };
    if len > most {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut payload = vec![0; len];
    input.read_exact(&mut payload)?;
    Ok((kind, payload))
}

pub fn write_answer(output: &mut impl Write, answer: Option<Decoded>) -> std::io::Result<()> {
    match answer {
        None => output.write_all(&[u8::from(Answer::Nothing)]),
        Some(Decoded::Pdf(pdf)) => {
            output.write_all(&[u8::from(Answer::Pdf)])?;
            output.write_all(&(pdf.len() as u64).to_le_bytes())?;
            output.write_all(&pdf)
        }
        Some(Decoded::Png(png)) => {
            output.write_all(&[u8::from(Answer::Png)])?;
            output.write_all(&(png.len() as u64).to_le_bytes())?;
            output.write_all(&png)
        }
        Some(Decoded::Rgba {
            width,
            height,
            pixels,
            natural,
        }) => {
            output.write_all(&[u8::from(Answer::Rgba)])?;
            for side in [width, height, natural.0, natural.1] {
                output.write_all(&side.to_le_bytes())?;
            }
            output.write_all(&pixels)
        }
    }
}

/// The answer of a helper to a request of `kind`. It may have been taken over: nothing it sends is trusted beyond these
/// bounds.
pub fn read_answer(input: &mut impl Read, kind: Request, side: Option<u32>) -> std::io::Result<Option<Decoded>> {
    let invalid = || std::io::Error::from(std::io::ErrorKind::InvalidData);
    let u32_from = |input: &mut dyn Read| -> std::io::Result<u32> {
        let mut b = [0u8; 4];
        input.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    };
    let mut tag = [0u8; 1];
    input.read_exact(&mut tag)?;
    let answer = Answer::try_from(tag[0])?;
    if !kind.may_have(answer) {
        return Err(invalid());
    }
    match answer {
        Answer::Nothing => Ok(None),
        Answer::Pdf => {
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
        Answer::Png => {
            let mut len = [0u8; 8];
            input.read_exact(&mut len)?;
            let len = usize::try_from(u64::from_le_bytes(len)).map_err(|_| invalid())?;
            if len > MAX_PASTED_BYTES {
                return Err(invalid());
            }
            let mut png = vec![0; len];
            input.read_exact(&mut png)?;
            Ok(png.starts_with(b"\x89PNG").then_some(Decoded::Png(png)))
        }
        Answer::Rgba => {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(width: u32, height: u32, natural: (u32, u32), pixels: usize) -> Vec<u8> {
        let mut answer = vec![u8::from(Answer::Rgba)];
        for value in [width, height, natural.0, natural.1] {
            answer.extend_from_slice(&value.to_le_bytes());
        }
        answer.extend(std::iter::repeat_n(0, pixels));
        answer
    }

    fn read(answer: &[u8], side: Option<u32>) -> std::io::Result<Option<Decoded>> {
        read_answer(&mut std::io::Cursor::new(answer), Request::Image, side)
    }

    #[test]
    fn an_honest_answer_comes_through() {
        assert!(read(&[u8::from(Answer::Nothing)], None).unwrap().is_none());
        let Decoded::Rgba { width, height, .. } = read(&rgba(2, 1, (2, 1), 8), Some(10)).unwrap().unwrap() else {
            panic!()
        };
        assert_eq!((width, height), (2, 1));
        let mut pdf = vec![u8::from(Answer::Pdf)];
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
        let mut huge_pdf = vec![u8::from(Answer::Pdf)];
        huge_pdf.extend_from_slice(&(MAX_PDF as u64 + 1).to_le_bytes());
        assert!(read(&huge_pdf, None).is_err(), "a PDF over the limit is not even read");
        let mut no_pdf = vec![u8::from(Answer::Pdf)];
        no_pdf.extend_from_slice(&3u64.to_le_bytes());
        no_pdf.extend_from_slice(b"abc");
        assert!(
            read(&no_pdf, None).unwrap().is_none(),
            "what is no PDF stays out of the system's decoders"
        );
    }

    #[test]
    fn a_helper_answers_only_in_the_kind_its_request_has() {
        let mut huge_png = vec![u8::from(Answer::Png)];
        huge_png.extend_from_slice(&(MAX_PASTED_BYTES as u64).to_le_bytes());
        for kind in [
            Request::Image,
            Request::FormulaSvg,
            Request::NativeImage,
            Request::SelfCheck,
        ] {
            let read = read_answer(&mut std::io::Cursor::new(&huge_png), kind, Some(10));
            let error = read.expect_err("no room is made for it");
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{kind:?}");
        }
        let mut png = vec![u8::from(Answer::Png)];
        png.extend_from_slice(&4u64.to_le_bytes());
        png.extend_from_slice(b"\x89PNG");
        let pasted = read_answer(&mut std::io::Cursor::new(&png), Request::PastedTiff, None);
        assert!(matches!(pasted.unwrap(), Some(Decoded::Png(_))));
        let pixels = read_answer(
            &mut std::io::Cursor::new(rgba(1, 1, (1, 1), 4)),
            Request::PastedTiff,
            None,
        );
        assert!(pixels.is_err(), "pixels are no answer to a pasted picture");
        let pdf_of_formula = read_answer(
            &mut std::io::Cursor::new(rgba(1, 1, (1, 1), 4)),
            Request::FormulaSvg,
            None,
        );
        assert!(pdf_of_formula.is_err(), "and none to a formula");
    }

    #[test]
    fn without_a_limit_of_its_own_an_answer_stays_within_the_export_size() {
        assert!(read(&rgba(EXPORT_SIDE + 1, 1, (1, 1), 0), None).is_err());
    }

    #[test]
    fn requests_and_answers_cross_the_pipe_intact() {
        let mut wire = vec![u8::from(Request::Image)];
        wire.extend_from_slice(&3u64.to_le_bytes());
        wire.extend_from_slice(b"abc");
        assert_eq!(
            read_request(&mut wire.as_slice()).unwrap(),
            (Request::Image, b"abc".to_vec())
        );
        let mut huge = vec![u8::from(Request::Image)];
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
        let mut expected = vec![u8::from(Answer::Nothing), u8::from(Answer::Pdf)];
        expected.extend_from_slice(&4u64.to_le_bytes());
        expected.extend_from_slice(b"%PDF");
        expected.push(u8::from(Answer::Rgba));
        for side in [1u32, 1, 9, 9] {
            expected.extend_from_slice(&side.to_le_bytes());
        }
        expected.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(out, expected);
    }

    #[test]
    fn every_code_names_one_request_or_answer_and_the_rest_none() {
        let requests: Vec<Request> = (0..=u8::MAX).filter_map(|code| Request::try_from(code).ok()).collect();
        let answers: Vec<Answer> = (0..=u8::MAX).filter_map(|code| Answer::try_from(code).ok()).collect();
        assert_eq!(requests.len(), 5);
        assert_eq!(answers.len(), 4);
        for code in 0..=u8::MAX {
            match Request::try_from(code) {
                Ok(request) => assert_eq!(u8::from(request), code),
                Err(unknown) => assert_eq!(unknown, UnknownCode(code)),
            }
            match Answer::try_from(code) {
                Ok(answer) => assert_eq!(u8::from(answer), code),
                Err(unknown) => assert_eq!(unknown, UnknownCode(code)),
            }
        }
    }

    #[test]
    fn a_request_the_helper_does_not_know_ends_it() {
        let mut wire = vec![9];
        wire.extend_from_slice(&0u64.to_le_bytes());
        let error = read_request(&mut wire.as_slice()).expect_err("no request");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        let mut tiff = vec![u8::from(Request::PastedTiff)];
        tiff.extend_from_slice(&((MAX_FILE_BYTES + SIDE_BYTES + 1) as u64).to_le_bytes());
        assert!(
            read_request(&mut tiff.as_slice()).is_err_and(|e| e.kind() == std::io::ErrorKind::UnexpectedEof),
            "a pasted picture may be larger than a file"
        );
    }

    #[test]
    fn the_memory_of_a_process_can_be_read_and_of_one_that_is_gone_not() {
        let own = i32::try_from(std::process::id()).unwrap();
        assert!(memory_of(own).is_some_and(|bytes| bytes > 1 << 20));
        let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        let pid = i32::try_from(child.id()).unwrap();
        child.wait().unwrap();
        assert_eq!(memory_of(pid), None);
    }

    #[test]
    fn only_pictures_of_the_system_go_to_its_helper_and_only_pasted_ones_may_be_huge() {
        assert!(Request::NativeImage.is_native() && Request::PastedTiff.is_native());
        assert!(!Request::Image.is_native() && !Request::FormulaSvg.is_native() && !Request::SelfCheck.is_native());
        assert_eq!(Request::PastedTiff.most_bytes(), MAX_PASTED_BYTES);
        assert_eq!(Request::Image.most_bytes(), MAX_FILE_BYTES);
    }
}
