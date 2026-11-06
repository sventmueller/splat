//! Pack files and their indexes, read by position and checked against each other before anything in them is believed.

use crate::ReadError;
use crate::inflate::Inflater;
use crate::object::{Kind, MAX_OBJECT, Oid};
use crate::source::{Source, read_exact_at};

const IDX_MAGIC: &[u8; 4] = b"\xfftOc";
/// Magic and version, then 256 counts: how many objects have a first byte up to that value.
const IDX_NAMES: u64 = 8 + 256 * 4;
/// Behind the names lie checksums and offsets of four bytes each, then eight-byte offsets, then two checksums of twenty bytes.
const IDX_PER_OBJECT: u64 = 20 + 4 + 4;
const IDX_TRAILER: u64 = 40;

const PACK_HEADER: u64 = 12;
const PACK_TRAILER: u64 = 20;

/// The longest entry header: a size of ten bytes and a base of twenty.
const LONGEST_ENTRY_HEADER: u64 = 30;

/// The index of a pack: which objects it holds and where.
pub struct Idx {
    source: Box<dyn Source>,
    fanout: Vec<u32>,
    objects: u32,
    wide_offsets: u64,
}

impl Idx {
    /// Checks everything that does not depend on the size of the pack: version, order of the counts and the size of the file.
    pub fn parse(source: Box<dyn Source>) -> Result<Idx, ReadError> {
        let size = source.size();
        if size < IDX_NAMES + IDX_TRAILER {
            return Err(ReadError::Damaged);
        }
        let mut head = [0; 1032];
        read_exact_at(&*source, &mut head, 0)?;
        if &head[..4] != IDX_MAGIC || head[4..8] != [0, 0, 0, 2] {
            return Err(ReadError::Unsupported);
        }
        let fanout: Vec<u32> = head[8..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|count| u32::from_be_bytes(*count))
            .collect();
        if fanout.windows(2).any(|pair| pair[0] > pair[1]) {
            return Err(ReadError::Damaged);
        }
        let objects = fanout[255];
        let narrow = IDX_NAMES + u64::from(objects) * IDX_PER_OBJECT + IDX_TRAILER;
        let wide = size.checked_sub(narrow).ok_or(ReadError::Damaged)?;
        // The first object of a pack lies at a short offset by necessity, which is why Git allows one wide offset less.
        if !wide.is_multiple_of(8) || wide / 8 > u64::from(objects).saturating_sub(1) {
            return Err(ReadError::Damaged);
        }
        Ok(Idx {
            source,
            fanout,
            objects,
            wide_offsets: wide / 8,
        })
    }

    /// How many objects the pack holds.
    pub fn objects(&self) -> u32 {
        self.objects
    }

    /// The name of the object at a place in the sorted list.
    pub fn name_at(&self, index: u32) -> Result<Oid, ReadError> {
        let mut name = [0; 20];
        read_exact_at(&*self.source, &mut name, IDX_NAMES + u64::from(index) * 20)?;
        Ok(Oid(name))
    }

    /// Where the pack holds an object, if it does; the pack still has to say whether that place exists.
    pub fn find(&self, oid: &Oid) -> Result<Option<u64>, ReadError> {
        let first = usize::from(oid.0[0]);
        let mut low = if first == 0 { 0 } else { self.fanout[first - 1] };
        let mut high = self.fanout[first];
        while low < high {
            let middle = low + (high - low) / 2;
            match self.name_at(middle)?.cmp(oid) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return self.offset_at(middle).map(Some),
            }
        }
        Ok(None)
    }

    /// An offset with its top bit set is the number of an eight-byte offset in the table behind the short ones.
    fn offset_at(&self, index: u32) -> Result<u64, ReadError> {
        let objects = u64::from(self.objects);
        let mut short = [0; 4];
        let table = IDX_NAMES + objects * 24;
        read_exact_at(&*self.source, &mut short, table + u64::from(index) * 4)?;
        let short = u32::from_be_bytes(short);
        if short & 0x8000_0000 == 0 {
            return Ok(u64::from(short));
        }
        let number = u64::from(short & 0x7fff_ffff);
        if number >= self.wide_offsets {
            return Err(ReadError::Damaged);
        }
        let mut wide = [0; 8];
        read_exact_at(&*self.source, &mut wide, table + objects * 4 + number * 8)?;
        Ok(u64::from_be_bytes(wide))
    }

    fn pack_checksum(&self) -> Result<[u8; 20], ReadError> {
        let mut checksum = [0; 20];
        read_exact_at(&*self.source, &mut checksum, self.source.size() - IDX_TRAILER)?;
        Ok(checksum)
    }
}

/// What a place in a pack holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Stored {
    /// An object as it is.
    Whole(Kind),
    /// A delta on the object at an earlier place of the same pack.
    DeltaOnOffset(u64),
    /// A delta on the object of that name.
    DeltaOnName(Oid),
}

/// The head of an entry of a pack.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Entry {
    pub(crate) stored: Stored,
    /// The size it declares of what is compressed in it, which for a delta is the delta.
    size: u64,
    /// Where the compressed data starts.
    data_at: u64,
}

/// A pack that fits its index.
pub(crate) struct Pack {
    source: Box<dyn Source>,
    entries_end: u64,
}

impl Pack {
    /// Holds a pack to its index by what both store: the number of objects and the checksum of the pack.
    pub(crate) fn parse(source: Box<dyn Source>, idx: &Idx) -> Result<Pack, ReadError> {
        let size = source.size();
        if size < PACK_HEADER + PACK_TRAILER {
            return Err(ReadError::Damaged);
        }
        let mut head = [0; 12];
        read_exact_at(&*source, &mut head, 0)?;
        if &head[..4] != b"PACK" {
            return Err(ReadError::Damaged);
        }
        if head[4..8] != [0, 0, 0, 2] {
            return Err(ReadError::Unsupported);
        }
        if head[8..12] != idx.objects.to_be_bytes() {
            return Err(ReadError::Damaged);
        }
        let entries_end = size - PACK_TRAILER;
        let mut trailer = [0; 20];
        read_exact_at(&*source, &mut trailer, entries_end)?;
        if trailer != idx.pack_checksum()? {
            return Err(ReadError::Damaged);
        }
        Ok(Pack { source, entries_end })
    }

    /// The head of the entry at `offset`, which comes from a file that may lie.
    pub(crate) fn entry(&self, offset: u64) -> Result<Entry, ReadError> {
        if offset < PACK_HEADER || offset >= self.entries_end {
            return Err(ReadError::Damaged);
        }
        let available = (self.entries_end - offset).min(LONGEST_ENTRY_HEADER);
        let mut head = vec![0; usize::try_from(available).map_err(|_| ReadError::Damaged)?];
        read_exact_at(&*self.source, &mut head, offset)?;
        let mut bytes = head.iter().copied();
        let mut read = 0;
        let mut next = || {
            read += 1;
            bytes.next().ok_or(ReadError::Damaged)
        };
        let first = next()?;
        let kind = first >> 4 & 7;
        let mut size = u64::from(first & 15);
        let mut more = first & 0x80 != 0;
        let mut shift = 4;
        while more {
            let byte = next()?;
            if shift > 60 {
                return Err(ReadError::Damaged);
            }
            size |= u64::from(byte & 0x7f) << shift;
            shift += 7;
            more = byte & 0x80 != 0;
        }
        let stored = match kind {
            1 => Stored::Whole(Kind::Commit),
            2 => Stored::Whole(Kind::Tree),
            3 => Stored::Whole(Kind::Blob),
            4 => Stored::Whole(Kind::Tag),
            6 => Stored::DeltaOnOffset(base_offset(offset, &mut next)?),
            7 => {
                let mut name = [0; 20];
                for byte in &mut name {
                    *byte = next()?;
                }
                Stored::DeltaOnName(Oid(name))
            }
            _ => return Err(ReadError::Damaged),
        };
        Ok(Entry {
            stored,
            size,
            data_at: offset + read,
        })
    }

    /// The compressed data of an entry, which has to inflate to exactly the size it declares.
    pub(crate) fn inflate(&self, entry: &Entry, limit: u64) -> Result<Vec<u8>, ReadError> {
        if entry.size > limit.min(MAX_OBJECT) {
            return Err(ReadError::TooLarge);
        }
        let size = usize::try_from(entry.size).map_err(|_| ReadError::TooLarge)?;
        Inflater::new(&*self.source, entry.data_at).finish(Vec::new(), size)
    }
}

/// The base of an offset delta lies this far before its entry, in a form where each further byte first adds one, so that no
/// value has two spellings. A base behind or at its own delta cannot exist, which is what makes such chains free of cycles.
fn base_offset(offset: u64, next: &mut impl FnMut() -> Result<u8, ReadError>) -> Result<u64, ReadError> {
    let mut byte = next()?;
    let mut distance = u64::from(byte & 0x7f);
    while byte & 0x80 != 0 {
        byte = next()?;
        distance = distance
            .checked_add(1)
            .and_then(|distance| distance.checked_mul(128))
            .ok_or(ReadError::Damaged)?
            | u64::from(byte & 0x7f);
    }
    if distance == 0 || distance > offset - PACK_HEADER {
        return Err(ReadError::Damaged);
    }
    Ok(offset - distance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::support::{Failing, Packer};

    fn pair(pack: Vec<u8>, idx: Vec<u8>) -> (Pack, Idx) {
        let idx = Idx::parse(Box::new(idx)).unwrap();
        (Pack::parse(Box::new(pack), &idx).unwrap(), idx)
    }

    fn three_objects() -> (Vec<u8>, Vec<u8>) {
        let mut packer = Packer::new();
        packer.whole(Oid([0x10; 20]), 3, b"first");
        packer.whole(Oid([0x80; 20]), 3, b"second");
        packer.whole(Oid([0x81; 20]), 3, b"third");
        packer.finish()
    }

    #[test]
    fn names_are_found_by_their_first_byte_and_then_by_search() {
        let (pack, idx) = three_objects();
        let (pack, idx) = pair(pack, idx);
        assert_eq!(idx.objects(), 3);
        assert_eq!(idx.name_at(1), Ok(Oid([0x80; 20])));
        for (name, text) in [(0x10, &b"first"[..]), (0x80, b"second"), (0x81, b"third")] {
            let offset = idx.find(&Oid([name; 20])).unwrap().unwrap();
            let entry = pack.entry(offset).unwrap();
            assert_eq!(entry.stored, Stored::Whole(Kind::Blob));
            assert_eq!(pack.inflate(&entry, MAX_OBJECT).as_deref(), Ok(text));
        }
        assert_eq!(idx.find(&Oid([0x00; 20])), Ok(None));
        assert_eq!(idx.find(&Oid([0x7f; 20])), Ok(None));
        assert_eq!(idx.find(&Oid([0x82; 20])), Ok(None));
        assert_eq!(idx.find(&Oid([0xff; 20])), Ok(None));
    }

    #[test]
    fn a_name_is_found_among_many_with_the_same_first_byte() {
        let mut packer = Packer::new();
        for second in 0..200 {
            let mut name = Oid([0x42; 20]);
            name.0[1] = second;
            packer.whole(name, 3, &[second]);
        }
        let (pack, idx) = packer.finish();
        let (pack, idx) = pair(pack, idx);
        for second in 0..200 {
            let mut name = Oid([0x42; 20]);
            name.0[1] = second;
            let entry = pack.entry(idx.find(&name).unwrap().unwrap()).unwrap();
            assert_eq!(pack.inflate(&entry, MAX_OBJECT), Ok(vec![second]));
        }
    }

    #[test]
    fn eight_byte_offsets_are_read_from_their_table() {
        let mut packer = Packer::new();
        packer.whole(Oid([1; 20]), 3, b"low");
        packer.whole(Oid([2; 20]), 3, b"high");
        let (pack, idx) = packer.finish_with_wide_offsets(&[Oid([2; 20])]);
        let (pack, idx) = pair(pack, idx);
        let entry = pack.entry(idx.find(&Oid([2; 20])).unwrap().unwrap()).unwrap();
        assert_eq!(pack.inflate(&entry, MAX_OBJECT).as_deref(), Ok(&b"high"[..]));
    }

    #[test]
    fn an_offset_that_names_no_entry_of_the_table_is_damaged() {
        let mut packer = Packer::new();
        packer.whole(Oid([1; 20]), 3, b"x");
        let (_, mut idx) = packer.finish();
        let table = 1032 + 24;
        idx[table..table + 4].copy_from_slice(&0x8000_0000u32.to_be_bytes());
        let idx = Idx::parse(Box::new(idx)).unwrap();
        assert_eq!(idx.find(&Oid([1; 20])), Err(ReadError::Damaged));
    }

    #[test]
    fn an_index_with_counts_that_fall_is_damaged() {
        let (_, mut idx) = three_objects();
        idx[8 + 4 * 7..8 + 4 * 8].copy_from_slice(&9u32.to_be_bytes());
        assert_eq!(Idx::parse(Box::new(idx)).err(), Some(ReadError::Damaged));
    }

    #[test]
    fn an_index_of_any_other_size_than_its_counts_say_is_damaged() {
        let (_, idx) = three_objects();
        for size in [0, 8, 1031, 1032, idx.len() - 1] {
            assert_eq!(
                Idx::parse(Box::new(idx[..size].to_vec())).err(),
                Some(ReadError::Damaged),
                "{size}"
            );
        }
        for extra in [1, 4, 7] {
            let mut longer = idx.clone();
            longer.resize(idx.len() + extra, 0);
            assert_eq!(Idx::parse(Box::new(longer)).err(), Some(ReadError::Damaged), "{extra}");
        }
    }

    #[test]
    fn an_index_cannot_claim_a_wide_offset_for_every_object() {
        let (_, idx) = three_objects();
        let mut longer = idx.clone();
        longer.resize(idx.len() + 8 * 2, 0);
        assert!(Idx::parse(Box::new(longer.clone())).is_ok());
        longer.resize(idx.len() + 8 * 3, 0);
        assert_eq!(Idx::parse(Box::new(longer)).err(), Some(ReadError::Damaged));
    }

    #[test]
    fn an_index_of_no_objects_has_no_wide_offsets_either() {
        let (_, idx) = Packer::new().finish();
        assert_eq!(Idx::parse(Box::new(idx.clone())).map(|idx| idx.objects()).ok(), Some(0));
        let mut longer = idx;
        longer.resize(longer.len() + 8, 0);
        assert_eq!(Idx::parse(Box::new(longer)).err(), Some(ReadError::Damaged));
    }

    #[test]
    fn counts_that_claim_more_objects_than_the_file_holds_are_damaged() {
        let (_, mut idx) = three_objects();
        idx[8 + 4 * 255..8 + 4 * 256].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(Idx::parse(Box::new(idx)).err(), Some(ReadError::Damaged));
    }

    #[test]
    fn an_index_of_another_version_cannot_be_read() {
        let (_, idx) = three_objects();
        let mut other_version = idx.clone();
        other_version[7] = 3;
        assert_eq!(Idx::parse(Box::new(other_version)).err(), Some(ReadError::Unsupported));
        let mut no_magic = idx;
        no_magic[0] = 0;
        assert_eq!(Idx::parse(Box::new(no_magic)).err(), Some(ReadError::Unsupported));
    }

    #[test]
    fn a_pack_that_is_not_the_one_of_its_index_is_damaged() {
        let (pack, idx) = three_objects();
        let idx = Idx::parse(Box::new(idx)).unwrap();
        let mut other_count = pack.clone();
        other_count[11] = 9;
        assert_eq!(Pack::parse(Box::new(other_count), &idx).err(), Some(ReadError::Damaged));
        let mut other_checksum = pack.clone();
        *other_checksum.last_mut().unwrap() ^= 1;
        assert_eq!(
            Pack::parse(Box::new(other_checksum), &idx).err(),
            Some(ReadError::Damaged)
        );
        let mut no_magic = pack.clone();
        no_magic[0] = b'Q';
        assert_eq!(Pack::parse(Box::new(no_magic), &idx).err(), Some(ReadError::Damaged));
        assert_eq!(
            Pack::parse(Box::new(pack[..31].to_vec()), &idx).err(),
            Some(ReadError::Damaged)
        );
    }

    #[test]
    fn a_pack_of_another_version_cannot_be_read() {
        let (mut pack, idx) = three_objects();
        pack[7] = 3;
        let idx = Idx::parse(Box::new(idx)).unwrap();
        assert_eq!(Pack::parse(Box::new(pack), &idx).err(), Some(ReadError::Unsupported));
    }

    #[test]
    fn a_truncated_pack_no_longer_fits_its_index() {
        let (pack, idx) = three_objects();
        let idx = Idx::parse(Box::new(idx)).unwrap();
        let cut = pack[..pack.len() - 5].to_vec();
        assert_eq!(Pack::parse(Box::new(cut), &idx).err(), Some(ReadError::Damaged));
    }

    #[test]
    fn offsets_outside_the_entries_are_damaged() {
        let (pack, idx) = three_objects();
        let size = pack.len() as u64;
        let (pack, _) = pair(pack, idx);
        for offset in [0, 11, size - 20, size, size + 1, u64::MAX] {
            assert_eq!(pack.entry(offset), Err(ReadError::Damaged), "{offset}");
        }
    }

    #[test]
    fn a_header_that_runs_out_of_the_entries_is_damaged() {
        let mut packer = Packer::new();
        packer.raw(Oid([1; 20]), vec![0xb0]);
        let (pack, idx) = packer.finish();
        let (pack, _) = pair(pack, idx);
        assert_eq!(pack.entry(12), Err(ReadError::Damaged));
    }

    #[test]
    fn a_size_of_more_bits_than_any_object_has_is_damaged() {
        let mut packer = Packer::new();
        packer.raw(Oid([1; 20]), vec![0xb0; 29]);
        let (pack, idx) = packer.finish();
        let (pack, _) = pair(pack, idx);
        assert_eq!(pack.entry(12), Err(ReadError::Damaged));
    }

    #[test]
    fn unknown_kinds_are_damaged() {
        for kind in [0u8, 5] {
            let mut packer = Packer::new();
            packer.raw(Oid([1; 20]), vec![kind << 4 | 1, 0, 0, 0]);
            let (pack, idx) = packer.finish();
            let (pack, _) = pair(pack, idx);
            assert_eq!(pack.entry(12), Err(ReadError::Damaged), "{kind}");
        }
    }

    #[test]
    fn every_whole_kind_has_its_number() {
        for (number, kind) in [(1u8, Kind::Commit), (2, Kind::Tree), (3, Kind::Blob), (4, Kind::Tag)] {
            let mut packer = Packer::new();
            packer.whole(Oid([1; 20]), number, b"x");
            let (pack, idx) = packer.finish();
            let (pack, _) = pair(pack, idx);
            assert_eq!(pack.entry(12).unwrap().stored, Stored::Whole(kind));
        }
    }

    #[test]
    fn a_size_above_the_limit_is_too_large_before_anything_inflates() {
        let mut packer = Packer::new();
        packer.declared(Oid([1; 20]), 3, MAX_OBJECT + 1, b"tiny");
        packer.declared(Oid([2; 20]), 2, 100, b"tiny");
        let (pack, idx) = packer.finish();
        let (pack, idx) = pair(pack, idx);
        let huge = pack.entry(idx.find(&Oid([1; 20])).unwrap().unwrap()).unwrap();
        assert_eq!(pack.inflate(&huge, MAX_OBJECT), Err(ReadError::TooLarge));
        let tree = pack.entry(idx.find(&Oid([2; 20])).unwrap().unwrap()).unwrap();
        assert_eq!(pack.inflate(&tree, 99), Err(ReadError::TooLarge));
    }

    #[test]
    fn a_size_that_does_not_fit_the_data_is_damaged() {
        let mut packer = Packer::new();
        packer.declared(Oid([1; 20]), 3, 3, b"longer than three");
        let (pack, idx) = packer.finish();
        let (pack, _) = pair(pack, idx);
        let entry = pack.entry(12).unwrap();
        assert_eq!(pack.inflate(&entry, MAX_OBJECT), Err(ReadError::Damaged));
    }

    #[test]
    fn an_offset_delta_names_an_earlier_entry() {
        let mut packer = Packer::new();
        let base = packer.whole(Oid([1; 20]), 3, b"base");
        let far = packer.ofs_delta_at_distance(Oid([2; 20]), 0, b"ignored");
        let (pack, idx) = packer.finish();
        let (pack, _) = pair(pack, idx);
        assert_eq!(pack.entry(far), Err(ReadError::Damaged), "a delta on itself");
        assert_eq!(base, 12);
    }

    #[test]
    fn a_base_before_the_first_entry_is_damaged() {
        let mut packer = Packer::new();
        let first = packer.ofs_delta_at_distance(Oid([1; 20]), 1, b"x");
        let second = packer.ofs_delta_at_distance(Oid([2; 20]), 500, b"x");
        let (pack, idx) = packer.finish();
        let (pack, _) = pair(pack, idx);
        assert_eq!(pack.entry(first), Err(ReadError::Damaged));
        assert_eq!(pack.entry(second), Err(ReadError::Damaged));
    }

    #[test]
    fn distances_of_many_bytes_are_read_as_git_writes_them() {
        let mut packer = Packer::new();
        let base = packer.whole(Oid([1; 20]), 3, &[7; 5000]);
        let delta = packer.ofs_delta(Oid([2; 20]), base, b"delta");
        let (pack, idx) = packer.finish();
        let (pack, _) = pair(pack, idx);
        assert_eq!(pack.entry(delta).unwrap().stored, Stored::DeltaOnOffset(base));
    }

    #[test]
    fn a_distance_too_long_for_any_file_is_damaged() {
        let mut packer = Packer::new();
        let mut bytes = vec![0x60];
        bytes.extend_from_slice(&[0xff; 12]);
        bytes.push(0x7f);
        packer.raw(Oid([1; 20]), bytes);
        let (pack, idx) = packer.finish();
        let (pack, _) = pair(pack, idx);
        assert_eq!(pack.entry(12), Err(ReadError::Damaged));
    }

    #[test]
    fn a_delta_on_a_name_carries_the_name() {
        let mut packer = Packer::new();
        packer.ref_delta(Oid([1; 20]), Oid([9; 20]), b"delta");
        let (pack, idx) = packer.finish();
        let (pack, _) = pair(pack, idx);
        assert_eq!(pack.entry(12).unwrap().stored, Stored::DeltaOnName(Oid([9; 20])));
    }

    #[test]
    fn a_failing_volume_is_no_damage() {
        assert_eq!(
            Idx::parse(Box::new(Failing::sized(2000))).err(),
            Some(ReadError::Io(std::io::ErrorKind::Other))
        );
        let (_, idx) = three_objects();
        let idx = Idx::parse(Box::new(idx)).unwrap();
        assert_eq!(
            Pack::parse(Box::new(Failing::sized(2000)), &idx).err(),
            Some(ReadError::Io(std::io::ErrorKind::Other))
        );
    }
}
