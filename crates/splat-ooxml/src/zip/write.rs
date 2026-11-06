//! The container of a Word file as Splat writes it: a ZIP archive with its entries stored as they are.

use super::crc32;

/// A ZIP archive with uncompressed entries, which Office reads like any other.
#[derive(Default)]
pub struct Zip {
    data: Vec<u8>,
    directory: Vec<u8>,
    count: usize,
}

/// More than a ZIP holds without its 64-bit extension.
#[derive(Debug, PartialEq)]
pub struct TooLarge;

/// The highest values the format allows: the next ones announce its 64-bit extension.
const MOST_ENTRIES_WITHOUT_ZIP64: usize = 0xFFFE;
const MOST_BYTES_WITHOUT_ZIP64: usize = 0xFFFF_FFFE;

fn zip_holds(entries: usize, bytes: usize) -> bool {
    entries <= MOST_ENTRIES_WITHOUT_ZIP64 && bytes <= MOST_BYTES_WITHOUT_ZIP64
}

#[expect(clippy::cast_possible_truncation, reason = "`add` turns away what would not fit")]
impl Zip {
    /// Appends an entry `name` holding `content`; [`TooLarge`] when the archive would outgrow the format without its
    /// 64-bit extension.
    pub fn add(&mut self, name: &str, content: &[u8]) -> Result<(), TooLarge> {
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

    /// The bytes of the archive, its central directory at the end.
    pub fn finish(mut self) -> Vec<u8> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zip_turns_away_what_needs_its_extension() {
        assert!(zip_holds(MOST_ENTRIES_WITHOUT_ZIP64, MOST_BYTES_WITHOUT_ZIP64));
        assert!(!zip_holds(MOST_ENTRIES_WITHOUT_ZIP64 + 1, 0));
        assert!(!zip_holds(1, MOST_BYTES_WITHOUT_ZIP64 + 1));
        let mut zip = Zip::default();
        for _ in 0..MOST_ENTRIES_WITHOUT_ZIP64 {
            zip.add("a", b"").expect("room for it");
        }
        assert_eq!(zip.add("a", b""), Err(TooLarge));
        let bytes = zip.finish();
        let end = &bytes[bytes.len() - 22..];
        assert_eq!(end[..4], 0x0605_4b50u32.to_le_bytes());
        assert_eq!(end[10..12], 0xFFFEu16.to_le_bytes());
    }
}
