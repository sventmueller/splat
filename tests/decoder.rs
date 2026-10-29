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

#[test]
fn images_decode_to_pixels_and_svgs_to_pdf() {
    let mut child = helper();
    send(&mut child, 1, &bmp());
    let head = read(&mut child, 9);
    assert_eq!(head, [2, 2, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(read(&mut child, 8), [255, 0, 0, 255, 0, 0, 255, 255]);
    let svg =
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="20"><text x="2" y="15">Splat</text></svg>"#;
    send(&mut child, 1, svg);
    assert_eq!(read(&mut child, 1), [1]);
    let len = u64::from_le_bytes(read(&mut child, 8).try_into().unwrap()) as usize;
    assert!(read(&mut child, len).starts_with(b"%PDF"));
    send(&mut child, 1, b"not an image");
    assert_eq!(read(&mut child, 1), [0]);
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success(), "the helper ends with its input");
}

#[test]
fn the_helper_can_neither_read_files_nor_connect() {
    let path = std::env::temp_dir().join(format!("splat-secret-{}", std::process::id()));
    std::fs::write(&path, "secret").unwrap();
    let mut child = helper();
    send(&mut child, 3, path.to_str().unwrap().as_bytes());
    assert_eq!(
        read(&mut child, 2),
        [3, 0],
        "the helper read the file or reached the network"
    );
    let _ = std::fs::remove_file(&path);
}
