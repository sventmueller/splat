//! Pictures carried in `data:` addresses, as Word and browsers paste them.

use splat_markdown::url;

use crate::rich::{MOST_PICTURE_BYTES as MOST_BYTES, Source, is_picture};

/// `src` as what a picture comes from, if it can be had at all: an address on this disk means nothing to anyone else, and a
/// `data:` address has to hold what it says.
pub(super) fn source_of(src: &str) -> Option<Source> {
    match url::scheme(src).as_deref() {
        _ if src.is_empty() => None,
        Some("file") => None,
        Some("data") => {
            let (mime, bytes) = picture_in(src)?;
            Some(Source::Data { mime, bytes })
        }
        _ => Some(Source::Address(src.to_string())),
    }
}

fn picture_in(address: &str) -> Option<(&'static str, Vec<u8>)> {
    let (header, payload) = address.split_once(',')?;
    let header = header.to_ascii_lowercase();
    let mut parts = header.split_once(':')?.1.split(';');
    let mime = match parts.next()?.trim() {
        "image/png" => "image/png",
        "image/jpeg" | "image/jpg" => "image/jpeg",
        "image/gif" => "image/gif",
        "image/webp" => "image/webp",
        _ => return None,
    };
    if !parts.any(|part| part.trim() == "base64") {
        return None;
    }
    let symbols = payload.bytes().filter(|byte| !byte.is_ascii_whitespace()).count();
    if symbols / 4 * 3 > MOST_BYTES {
        return None;
    }
    let bytes = decode_base64(payload)?;
    is_picture(mime, &bytes).then_some((mime, bytes))
}

fn symbol_value(byte: u8) -> Option<u32> {
    let value = match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    };
    Some(u32::from(value))
}

/// Base64 with whitespace anywhere and its padding optional.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3 + 3);
    let mut bits = 0u32;
    let mut held = 0u32;
    let mut padded = false;
    for byte in text.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        if byte == b'=' {
            padded = true;
            continue;
        }
        if padded {
            return None;
        }
        bits = bits << 6 | symbol_value(byte)?;
        held += 1;
        if held == 4 {
            let [_, a, b, c] = bits.to_be_bytes();
            out.extend_from_slice(&[a, b, c]);
            bits = 0;
            held = 0;
        }
    }
    match held {
        0 => {}
        2 => {
            let [_, a, _, _] = (bits << 12).to_be_bytes();
            out.push(a);
        }
        3 => {
            let [_, a, b, _] = (bits << 6).to_be_bytes();
            out.extend_from_slice(&[a, b]);
        }
        _ => return None,
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_reads_with_and_without_padding_and_with_whitespace() {
        assert_eq!(decode_base64("TWFu").unwrap(), b"Man");
        assert_eq!(decode_base64("TWE=").unwrap(), b"Ma");
        assert_eq!(decode_base64("TWE").unwrap(), b"Ma");
        assert_eq!(decode_base64("TQ==").unwrap(), b"M");
        assert_eq!(decode_base64("T Q\n=\t=").unwrap(), b"M");
        assert_eq!(decode_base64("").unwrap(), b"");
        assert_eq!(decode_base64("+/+/").unwrap(), [0xFB, 0xFF, 0xBF]);
    }

    #[test]
    fn what_is_no_base64_is_refused() {
        for text in ["T", "TWFuT", "TW=F", "T*Fu", "ä"] {
            assert_eq!(decode_base64(text), None, "{text}");
        }
    }

    fn encoded(bytes: &[u8]) -> String {
        const SYMBOLS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for group in bytes.chunks(3) {
            let number = group.iter().fold(0usize, |n, b| n << 8 | usize::from(*b)) << (8 * (3 - group.len()));
            for n in 0..=group.len() {
                out.push(char::from(SYMBOLS[number >> (18 - 6 * n) & 63]));
            }
        }
        out
    }

    #[test]
    fn a_picture_has_to_be_what_its_address_says() {
        let png = encoded(b"\x89PNG\r\n\x1a\nrest");
        let jpeg = encoded(&[0xFF, 0xD8, 0xFF, 1]);
        let gif = encoded(b"GIF89a");
        let gif87 = encoded(b"GIF87a");
        let webp = encoded(b"RIFF....WEBPVP8");
        for (address, mime) in [
            (format!("data:image/png;base64,{png}"), "image/png"),
            (format!("DATA:IMAGE/PNG;BASE64,{png}"), "image/png"),
            (format!("data:image/jpeg;base64,{jpeg}"), "image/jpeg"),
            (format!("data:image/jpg;base64,{jpeg}"), "image/jpeg"),
            (format!("data:image/gif;base64,{gif}"), "image/gif"),
            (format!("data:image/gif;base64,{gif87}"), "image/gif"),
            (format!("data:image/webp;base64,{webp}"), "image/webp"),
            (format!("data:image/png;charset=x;base64,{png}"), "image/png"),
        ] {
            let source = source_of(&address).expect(&address);
            assert!(
                matches!(source, Source::Data { mime: found, .. } if found == mime),
                "{address}"
            );
        }
        for address in [
            format!("data:image/png;base64,{jpeg}"),
            format!("data:image/jpeg;base64,{png}"),
            format!("data:image/gif;base64,{png}"),
            format!("data:image/webp;base64,{png}"),
            format!("data:image/svg+xml;base64,{png}"),
            format!("data:text/html;base64,{png}"),
            format!("data:image/png,{png}"),
            "data:image/png;base64,***".to_string(),
            "data:image/png;base64".to_string(),
            "data:,x".to_string(),
        ] {
            assert_eq!(source_of(&address), None, "{address}");
        }
    }

    #[test]
    fn a_picture_beyond_the_limit_is_refused_before_it_is_decoded() {
        let fits = format!(
            "data:image/gif;base64,{}",
            "A".repeat(MOST_BYTES / 3 * 4 - 16) + &encoded(b"GIF89a")
        );
        assert_eq!(source_of(&fits), None, "the magic has to be at the start");
        let start = encoded(b"GIF89a");
        let fits = format!(
            "data:image/gif;base64,{start}{}",
            "A".repeat(MOST_BYTES / 3 * 4 - start.len())
        );
        assert!(source_of(&fits).is_some());
        let too_long = format!("{fits}AAAA");
        assert_eq!(source_of(&too_long), None);
    }

    #[test]
    fn addresses_on_this_disk_and_empty_ones_are_nothing_and_the_others_stay() {
        assert_eq!(source_of("file:///a/b.png"), None);
        assert_eq!(source_of("FILE:///a/b.png"), None);
        assert_eq!(source_of("fi\tle:///a/b.png"), None);
        assert_eq!(source_of(""), None);
        for address in ["https://a.bc/d.png", "d.png", "cid:abc"] {
            assert_eq!(source_of(address), Some(Source::Address(address.to_string())));
        }
    }
}
