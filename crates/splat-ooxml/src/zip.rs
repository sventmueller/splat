//! ZIP archives: a reader by their central directory, for files from anywhere, which refuses what overlaps, lies
//! outside the file, is encrypted or needs the 64-bit extension and holds inflating to limits while it inflates; and a
//! writer of stored entries for Word.

#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod write;

use std::cell::Cell;
use std::collections::HashMap;

use miniz_oxide::inflate::decompress_to_vec_with_limit;

pub use write::{TooLarge, Zip};

/// Why an archive or an entry cannot be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZipError {
    /// Bytes without the end record of a ZIP archive.
    NotAnArchive,
    /// An archive with an entry protected by a password.
    Encrypted,
    /// An archive that needs the 64-bit extension, which this reader refuses.
    Zip64,
    /// More entries than `MOST_ENTRIES`, or more bytes inflated from the archive in all than `MOST_TOTAL_BYTES`.
    TooLarge,
    /// One entry that inflates to more than `MOST_ENTRY_BYTES`; the others can still be read.
    EntryTooLarge,
    /// An archive whose records contradict each other or its content.
    Damaged,
}

impl std::fmt::Display for ZipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ZipError::NotAnArchive => "not a ZIP archive",
            ZipError::Encrypted => "a ZIP archive with encrypted entries",
            ZipError::Zip64 => "a ZIP archive that needs the 64-bit extension",
            ZipError::TooLarge => "a ZIP archive beyond the limits of what is read",
            ZipError::EntryTooLarge => "an entry of a ZIP archive larger than one entry may be",
            ZipError::Damaged => "a damaged ZIP archive",
        })
    }
}

impl std::error::Error for ZipError {}

/// The most entries an archive may have.
pub const MOST_ENTRIES: usize = 10_000;
/// The most bytes an entry may inflate to.
pub const MOST_ENTRY_BYTES: u64 = 256 << 20;
/// The most bytes all that is read from an archive may inflate to.
pub const MOST_TOTAL_BYTES: u64 = 1 << 30;

const END_SIGNATURE: &[u8; 4] = b"PK\x05\x06";
const ZIP64_LOCATOR_SIGNATURE: &[u8; 4] = b"PK\x06\x07";
const CENTRAL_SIGNATURE: &[u8; 4] = b"PK\x01\x02";
const LOCAL_SIGNATURE: &[u8; 4] = b"PK\x03\x04";
const END_SIZE: usize = 22;
const CENTRAL_SIZE: usize = 46;
const LOCAL_SIZE: usize = 30;
const LONGEST_COMMENT: usize = 0xFFFF;
const ZIP64_EXTRA: u16 = 1;

const ENCRYPTED_FLAGS: u16 = 1 | 1 << 6 | 1 << 13;
const STORED: u16 = 0;
const DEFLATED: u16 = 8;
const AES: u16 = 99;

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// An entry's place in the file, without its name.
#[derive(Clone, Copy, Debug)]
struct Entry {
    method: u16,
    crc: u32,
    compressed: u32,
    size: u32,
    data_start: usize,
}

/// An archive being read; what it hands out counts against `MOST_TOTAL_BYTES`.
#[derive(Debug)]
pub struct Archive<'a> {
    data: &'a [u8],
    names: Vec<String>,
    entries: Vec<Entry>,
    by_name: HashMap<String, usize>,
    inflated: Cell<u64>,
}

/// The central directory as the end record describes it.
struct Directory {
    start: usize,
    size: usize,
    count: usize,
}

fn find_directory(data: &[u8]) -> Result<Directory, ZipError> {
    let last = data.len().checked_sub(END_SIZE).ok_or(ZipError::NotAnArchive)?;
    let first = last.saturating_sub(LONGEST_COMMENT);
    let end = (first..=last)
        .rev()
        .find(|&at| {
            &data[at..at + 4] == END_SIGNATURE
                && u16_at(data, at + 20).is_some_and(|comment| at + END_SIZE + usize::from(comment) == data.len())
        })
        .ok_or(ZipError::NotAnArchive)?;
    if end >= 20 && &data[end - 20..end - 16] == ZIP64_LOCATOR_SIGNATURE {
        return Err(ZipError::Zip64);
    }
    let field16 = |offset| u16_at(data, end + offset).ok_or(ZipError::Damaged);
    let field32 = |offset| u32_at(data, end + offset).ok_or(ZipError::Damaged);
    let (disk, directory_disk, on_this_disk, count) = (field16(4)?, field16(6)?, field16(8)?, field16(10)?);
    let (size, start) = (field32(12)?, field32(16)?);
    if count == 0xFFFF || size == 0xFFFF_FFFF || start == 0xFFFF_FFFF {
        return Err(ZipError::Zip64);
    }
    if disk != 0 || directory_disk != 0 || on_this_disk != count {
        return Err(ZipError::Damaged);
    }
    let (size, start) = (size as usize, start as usize);
    if start.checked_add(size).is_none_or(|directory_end| directory_end > end) {
        return Err(ZipError::Damaged);
    }
    let count = usize::from(count);
    if count > MOST_ENTRIES {
        return Err(ZipError::TooLarge);
    }
    if count.saturating_mul(CENTRAL_SIZE) > size {
        return Err(ZipError::Damaged);
    }
    Ok(Directory { start, size, count })
}

impl<'a> Archive<'a> {
    /// Reads the directory of `data`, checking every entry's place in the file.
    pub fn read(data: &'a [u8]) -> Result<Archive<'a>, ZipError> {
        let directory = find_directory(data)?;
        let mut archive = Archive {
            data,
            names: Vec::with_capacity(directory.count),
            entries: Vec::with_capacity(directory.count),
            by_name: HashMap::with_capacity(directory.count),
            inflated: Cell::new(0),
        };
        let mut spans = Vec::with_capacity(directory.count);
        let mut at = directory.start;
        let end = directory.start + directory.size;
        for _ in 0..directory.count {
            at = archive.read_entry(at, end, directory.start, &mut spans)?;
        }
        spans.sort_unstable();
        if spans.windows(2).any(|pair| pair[0].1 > pair[1].0) {
            return Err(ZipError::Damaged);
        }
        Ok(archive)
    }

    /// Takes in the directory record at `at` and returns the offset of the record behind it.
    fn read_entry(
        &mut self,
        at: usize,
        directory_end: usize,
        files_end: usize,
        spans: &mut Vec<(usize, usize)>,
    ) -> Result<usize, ZipError> {
        let record = self.data.get(at..directory_end).ok_or(ZipError::Damaged)?;
        if !record.starts_with(CENTRAL_SIGNATURE) || record.len() < CENTRAL_SIZE {
            return Err(ZipError::Damaged);
        }
        let field16 = |offset| u16_at(record, offset).ok_or(ZipError::Damaged);
        let field32 = |offset| u32_at(record, offset).ok_or(ZipError::Damaged);
        let (flags, method) = (field16(8)?, field16(10)?);
        let (compressed, size, local) = (field32(20)?, field32(24)?, field32(42)?);
        let (name_len, extra_len, comment_len) = (
            usize::from(field16(28)?),
            usize::from(field16(30)?),
            usize::from(field16(32)?),
        );
        if flags & ENCRYPTED_FLAGS != 0 || method == AES {
            return Err(ZipError::Encrypted);
        }
        if compressed == 0xFFFF_FFFF || size == 0xFFFF_FFFF || local == 0xFFFF_FFFF {
            return Err(ZipError::Zip64);
        }
        if method != STORED && method != DEFLATED || (method == STORED && compressed != size) || field16(34)? != 0 {
            return Err(ZipError::Damaged);
        }
        let name = record
            .get(CENTRAL_SIZE..CENTRAL_SIZE + name_len)
            .ok_or(ZipError::Damaged)?;
        let extra = record
            .get(CENTRAL_SIZE + name_len..CENTRAL_SIZE + name_len + extra_len)
            .ok_or(ZipError::Damaged)?;
        if holds_zip64_field(extra) {
            return Err(ZipError::Zip64);
        }
        let record_len = CENTRAL_SIZE + name_len + extra_len + comment_len;
        if record_len > record.len() {
            return Err(ZipError::Damaged);
        }
        let local = local as usize;
        let data_start = self.data_start(local, files_end)?;
        let data_end = data_start
            .checked_add(compressed as usize)
            .filter(|end| *end <= files_end)
            .ok_or(ZipError::Damaged)?;
        spans.push((local, data_end));
        let name = String::from_utf8_lossy(name).into_owned();
        if self.by_name.insert(name.clone(), self.entries.len()).is_some() {
            return Err(ZipError::Damaged);
        }
        self.names.push(name);
        self.entries.push(Entry {
            method,
            crc: field32(16)?,
            compressed,
            size,
            data_start,
        });
        Ok(at + record_len)
    }

    /// Where the data behind the local header at `local` starts.
    fn data_start(&self, local: usize, files_end: usize) -> Result<usize, ZipError> {
        let header = self
            .data
            .get(local..local.checked_add(LOCAL_SIZE).ok_or(ZipError::Damaged)?)
            .filter(|header| header.starts_with(LOCAL_SIGNATURE) && local + LOCAL_SIZE <= files_end)
            .ok_or(ZipError::Damaged)?;
        let flags = u16_at(header, 6).ok_or(ZipError::Damaged)?;
        if flags & ENCRYPTED_FLAGS != 0 {
            return Err(ZipError::Encrypted);
        }
        let name_len = usize::from(u16_at(header, 26).ok_or(ZipError::Damaged)?);
        let extra_len = usize::from(u16_at(header, 28).ok_or(ZipError::Damaged)?);
        Ok(local + LOCAL_SIZE + name_len + extra_len)
    }

    /// The names of the entries, in the order of the directory.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }

    /// Whether an entry of this name exists.
    pub fn contains(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// What the entry says it inflates to.
    pub fn size_of(&self, name: &str) -> Option<u64> {
        let index = *self.by_name.get(name)?;
        Some(u64::from(self.entries[index].size))
    }

    /// The content of an entry, if the archive has one of this name.
    pub fn get(&self, name: &str) -> Result<Option<Vec<u8>>, ZipError> {
        let Some(&index) = self.by_name.get(name) else {
            return Ok(None);
        };
        self.extract(self.entries[index]).map(Some)
    }

    fn extract(&self, entry: Entry) -> Result<Vec<u8>, ZipError> {
        let size = u64::from(entry.size);
        let total = self.inflated.get();
        if size > MOST_ENTRY_BYTES {
            return Err(ZipError::EntryTooLarge);
        }
        if total.saturating_add(size) > MOST_TOTAL_BYTES {
            return Err(ZipError::TooLarge);
        }
        let packed = self
            .data
            .get(entry.data_start..entry.data_start + entry.compressed as usize)
            .ok_or(ZipError::Damaged)?;
        let content = if entry.method == STORED {
            packed.to_vec()
        } else {
            // The limit is what the entry says of itself, so a bomb that lies about its size ends here, whatever it
            // holds.
            decompress_to_vec_with_limit(packed, entry.size as usize).map_err(|_| ZipError::Damaged)?
        };
        if content.len() != entry.size as usize || crc32(&content) != entry.crc {
            return Err(ZipError::Damaged);
        }
        self.inflated.set(total + content.len() as u64);
        Ok(content)
    }
}

fn holds_zip64_field(extra: &[u8]) -> bool {
    let mut at = 0;
    while let (Some(id), Some(len)) = (u16_at(extra, at), u16_at(extra, at + 2)) {
        if id == ZIP64_EXTRA {
            return true;
        }
        at += 4 + usize::from(len);
    }
    false
}

/// The CRC-32 checksum ZIP stores for each entry.
pub fn crc32(bytes: &[u8]) -> u32 {
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

#[cfg(test)]
mod tests {
    use super::testing::{Part, archive, deflated, stored};
    use super::*;

    fn end_of(bytes: &[u8]) -> usize {
        bytes.len() - END_SIZE
    }

    #[test]
    fn stored_and_deflated_entries_read_back_in_directory_order() {
        let long = "Hallo ".repeat(1000);
        let file = archive(&[
            deflated("word/document.xml", long.as_bytes()),
            stored("b.txt", b"b"),
            stored("empty", b""),
        ]);
        let read = Archive::read(&file).unwrap();
        assert_eq!(
            read.names().collect::<Vec<_>>(),
            ["word/document.xml", "b.txt", "empty"]
        );
        assert!(read.contains("b.txt") && !read.contains("c"));
        assert_eq!(read.size_of("word/document.xml"), Some(long.len() as u64));
        assert_eq!(read.size_of("c"), None);
        assert_eq!(read.get("word/document.xml").unwrap().unwrap(), long.as_bytes());
        assert_eq!(read.get("b.txt").unwrap().unwrap(), b"b");
        assert_eq!(read.get("empty").unwrap().unwrap(), b"");
        assert_eq!(read.get("c").unwrap(), None);
    }

    #[test]
    fn an_error_says_in_words_what_went_wrong() {
        let all = [
            ZipError::NotAnArchive,
            ZipError::Encrypted,
            ZipError::Zip64,
            ZipError::TooLarge,
            ZipError::EntryTooLarge,
            ZipError::Damaged,
        ];
        let words: std::collections::HashSet<String> = all.iter().map(ToString::to_string).collect();
        assert_eq!(words.len(), all.len());
        let error: Box<dyn std::error::Error> = Box::new(ZipError::Zip64);
        assert!(error.to_string().contains("64-bit"));
    }

    #[test]
    fn the_archive_splat_writes_can_be_read() {
        let mut zip = Zip::default();
        zip.add("a", b"1").unwrap();
        zip.add("ä/b", b"22").unwrap();
        let bytes = zip.finish();
        let read = Archive::read(&bytes).unwrap();
        assert_eq!(read.get("ä/b").unwrap().unwrap(), b"22");
    }

    #[test]
    fn crc_matches_the_reference_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn a_data_descriptor_changes_nothing() {
        let mut part = deflated("a", b"Hallo Hallo");
        part.descriptor = true;
        let file = archive(&[part, stored("b", b"x")]);
        let read = Archive::read(&file).unwrap();
        assert_eq!(read.get("a").unwrap().unwrap(), b"Hallo Hallo");
        assert_eq!(read.get("b").unwrap().unwrap(), b"x");
    }

    #[test]
    fn a_comment_behind_the_directory_is_no_problem_but_junk_is() {
        let mut file = archive(&[stored("a", b"x")]);
        let end = end_of(&file);
        file[end + 20] = 3;
        file.extend_from_slice(b"abc");
        assert!(Archive::read(&file).is_ok());
        file.push(b'x');
        assert_eq!(Archive::read(&file).unwrap_err(), ZipError::NotAnArchive);
    }

    #[test]
    fn what_is_no_archive_is_said_to_be_none() {
        for bytes in [&b""[..], b"PK", b"plain text that is long enough to hold an end record"] {
            assert_eq!(Archive::read(bytes).unwrap_err(), ZipError::NotAnArchive);
        }
    }

    #[test]
    fn encryption_is_refused_by_flag_method_and_local_header() {
        let mut flagged = stored("a", b"x");
        flagged.flags = 1;
        assert_eq!(Archive::read(&archive(&[flagged])).unwrap_err(), ZipError::Encrypted);
        let mut aes = stored("a", b"x");
        aes.method = AES;
        assert_eq!(Archive::read(&archive(&[aes])).unwrap_err(), ZipError::Encrypted);
        let mut local_only = archive(&[stored("a", b"x")]);
        local_only[6] = 1;
        assert_eq!(Archive::read(&local_only).unwrap_err(), ZipError::Encrypted);
    }

    #[test]
    fn the_64_bit_extension_is_refused_wherever_it_shows() {
        let mut extra = stored("a", b"x");
        extra.extra = vec![9, 0, 1, 0, 7, 1, 0, 0, 0];
        assert_eq!(Archive::read(&archive(&[extra])).unwrap_err(), ZipError::Zip64);
        let mut harmless_extra = stored("a", b"x");
        harmless_extra.extra = vec![9, 0, 1, 0, 7];
        assert!(Archive::read(&archive(&[harmless_extra])).is_ok());
        let mut sized = stored("a", b"x");
        sized.size = u32::MAX;
        assert_eq!(Archive::read(&archive(&[sized])).unwrap_err(), ZipError::Zip64);

        let file = archive(&[stored("a", b"x")]);
        let end = end_of(&file);
        let mut located = file[..end].to_vec();
        located.extend_from_slice(ZIP64_LOCATOR_SIGNATURE);
        located.extend_from_slice(&[0; 16]);
        located.extend_from_slice(&file[end..]);
        assert_eq!(Archive::read(&located).unwrap_err(), ZipError::Zip64);
        for (offset, len) in [(10, 2), (12, 4), (16, 4)] {
            let mut big = file.clone();
            big[end + offset..end + offset + len].fill(0xFF);
            assert_eq!(Archive::read(&big).unwrap_err(), ZipError::Zip64, "{offset}");
        }
    }

    #[test]
    fn entries_that_overlap_or_leave_the_file_are_refused() {
        let file = archive(&[stored("a", b"xxxx"), stored("b", b"yyyy")]);
        let directory_of = |file: &[u8]| {
            let end = end_of(file);
            u32_at(file, end + 16).unwrap() as usize
        };
        let first = directory_of(&file);
        let second = first + CENTRAL_SIZE + 1;
        let mut overlapping = file.clone();
        overlapping[second + 42..second + 46].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(Archive::read(&overlapping).unwrap_err(), ZipError::Damaged);
        let mut outside = file.clone();
        outside[first + 42..first + 46].copy_from_slice(&1_000_000u32.to_le_bytes());
        assert_eq!(Archive::read(&outside).unwrap_err(), ZipError::Damaged);
        let mut into_directory = file.clone();
        let huge = u32::try_from(first).unwrap();
        into_directory[first + 20..first + 24].copy_from_slice(&huge.to_le_bytes());
        into_directory[first + 24..first + 28].copy_from_slice(&huge.to_le_bytes());
        assert_eq!(Archive::read(&into_directory).unwrap_err(), ZipError::Damaged);
    }

    #[test]
    fn a_damaged_directory_is_found() {
        let file = archive(&[stored("a", b"x")]);
        let end = end_of(&file);
        let start = u32_at(&file, end + 16).unwrap() as usize;
        let changed = |at: usize, value: u8| {
            let mut bytes = file.clone();
            bytes[at] = value;
            Archive::read(&bytes).map(|_| ())
        };
        assert_eq!(changed(start, b'X'), Err(ZipError::Damaged), "signature");
        assert_eq!(changed(end + 4, 1), Err(ZipError::Damaged), "disk");
        assert_eq!(changed(end + 6, 1), Err(ZipError::Damaged), "directory disk");
        assert_eq!(changed(end + 8, 2), Err(ZipError::Damaged), "entries on disk");
        assert_eq!(
            changed(end + 12, 0xF0),
            Err(ZipError::Damaged),
            "directory beyond its end"
        );
        assert_eq!(changed(start + 10, 7), Err(ZipError::Damaged), "unknown method");
        assert_eq!(changed(start + 34, 1), Err(ZipError::Damaged), "other disk");
        assert_eq!(
            changed(start + 28, 0xFF),
            Err(ZipError::Damaged),
            "name beyond the directory"
        );
        assert_eq!(
            changed(start + 30, 0xFF),
            Err(ZipError::Damaged),
            "extra beyond the directory"
        );
        assert_eq!(
            changed(start + 32, 0xFF),
            Err(ZipError::Damaged),
            "comment beyond the directory"
        );
        assert_eq!(changed(start + 20, 9), Err(ZipError::Damaged), "stored with two sizes");
        assert_eq!(changed(0, b'X'), Err(ZipError::Damaged), "local signature");
    }

    #[test]
    fn a_name_twice_is_refused() {
        let file = archive(&[stored("a", b"x"), stored("a", b"y")]);
        assert_eq!(Archive::read(&file).unwrap_err(), ZipError::Damaged);
    }

    #[test]
    fn more_entries_than_allowed_are_refused() {
        let parts: Vec<Part> = (0..=MOST_ENTRIES).map(|n| stored(&n.to_string(), b"")).collect();
        assert_eq!(Archive::read(&archive(&parts)).unwrap_err(), ZipError::TooLarge);
        let fitting: Vec<Part> = (0..MOST_ENTRIES).map(|n| stored(&n.to_string(), b"")).collect();
        assert!(Archive::read(&archive(&fitting)).is_ok());
    }

    #[test]
    fn a_wrong_checksum_or_length_shows_when_reading() {
        let mut wrong_crc = stored("a", b"x");
        wrong_crc.crc ^= 1;
        let file = archive(&[wrong_crc]);
        assert_eq!(Archive::read(&file).unwrap().get("a"), Err(ZipError::Damaged));

        let mut lying = deflated("a", &[b'x'; 5000]);
        lying.size = 100;
        let file = archive(&[lying]);
        assert_eq!(
            Archive::read(&file).unwrap().get("a"),
            Err(ZipError::Damaged),
            "cut off at its word"
        );

        let mut short = deflated("a", b"xy");
        short.size = 100;
        let file = archive(&[short]);
        assert_eq!(Archive::read(&file).unwrap().get("a"), Err(ZipError::Damaged));

        let mut garbage = deflated("a", b"xy");
        garbage.stored = vec![0xFF; 10];
        let file = archive(&[garbage]);
        assert_eq!(Archive::read(&file).unwrap().get("a"), Err(ZipError::Damaged));
    }

    #[test]
    fn a_bomb_is_stopped_by_entry_and_by_total() {
        let mut big_entry = deflated("a", b"x");
        big_entry.size = u32::try_from(MOST_ENTRY_BYTES).unwrap() + 1;
        let file = archive(&[big_entry, stored("b", b"y")]);
        let read = Archive::read(&file).unwrap();
        assert_eq!(read.get("a"), Err(ZipError::EntryTooLarge));
        assert_eq!(read.get("b").unwrap().unwrap(), b"y", "the others can still be read");

        let one = deflated(
            "0",
            &vec![0u8; usize::try_from(MOST_ENTRY_BYTES).expect("a 64-bit Mac")],
        );
        let parts: Vec<Part> = (0..5)
            .map(|n| Part {
                name: n.to_string(),
                ..one.clone()
            })
            .collect();
        let file = archive(&parts);
        assert!(file.len() < 5 << 20, "{}", file.len());
        let read = Archive::read(&file).unwrap();
        for n in 0..4 {
            assert!(read.get(&n.to_string()).is_ok());
        }
        assert_eq!(read.get("4"), Err(ZipError::TooLarge));
    }
}
