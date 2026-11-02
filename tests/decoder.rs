//! The decoder helper, started the way the app starts it: it decodes images, and it can neither read files nor
//! open connections.

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};

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
