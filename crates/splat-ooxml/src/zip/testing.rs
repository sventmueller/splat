//! Archives built byte by byte for tests, with what a test wants wrong in them: a checksum, a size, a flag or an extra
//! field that the writer would never write.

use miniz_oxide::deflate::compress_to_vec;

use super::{CENTRAL_SIGNATURE, DEFLATED, END_SIGNATURE, LOCAL_SIGNATURE, STORED, crc32};

/// One entry of an archive built by hand; its fields go into the headers as they are.
#[derive(Clone, Debug)]
pub struct Part {
    /// The entry's name.
    pub name: String,
    /// The bytes as they stand in the archive.
    pub stored: Vec<u8>,
    /// The compression method, 0 for stored and 8 for deflated.
    pub method: u16,
    /// The checksum the headers give.
    pub crc: u32,
    /// The size the headers say the entry inflates to.
    pub size: u32,
    /// The general purpose flags.
    pub flags: u16,
    /// The extra field of the central directory record.
    pub extra: Vec<u8>,
    /// Local header with zero sizes and a data descriptor behind the data, as streaming writers make them.
    pub descriptor: bool,
}

/// An entry holding `content` deflated, with the right checksum and size.
pub fn deflated(name: &str, content: &[u8]) -> Part {
    Part {
        name: name.to_string(),
        stored: compress_to_vec(content, 6),
        method: DEFLATED,
        crc: crc32(content),
        size: u32::try_from(content.len()).expect("a small test"),
        flags: 0,
        extra: Vec::new(),
        descriptor: false,
    }
}

/// An entry holding `content` as it is.
pub fn stored(name: &str, content: &[u8]) -> Part {
    Part {
        stored: content.to_vec(),
        method: STORED,
        ..deflated(name, content)
    }
}

/// The bytes of an archive of `parts`, in their order, with the central directory behind them.
pub fn archive(parts: &[Part]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for part in parts {
        let offset = u32::try_from(out.len()).expect("a small test");
        let compressed = u32::try_from(part.stored.len()).expect("a small test");
        let name_len = u16::try_from(part.name.len()).expect("a small test");
        let extra_len = u16::try_from(part.extra.len()).expect("a small test");
        let descriptor_flag = if part.descriptor { 1 << 3 } else { 0 };
        out.extend_from_slice(LOCAL_SIGNATURE);
        for v in [20u16, part.flags | descriptor_flag, part.method, 0, 0x21] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        let local_sizes = if part.descriptor {
            [0, 0, 0]
        } else {
            [part.crc, compressed, part.size]
        };
        for v in local_sizes {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(part.name.as_bytes());
        out.extend_from_slice(&part.stored);
        if part.descriptor {
            out.extend_from_slice(b"PK\x07\x08");
            for v in [part.crc, compressed, part.size] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        directory.extend_from_slice(CENTRAL_SIGNATURE);
        for v in [20u16, 20, part.flags | descriptor_flag, part.method, 0, 0x21] {
            directory.extend_from_slice(&v.to_le_bytes());
        }
        for v in [part.crc, compressed, part.size] {
            directory.extend_from_slice(&v.to_le_bytes());
        }
        for v in [name_len, extra_len, 0, 0, 0] {
            directory.extend_from_slice(&v.to_le_bytes());
        }
        directory.extend_from_slice(&0u32.to_le_bytes());
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(part.name.as_bytes());
        directory.extend_from_slice(&part.extra);
    }
    let start = u32::try_from(out.len()).expect("a small test");
    let size = u32::try_from(directory.len()).expect("a small test");
    out.append(&mut directory);
    out.extend_from_slice(END_SIGNATURE);
    let count = u16::try_from(parts.len()).expect("a small test");
    for v in [0u16, 0, count, count] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}
