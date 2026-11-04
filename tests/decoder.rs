//! The decoder helper, started the way the app starts it: it decodes images, and it can neither read files nor
//! open connections.

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};

/// A helper that hangs on a blocking pipe would otherwise hang the whole test run.
fn limited() -> splat_testkit::TimeLimit {
    splat_testkit::time_limit(std::time::Duration::from_secs(60))
}

fn helper() -> Child {
    Command::new(env!("CARGO_BIN_EXE_splat"))
        .arg("--splat-decoder")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("helper starts")
}

fn send(child: &mut Child, kind: u8, payload: &[u8]) {
    let input = child.stdin.as_mut().unwrap();
    input.write_all(&[kind]).unwrap();
    input.write_all(&(payload.len() as u64).to_le_bytes()).unwrap();
    input.write_all(payload).unwrap();
    input.flush().unwrap();
}

fn read(child: &mut Child, n: usize) -> Vec<u8> {
    let mut out = vec![0; n];
    child.stdout.as_mut().unwrap().read_exact(&mut out).unwrap();
    out
}

/// A 2 × 1 BMP, red then blue: the format needs no checksums, so it is written here by hand.
fn bmp() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&62u32.to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&2i32.to_le_bytes());
    b.extend_from_slice(&1i32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    b.extend_from_slice(&[0; 24]);
    b.extend_from_slice(&[0, 0, 255, 255, 0, 0, 0, 0]);
    b
}

/// An image request: the longest side wanted, then the image.
fn image(side: u32, bytes: &[u8]) -> Vec<u8> {
    let mut payload = side.to_le_bytes().to_vec();
    payload.extend_from_slice(bytes);
    payload
}

#[test]
fn images_decode_to_pixels_and_svgs_to_pdf() {
    let _limit = limited();
    let mut child = helper();
    send(&mut child, 1, &image(2560, &bmp()));
    let head = read(&mut child, 17);
    assert_eq!(head, [2, 2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(read(&mut child, 8), [255, 0, 0, 255, 0, 0, 255, 255]);
    let svg =
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="20"><text x="2" y="15">Splat</text></svg>"#;
    send(&mut child, 1, &image(2560, svg));
    assert_eq!(read(&mut child, 1), [1]);
    let len = usize::try_from(u64::from_le_bytes(read(&mut child, 8).try_into().unwrap())).unwrap();
    assert!(read(&mut child, len).starts_with(b"%PDF"));
    send(&mut child, 1, &image(1, &bmp()));
    assert_eq!(
        read(&mut child, 17),
        [2, 1, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0],
        "scaled down to one pixel, and still naming its size"
    );
    read(&mut child, 4);
    send(&mut child, 1, &image(2560, b"not an image"));
    assert_eq!(read(&mut child, 1), [0]);
    send(&mut child, 1, b"no");
    assert_eq!(read(&mut child, 1), [0], "too short to name a side");
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success(), "the helper ends with its input");
}

/// MathJax leaves characters outside its fonts as text, and only the system fonts have glyphs for them.
#[test]
fn text_in_a_formula_reaches_the_pdf_as_outlines_in_every_script() {
    let _limit = limited();
    const FORMULA_SVG: u8 = 2;
    let mut child = helper();
    let mut pdf = |text: &str| {
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="40" viewBox="0 -750 12000 1000"><g fill="#000" transform="scale(1,-1)"><text transform="scale(1,-1)" font-size="884px" font-family="serif">{text}</text></g></svg>"##
        );
        send(&mut child, FORMULA_SVG, svg.as_bytes());
        assert_eq!(read(&mut child, 1), [1], "{text}");
        let len = usize::try_from(u64::from_le_bytes(read(&mut child, 8).try_into().unwrap())).unwrap();
        read(&mut child, len)
    };
    let empty = pdf("").len();
    for text in [
        "äöü",
        "αβγ",
        "Привет",
        "水",
        "日本語",
        "한국어",
        "العربية",
        "שלום",
        "ไทย",
        "हिन्दी",
    ] {
        let pdf = pdf(text);
        assert!(pdf.len() > empty, "{text} left no outlines");
        assert!(!pdf.windows(5).any(|w| w == b"/Font"), "{text} brought a font along");
    }
    assert!(
        pdf("😀").len() > 10_000,
        "a colored emoji is an image, unlike a placeholder"
    );
}

unsafe extern "C" {
    fn fcntl(fd: i32, command: i32, ...) -> i32;
}

#[test]
fn the_helper_can_neither_read_files_nor_connect() {
    let _limit = limited();
    const F_SETFD: i32 = 2;
    let path = std::env::temp_dir().join(format!("splat-secret-{}", std::process::id()));
    std::fs::write(&path, "secret").unwrap();
    // A file the app left open without close-on-exec, as a library might.
    let open = std::fs::File::open(&path).unwrap();
    // SAFETY: Clearing the flags of a descriptor this test owns for as long as the file lives.
    assert_eq!(unsafe { fcntl(std::os::fd::AsRawFd::as_raw_fd(&open), F_SETFD, 0) }, 0);
    let mut child = helper();
    drop(open);
    send(&mut child, 3, path.to_str().unwrap().as_bytes());
    assert_eq!(
        read(&mut child, 2),
        [3, 0],
        "the helper read the file or reached the network"
    );
    let _ = std::fs::remove_file(&path);
}

fn native_helper() -> Child {
    Command::new(env!("CARGO_BIN_EXE_splat"))
        .arg("--splat-native-decoder")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("helper starts")
}

/// Width, height, the size a picture says it has, and its pixels.
type Picture = (u32, u32, (u32, u32), Vec<u8>);

/// The answer to an image request of the native helper.
fn pixels_of(child: &mut Child, bytes: &[u8], side: u32) -> Option<Picture> {
    const NATIVE_IMAGE: u8 = 4;
    send(child, NATIVE_IMAGE, &image(side, bytes));
    if read(child, 1) != [2] {
        return None;
    }
    let number = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap());
    let head = read(child, 16);
    let (w, h) = (number(&head[..4]), number(&head[4..8]));
    let natural = (number(&head[8..12]), number(&head[12..16]));
    Some((w, h, natural, read(child, w as usize * h as usize * 4)))
}

/// What only the system reads, in every kind it comes in here: each must show something, in a process that started
/// without any of it having been read yet.
#[test]
fn the_system_decoders_work_inside_their_sandbox_and_hand_back_pixels() {
    let _limit = limited();
    let mut child = native_helper();
    for (name, bytes) in [
        ("TIFF", include_bytes!("../src/editor/warmup/sample.tiff").as_slice()),
        ("HEIC", include_bytes!("../src/editor/warmup/sample.heic")),
        ("AVIF", include_bytes!("../src/editor/warmup/sample.avif")),
        ("PDF", include_bytes!("../src/editor/warmup/sample.pdf")),
    ] {
        let (w, h, natural, pixels) = pixels_of(&mut child, bytes, 2560).unwrap_or_else(|| panic!("{name}"));
        assert!(w > 0 && h > 0 && natural.0 > 0 && natural.1 > 0, "{name}");
        assert!(pixels.iter().any(|p| *p != 0), "{name} shows nothing");
    }
    assert_eq!(pixels_of(&mut child, b"not an image", 2560), None);
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success(), "the helper ends with its input");
}

/// A pasted TIFF becomes a PNG of the full size, which pixels drawn for the preview would not be.
#[test]
fn a_pasted_tiff_becomes_a_png_inside_the_sandbox() {
    let _limit = limited();
    const PASTED_TIFF: u8 = 5;
    let tiff = include_bytes!("../src/editor/warmup/sample.tiff").as_slice();
    let mut child = native_helper();
    send(&mut child, PASTED_TIFF, tiff);
    assert_eq!(read(&mut child, 1), [3], "a file comes back");
    let len = u64::from_le_bytes(read(&mut child, 8).try_into().unwrap());
    let png = read(&mut child, usize::try_from(len).unwrap());
    assert!(png.starts_with(b"\x89PNG"));
    let (_, _, natural, _) = pixels_of(&mut child, tiff, 2560).expect("the TIFF");
    let (_, _, same, _) = pixels_of(&mut child, &png, 2560).expect("the PNG");
    assert_eq!(same, natural);
    send(&mut child, PASTED_TIFF, b"not an image");
    assert_eq!(read(&mut child, 1), [0]);
}

/// A PDF has no pixels of its own, so it is drawn sharp, within what was asked for and as large as it says it is.
#[test]
fn a_vector_picture_is_drawn_at_four_pixels_per_point_within_the_limit() {
    let _limit = limited();
    let mut child = native_helper();
    let pdf = include_bytes!("../src/editor/warmup/sample.pdf");
    let (w, h, natural, _) = pixels_of(&mut child, pdf, 2560).unwrap();
    assert_eq!((w, h, natural), (800, 160, (200, 40)));
    let (w, h, natural, _) = pixels_of(&mut child, pdf, 100).unwrap();
    assert_eq!(
        (w, h, natural),
        (100, 20, (200, 40)),
        "scaled to fit, and still naming its size"
    );
}

/// Half transparent colours arrive with straight alpha, as all pixels from a helper do.
#[test]
fn colours_come_apart_from_their_alpha() {
    let _limit = limited();
    let mut child = native_helper();
    let tiff = include_bytes!("../src/editor/warmup/sample.tiff");
    let (.., pixels) = pixels_of(&mut child, tiff, 2560).unwrap();
    let px = &pixels[..4];
    assert!(px[0] > 240 && px[1] < 10 && px[2] < 10, "{px:?}");
    assert!((120..=135).contains(&px[3]), "{px:?}");
}

/// The sandbox of this helper is wider than the other's by what the system's decoders need, and by nothing else.
#[test]
fn the_helper_for_the_system_decoders_reads_no_files_and_opens_no_connections() {
    let _limit = limited();
    const SELF_CHECK: u8 = 3;
    let secret = std::env::temp_dir().join(format!("splat-native-secret-{}", std::process::id()));
    std::fs::write(&secret, "secret").unwrap();
    let mut child = native_helper();
    send(&mut child, SELF_CHECK, secret.to_str().unwrap().as_bytes());
    assert_eq!(
        read(&mut child, 2),
        [SELF_CHECK, 0],
        "it reached the file or the network"
    );
    std::fs::remove_file(&secret).unwrap();
}
