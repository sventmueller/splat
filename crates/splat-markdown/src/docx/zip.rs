//! The container of a Word file: a ZIP archive with its entries stored as they are.

use super::TooLarge;

/// A ZIP archive with uncompressed entries, which Office reads like any other.
#[derive(Default)]
pub(super) struct Zip {
    data: Vec<u8>,
    directory: Vec<u8>,
    count: usize,
}

/// The highest values the format allows: the next ones announce its 64-bit extension.
pub(super) const MAX_ZIP_ENTRIES: usize = 0xFFFE;
pub(super) const MAX_ZIP_BYTES: usize = 0xFFFF_FFFE;

pub(super) fn zip_holds(entries: usize, bytes: usize) -> bool {
    entries <= MAX_ZIP_ENTRIES && bytes <= MAX_ZIP_BYTES
}

#[expect(clippy::cast_possible_truncation, reason = "`add` turns away what would not fit")]
impl Zip {
    pub(super) fn add(&mut self, name: &str, content: &[u8]) -> Result<(), TooLarge> {
        const HEADERS_AND_END: usize = 30 + 46 + 22;
        let bytes = self.data.len() + self.directory.len() + HEADERS_AND_END + 2 * name.len() + content.len();
        if !zip_holds(self.count + 1, bytes) {
            return Err(TooLarge);
        }
        let crc = crc32(content);
        let offset = self.data.len() as u32;
        let size = content.len() as u32;
        let header = |signature: u32, central: bool| {
            let mut h = Vec::new();
            h.extend_from_slice(&signature.to_le_bytes());
            if central {
                h.extend_from_slice(&20u16.to_le_bytes());
            }
            // Version 2.0, names in UTF-8, stored, no time stamp.
            for v in [20u16, 0x0800, 0, 0, 0x21] {
                h.extend_from_slice(&v.to_le_bytes());
            }
            for v in [crc, size, size] {
                h.extend_from_slice(&v.to_le_bytes());
            }
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            if central {
                for v in [0u16, 0, 0] {
                    h.extend_from_slice(&v.to_le_bytes());
                }
                h.extend_from_slice(&0u32.to_le_bytes());
                h.extend_from_slice(&offset.to_le_bytes());
            }
            h.extend_from_slice(name.as_bytes());
            h
        };
        self.data.extend(header(0x0403_4b50, false));
        self.data.extend_from_slice(content);
        self.directory.extend(header(0x0201_4b50, true));
        self.count += 1;
        Ok(())
    }

    pub(super) fn finish(mut self) -> Vec<u8> {
        let start = self.data.len() as u32;
        let size = self.directory.len() as u32;
        self.data.append(&mut self.directory);
        self.data.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        let count = self.count as u16;
        for v in [0u16, 0, count, count] {
            self.data.extend_from_slice(&v.to_le_bytes());
        }
        self.data.extend_from_slice(&size.to_le_bytes());
        self.data.extend_from_slice(&start.to_le_bytes());
        self.data.extend_from_slice(&0u16.to_le_bytes());
        self.data
    }
}

pub(super) fn crc32(bytes: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, entry) in (0u32..).zip(t.iter_mut()) {
            let mut c = i;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *entry = c;
        }
        t
    });
    !bytes
        .iter()
        .fold(!0u32, |c, &b| table[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8))
}

/// Entry names and contents of a stored ZIP, read with nothing but the format description.
#[cfg(any(test, feature = "selftest"))]
pub fn stored_entries(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut at = 0;
    while zip.get(at..at + 4) == Some(&0x0403_4b50u32.to_le_bytes()) {
        let field = |o: usize, n: usize| {
            zip[at + o..at + o + n]
                .iter()
                .rev()
                .fold(0usize, |v, b| v << 8 | *b as usize)
        };
        let (size, name_len, extra) = (field(18, 4), field(26, 2), field(28, 2));
        let name = String::from_utf8_lossy(&zip[at + 30..at + 30 + name_len]).into_owned();
        let start = at + 30 + name_len + extra;
        let content = zip[start..start + size].to_vec();
        assert_eq!(crc32(&content) as usize, field(14, 4), "{name}");
        out.push((name, content));
        at = start + size;
    }
    out
}
